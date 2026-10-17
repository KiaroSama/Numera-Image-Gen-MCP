mod common;
#[path = "common/wire.rs"]
mod wire;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::{Config, snapshot_credentials},
    error::Result,
    jobs::{Receipt, Store},
    logging::Logger,
    mcp::application::Application,
    runtime_paths::RuntimePaths,
    services::identity::connection_identity,
};
use std::{
    collections::HashMap,
    future::Future,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

// Application is the actual public handler dispatch seam, not an SDK envelope fixture.
struct Host(Application);
impl Drop for Host {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn host(root: &Path, origin: &str, cancellation: bool) -> Host {
    let mut connection = json!({
        "adapter":if cancellation {"comfyui"} else {"openai-responses"},
        "baseUrl":origin,"auth":{"type":"none"}
    });
    if cancellation {
        connection["baseUrlMode"] = json!("origin");
    } else {
        connection["defaultModel"] = json!("image-model");
    }
    let config = Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":connection}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    Host(
        Application::new(
            config,
            None,
            Arc::new(Logger::new(&root.join("logs"), "ERROR")),
            RuntimePaths::new(root).unwrap(),
            HashMap::new(),
            CancellationToken::new(),
        )
        .unwrap(),
    )
}

fn receipt(id: &str) -> Receipt {
    Receipt::new(json!({
        "request_id":id,"connection":"local","adapter":"openai-images",
        "operation":"generate","status":"prepared","requested_model":"image-model",
        "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
        "generation_outcome":"not_submitted","storage_outcome":"not_started",
        "another_submission_may_charge":false,
        "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
    }))
}

async fn seed(host: &Host, kind: &str) -> String {
    let current = host.0.current().unwrap();
    let (_, connection) = current.config.select(Some("local")).unwrap();
    assert_eq!(connection["requestTimeoutMs"], 300000);
    assert_eq!(connection["discoveryTimeoutMs"], 15000);
    let pinned = snapshot_credentials(&connection, &current.env, &current.runtime)
        .await
        .unwrap();
    let identity = connection_identity("local", &pinned).unwrap();
    let mut store = host.0.store.lock().unwrap();
    let mut old = store.prepare(receipt("old"), "hash", &identity).unwrap().1;
    assert!(store.admit("old", "local", 1, 1).unwrap());
    old.value["status"] = json!("running");
    old.value["generation_outcome"] = json!("running");
    old.value["upstream_job"] = json!({"id":"job","kind":kind});
    store.update(&mut old).unwrap();
    store.release("old", Some(&old)).unwrap();
    identity
}

async fn call(host: &Host, name: &str, arguments: Value) -> Result<Value> {
    let signal = CancellationToken::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        host.0.call(name, arguments, &signal),
    )
    .await
    .expect("Public call exceeded the source five-second bound")
}

fn request_shape(request: &wire::Request, cancellation: bool) {
    assert!(
        !request
            .headers
            .to_ascii_lowercase()
            .contains("authorization:")
    );
    if cancellation {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/jobs/job/cancel");
        let body =
            Value::parse_with_limit(std::str::from_utf8(&request.body).unwrap(), 1048576).unwrap();
        assert_eq!(body, json!({}));
    } else {
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/responses/job");
        assert!(request.body.is_empty());
    }
}

// The shared wire fixture cannot disconnect before headers or omit Content-Type. This
// reader is used only by those fault schedules; normal JSON acknowledgements reuse wire.
async fn capture(socket: &mut TcpStream) -> Option<wire::Request> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = match socket.read(&mut buffer).await {
            Ok(0) => return None,
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ) =>
            {
                return None;
            }
            Err(error) => panic!("Fault fixture socket read failed: {error}"),
        };
        bytes.extend_from_slice(&buffer[..read]);
        assert!(bytes.len() <= 1048576 + 65536, "bounded fault request");
        let Some(boundary) = wire::find(&bytes, b"\r\n\r\n") else {
            assert!(bytes.len() <= 65536, "bounded fault headers");
            continue;
        };
        assert!(boundary <= 65536, "bounded fault headers");
        let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
        let length = wire::header(headers, "content-length")
            .map(|value| value.parse::<usize>().unwrap())
            .unwrap_or(0);
        assert!(length <= 1048576, "bounded fault body");
        if bytes.len() < boundary + 4 + length {
            continue;
        }
        assert_eq!(bytes.len(), boundary + 4 + length);
        let mut first = headers.lines().next().unwrap().split_whitespace();
        return Some(wire::Request {
            method: first.next().unwrap().to_owned(),
            path: first.next().unwrap().to_owned(),
            headers: headers.to_owned(),
            body: bytes[boundary + 4..].to_vec(),
        });
    }
}

