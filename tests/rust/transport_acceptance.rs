mod common;
#[path = "common/wire.rs"]
mod wire;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::{Config, snapshot_credentials},
    files::images::{decode_base64, inspect},
    jobs::Receipt,
    logging::Logger,
    mcp::application::Application,
    runtime_paths::RuntimePaths,
    services::identity::connection_identity,
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

// Application is the RMCP handler's dispatch seam, not an SDK/protocol envelope fixture.
struct Host(Application);
impl Drop for Host {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn host(root: &Path, adapter: &str, origin: &str) -> Host {
    let config = Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":{
                "adapter":adapter,"baseUrl":format!("{origin}/v1"),
                "auth":{"type":"none"},"orchestrationModel":"text","defaultModel":"image"
            }}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let logger = Arc::new(Logger::new(&root.join("logs"), "ERROR"));
    Host(
        Application::new(
            config,
            None,
            logger,
            RuntimePaths::new(root).unwrap(),
            HashMap::new(),
            CancellationToken::new(),
        )
        .unwrap(),
    )
}

async fn call(host: &Host, name: &str, arguments: Value) -> Value {
    let signal = CancellationToken::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        host.0.call(name, arguments, &signal),
    )
    .await
    .expect("Public application call exceeded source five-second bound")
    .unwrap()
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

fn original() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
    assert!(std::fs::metadata(&path).unwrap().len() <= 1048576);
    let rows = Value::parse_with_limit(&std::fs::read_to_string(path).unwrap(), 1048576).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "png")
        .unwrap();
    let bytes = decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    assert_eq!(image.bytes, bytes);
    bytes
}

fn saved_original(result: &Value, bytes: &[u8]) {
    let outputs = result["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 1);
    assert_eq!(
        std::fs::read(outputs[0]["path"].as_str().unwrap()).unwrap(),
        bytes
    );
}

fn submission(request: &wire::Request, adapter: &str) {
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path,
        if adapter == "openai-responses" {
            "/v1/responses"
        } else {
            "/v1/interactions"
        }
    );
    assert!(
        !request
            .headers
            .to_ascii_lowercase()
            .contains("authorization:")
    );
    let body =
        Value::parse_with_limit(std::str::from_utf8(&request.body).unwrap(), 1048576).unwrap();
    assert_eq!(body["store"], false);
    if adapter == "openai-responses" {
        assert_eq!(body["model"], "text");
        assert_eq!(body["input"][0]["content"][0]["text"], "fixture");
        assert_eq!(body["tools"][0]["model"], "image");
        assert_eq!(body["tools"][0]["action"], "generate");
    } else {
        assert_eq!(body["model"], "image");
        assert_eq!(body["input"][0]["text"], "fixture");
        assert_eq!(body["response_format"]["type"], "image");
    }
}

// The shared wire fixture writes whole bodies; this bounded reader supports the source's
// two physical SSE writes without changing the shared fixture or spawning a server task.
async fn capture(socket: &mut TcpStream) -> wire::Request {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "request ended before its complete body");
        bytes.extend_from_slice(&buffer[..read]);
        assert!(
            bytes.len() <= 1048576 + 65536,
            "bounded source fixture request"
        );
        let Some(boundary) = wire::find(&bytes, b"\r\n\r\n") else {
            assert!(bytes.len() <= 65536, "bounded request headers");
            continue;
        };
        assert!(boundary <= 65536, "bounded request headers");
        let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
        let length = wire::header(headers, "content-length")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert!(length <= 1048576, "bounded request body");
        if bytes.len() < boundary + 4 + length {
            continue;
        }
        assert_eq!(bytes.len(), boundary + 4 + length);
        let mut first = headers.lines().next().unwrap().split_whitespace();
        return wire::Request {
            method: first.next().unwrap().to_owned(),
            path: first.next().unwrap().to_owned(),
            headers: headers.to_owned(),
            body: bytes[boundary + 4..].to_vec(),
        };
    }
}

async fn terminal_sse(workspace: &Workspace, state: &str) {
    let bytes = original();
    let data = json!({"response":{
        "id":"owned","status":state,
        "output":if state == "incomplete" {
            json!([{"type":"image_generation_call","status":"completed","result":STANDARD.encode(&bytes)}])
        } else { json!([]) }
    }});
    let frame = format!("event: response.{state}\ndata: {data}\n\n").into_bytes();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let posts = AtomicUsize::new(0);
    let server = async {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                let request = capture(&mut socket).await;
                submission(&request, "openai-responses");
                assert!(posts.fetch_add(1, Ordering::SeqCst) < 2);
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .unwrap();
                for chunk in [&frame[..19], &frame[19..]] {
                    socket
                        .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                        .await
                        .unwrap();
                    socket.write_all(chunk).await.unwrap();
                    socket.write_all(b"\r\n").await.unwrap();
                }
                socket.write_all(b"0\r\n\r\n").await.unwrap();
                socket.shutdown().await.unwrap();
            })
            .await
            .expect("Fragmented SSE socket exceeded three seconds");
        }
    };
    let action = async {
        let host = host(&workspace.0, "openai-responses", &origin);
        let arguments = json!({"prompt":"fixture","request_id":"sse"});
        let first = call(&host, "generate_image", arguments.clone()).await;
        assert_eq!(first["generation_outcome"], state);
        assert_eq!(
            first["status"],
            if state == "failed" {
                "failed"
            } else {
                "partial"
            }
        );
        if state == "incomplete" {
            saved_original(&first, &bytes);
        }
        call(&host, "generate_image", arguments).await;
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        let mut store = host.0.store.lock().unwrap();
        store.prepare(receipt("next"), "next", "fixture").unwrap();
        assert!(store.admit("next", "local", 1, 1).unwrap());
    };
    tokio::time::timeout(Duration::from_secs(8), async {
        tokio::select! {
            () = action => {},
            () = server => panic!("Fragmented SSE listener ended unexpectedly"),
        }
    })
    .await
    .expect("Scoped fragmented SSE action exceeded eight seconds");
}

async fn uncertain_sse(workspace: &Workspace, mode: &str) {
    let root = &workspace.0;
    let frame = if mode == "error" {
        "event: error\ndata: {\"error\":\"synthetic\"}\n\n".to_owned()
    } else {
        let mut response = json!({"status":"failed"});
        if mode == "empty-terminal-id" {
            response["id"] = json!("");
        } else if mode == "blank-terminal-id" {
            response["id"] = json!(" ");
        }
        format!(
            "event: response.failed\ndata: {}\n\n",
            json!({"response":response})
        )
    };
    wire::serve(
        |request| {
            submission(request, "openai-responses");
            wire::Reply {
                status: 200,
                mime: "text/event-stream",
                body: frame.as_bytes().to_vec(),
            }
        },
        |origin, requests| async move {
            let host = host(root, "openai-responses", &origin);
            let result = call(
                &host,
                "generate_image",
                json!({"prompt":"fixture","request_id":"uncertain"}),
            )
            .await;
            assert_eq!(result["status"], "outcome_unknown");
            assert_eq!(result["outputs"], json!([]));
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.method == "POST")
                    .count(),
                1
            );
        },
    )
    .await;
}

fn completed(adapter: &str, returned: Option<&str>, encoded: &str) -> Value {
    let mut response = json!({"status":"completed"});
    if let Some(id) = returned {
        response["id"] = json!(id);
    }
    if adapter == "openai-responses" {
        response["output"] = json!([{
            "type":"image_generation_call","status":"completed","result":encoded
        }]);
    } else {
        response["steps"] = json!([{
            "type":"model_output","content":[{"type":"image","data":encoded}]
        }]);
    }
    response
}