async fn fault<F, A>(mode: &str, action: F)
where
    F: FnOnce(String, Arc<Mutex<Vec<wire::Request>>>) -> A,
    A: Future<Output = ()>,
{
    assert!(["disconnect", "malformed", "ambiguous"].contains(&mode));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let server = async {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                let Some(request) = capture(&mut socket).await else {
                    return;
                };
                request_shape(&request, mode == "ambiguous");
                captured.lock().unwrap().push(request);
                if mode == "malformed" {
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot-json",
                        )
                        .await
                        .unwrap();
                    socket.shutdown().await.unwrap();
                }
                // Disconnect/ambiguous send no HTTP bytes, matching source res.destroy().
            })
            .await
            .expect("Owned fault socket exceeded three seconds");
        }
    };
    tokio::time::timeout(Duration::from_secs(8), async {
        tokio::select! {
            () = action(origin, requests) => {},
            () = server => panic!("Fault listener ended unexpectedly"),
        }
    })
    .await
    .expect("Owned fault action exceeded eight seconds");
}

async fn poll(workspace: &Workspace, mode: &str) {
    let root = &workspace.0;
    fault(mode, |origin, requests| async move {
        let host = host(root, &origin, false);
        let identity = seed(&host, "responses").await;
        // Source does not inspect the tool reply; persisted state/capacity and one GET
        // must establish the result even if the handler would return an error envelope.
        let _ = call(&host, "get_job", json!({"request_id":"old","refresh":true})).await;
        {
            let mut store = host.0.store.lock().unwrap();
            let old = store.get("old").unwrap();
            assert!(old.value.get("upstream_terminal").is_none());
            assert_eq!(old.status(), "running");
            store.prepare(receipt("fresh"), "fresh", &identity).unwrap();
            assert!(!store.admit("fresh", "local", 1, 1).unwrap());
        }
        let captured = requests.lock().unwrap();
        assert_eq!(
            captured
                .iter()
                .filter(|request| request.method == "POST")
                .count(),
            0
        );
        assert_eq!(
            captured
                .iter()
                .filter(|request| request.method == "GET")
                .count(),
            1
        );
    })
    .await;
}

async fn cancellation(
    root: &Path,
    origin: &str,
    requests: &Arc<Mutex<Vec<wire::Request>>>,
    mode: &str,
) {
    let host = host(root, origin, true);
    let identity = seed(&host, "comfyui").await;
    for _ in 0..2 {
        let result = call(&host, "cancel_job", json!({"request_id":"old"})).await;
        if mode == "false" {
            let result = result.unwrap();
            assert_eq!(result["refund_verified"], false);
            assert_eq!(result["upstream_cancelled"], false);
        }
    }
    drop(host);
    for n in 0..2 {
        let mut store = Store::open(&root.join("state")).unwrap();
        assert_eq!(store.get("old").unwrap().status(), "running");
        let id = format!("fresh-{n}");
        store.prepare(receipt(&id), &id, &identity).unwrap();
        assert!(!store.admit(&id, "local", 1, 1).unwrap());
    }
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 2);
    for request in captured.iter() {
        request_shape(request, true);
    }
    assert_eq!(
        captured
            .iter()
            .filter(|request| request.method == "POST" && !request.path.ends_with("/cancel"))
            .count(),
        0
    );
    assert_eq!(
        captured
            .iter()
            .filter(|request| request.method == "POST" && request.path.ends_with("/cancel"))
            .count(),
        2
    );
}

async fn cancel(workspace: &Workspace, mode: &str) {
    let root = &workspace.0;
    if mode == "false" {
        wire::serve(
            |request| {
                request_shape(request, true);
                wire::json_reply(json!({"cancelled":false}))
            },
            |origin, requests| async move {
                cancellation(root, &origin, &requests, mode).await;
            },
        )
        .await;
    } else {
        assert_eq!(mode, "ambiguous");
        fault(mode, |origin, requests| async move {
            cancellation(root, &origin, &requests, mode).await;
        })
        .await;
    }
}

// Runtime owns pending lease-task teardown and drops before synchronous Workspace cleanup.
#[test]
fn exact_source_disconnect_poll_retains_running_job_and_live_capacity() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(poll(&workspace, "disconnect"));
}

#[test]
fn exact_source_malformed_poll_retains_running_job_and_live_capacity() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(poll(&workspace, "malformed"));
}

#[test]
fn exact_source_false_cancel_ack_preserves_capacity_across_two_reopens_without_generation() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(cancel(&workspace, "false"));
}

#[test]
fn exact_source_ambiguous_cancel_preserves_capacity_across_two_reopens_without_generation() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(cancel(&workspace, "ambiguous"));
}