async fn job_identity(workspace: &Workspace, adapter: &str, returned: Option<&str>) {
    let bytes = original();
    let response = completed(adapter, returned, &STANDARD.encode(&bytes));
    let kind = if adapter == "openai-responses" {
        "responses"
    } else {
        "interactions"
    };
    let (root, bytes) = (&workspace.0, bytes.as_slice());
    wire::serve(
        |request| {
            if request.method == "POST" {
                submission(request, adapter);
                wire::json_reply(json!({"id":"owned","status":"in_progress"}))
            } else {
                assert_eq!(request.method, "GET");
                assert_eq!(request.path, format!("/v1/{kind}/owned"));
                assert!(
                    !request
                        .headers
                        .to_ascii_lowercase()
                        .contains("authorization:")
                );
                assert!(request.body.is_empty());
                wire::json_reply(response.clone())
            }
        },
        |origin, requests| async move {
            let host = host(root, adapter, &origin);
            let first = call(
                &host,
                "generate_image",
                json!({"prompt":"fixture","request_id":"waiting"}),
            )
            .await;
            {
                let current = host.0.current().unwrap();
                let (_, connection) = current.config.select(Some("local")).unwrap();
                let pinned = snapshot_credentials(&connection, &current.env, &current.runtime)
                    .await
                    .unwrap();
                let identity = connection_identity("local", &pinned).unwrap();
                let mut store = host.0.store.lock().unwrap();
                let mut saved = store
                    .prepare(receipt("detached"), "fixture", &identity)
                    .unwrap()
                    .1;
                assert!(store.admit("detached", "local", 4, 4).unwrap());
                saved.value["status"] = json!("running");
                saved.value["generation_outcome"] = json!("running");
                saved.value["adapter"] = json!(adapter);
                saved.value["upstream_job"] = json!({"id":"owned","kind":kind});
                store.update(&mut saved).unwrap();
                store.release("detached", Some(&saved)).unwrap();
            }
            let second = call(
                &host,
                "get_job",
                json!({"request_id":"detached","refresh":true}),
            )
            .await;
            for result in [&first, &second] {
                if returned == Some("owned") {
                    saved_original(result, bytes);
                } else {
                    assert_eq!(result["status"], "running");
                    assert_eq!(result["outputs"], json!([]));
                    assert!(
                        result["errors"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|e| e["code"] == "invalid_response")
                    );
                }
            }
            let captured = requests.lock().unwrap();
            assert_eq!(captured.iter().filter(|r| r.method == "POST").count(), 1);
            assert_eq!(captured.iter().filter(|r| r.method == "GET").count(), 2);
        },
    )
    .await;
}

// Runtime drops pending lease futures before synchronous workspace cleanup, also on unwind.
#[test]
fn exact_source_failed_sse_preserves_terminal_state_and_admission() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(terminal_sse(&workspace, "failed"));
}

#[test]
fn exact_source_incomplete_sse_retains_original_and_terminal_admission() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(terminal_sse(&workspace, "incomplete"));
}

#[test]
fn exact_source_error_sse_remains_uncertain_without_resubmission() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(uncertain_sse(&workspace, "error"));
}

#[test]
fn exact_source_malformed_terminal_sse_remains_uncertain_without_resubmission() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(uncertain_sse(&workspace, "malformed-terminal"));
}

#[test]
fn exact_source_empty_terminal_id_sse_remains_uncertain_without_resubmission() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(uncertain_sse(&workspace, "empty-terminal-id"));
}

#[test]
fn exact_source_blank_terminal_id_sse_remains_uncertain_without_resubmission() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(uncertain_sse(&workspace, "blank-terminal-id"));
}

#[test]
fn exact_source_responses_other_id_rejects_waiter_and_refresh() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(job_identity(&workspace, "openai-responses", Some("other")));
}

#[test]
fn exact_source_responses_missing_id_rejects_waiter_and_refresh() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(job_identity(&workspace, "openai-responses", None));
}

#[test]
fn exact_source_responses_owned_id_retains_waiter_and_refresh() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(job_identity(&workspace, "openai-responses", Some("owned")));
}

#[test]
fn exact_source_interactions_other_id_rejects_waiter_and_refresh() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(job_identity(
        &workspace,
        "gemini-interactions",
        Some("other"),
    ));
}

#[test]
fn exact_source_interactions_missing_id_rejects_waiter_and_refresh() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(job_identity(&workspace, "gemini-interactions", None));
}

#[test]
fn exact_source_interactions_owned_id_retains_waiter_and_refresh() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(job_identity(
        &workspace,
        "gemini-interactions",
        Some("owned"),
    ));
}
