mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    files::images::{decode_base64, inspect},
    jobs::Store,
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use tokio_util::sync::CancellationToken;

fn generation(root: &Path, origin: &str) -> Generation {
    let config = Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":{
                "adapter":"comfyui","baseUrl":origin,"baseUrlMode":"origin",
                "auth":{"type":"none"},"defaultModel":"workflow",
                "workflow":{
                    "graph":{
                        "text":{"class_type":"Text","inputs":{"text":""}},
                        "save":{"class_type":"Save","inputs":{"text":"output"}}
                    },
                    "bindings":{"prompt":{"node":"text","input":"text"}},
                    "outputNodes":["save"]
                }
            }}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let (_, connection) = config.select(None).unwrap();
    assert_eq!(connection["requestTimeoutMs"], 300000);
    assert_eq!(connection["discoveryTimeoutMs"], 15000);
    assert_eq!(connection["maxConcurrentRequests"], 2);
    assert_eq!(connection["workflow"]["requireGpu"], true);
    Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(Store::open(&root.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&root.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(root).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
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

struct Request {
    method: String,
    target: String,
    body: Vec<u8>,
}

// The shared synchronous wire responder cannot retain one history socket while accepting
// another. These bounded I/O helpers stay local to this event-controlled fixture.
async fn request(socket: &mut TcpStream) -> Request {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0, "request ended before its complete body");
            bytes.extend_from_slice(&buffer[..read]);
            assert!(bytes.len() <= 1048576 + 65536, "bounded race request");
            let Some(boundary) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
                assert!(bytes.len() <= 65536, "bounded race headers");
                continue;
            };
            assert!(boundary <= 65536, "bounded race headers");
            let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            assert!(length <= 1048576, "bounded race body");
            if bytes.len() < boundary + 4 + length {
                continue;
            }
            assert_eq!(bytes.len(), boundary + 4 + length);
            let mut first = headers.lines().next().unwrap().split_whitespace();
            return Request {
                method: first.next().unwrap().to_owned(),
                target: first.next().unwrap().to_owned(),
                body: bytes[boundary + 4..].to_vec(),
            };
        }
    })
    .await
    .expect("Owned Comfy race request exceeded three seconds")
}

async fn reply(socket: &mut TcpStream, status: u16, mime: &str, bytes: &[u8]) {
    let headers = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len(),
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(bytes).await.unwrap();
        socket.shutdown().await.unwrap();
    })
    .await
    .expect("Owned Comfy race response exceeded three seconds");
}

async fn json_reply(socket: &mut TcpStream, value: &Value) {
    reply(
        socket,
        200,
        "application/json",
        value.to_string().as_bytes(),
    )
    .await;
}

async fn scenario(workspace: &Workspace, bytes: &[u8]) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let generation = generation(&workspace.0, &origin);
    let signal = CancellationToken::new();
    let (ready, mut waiting) = oneshot::channel::<()>();
    let (release, mut released) = oneshot::channel::<()>();
    let posts = AtomicUsize::new(0);
    let history = json!({"race":{
        "status":{"completed":true,"status_str":"success"},
        "outputs":{"save":{"images":[{
            "filename":"final.png","subfolder":"","type":"output"
        }]}}
    }});
    let server = async {
        let mut first_history: Option<TcpStream> = None;
        let mut ready = Some(ready);
        let mut history_calls = 0;
        let mut release_seen = false;
        loop {
            tokio::select! {
                released = &mut released, if !release_seen => {
                    released.expect("Client must release its held original history");
                    release_seen = true;
                    let mut first = first_history.take().expect("First history socket must remain owned");
                    json_reply(&mut first, &history).await;
                },
                accepted = listener.accept() => {
                    let (mut socket, _) = accepted.unwrap();
                    let request = request(&mut socket).await;
                    let url = url::Url::parse(&format!("http://fixture{}", request.target)).unwrap();
                    match url.path() {
                        "/object_info" => {
                            assert_eq!(request.method, "GET");
                            json_reply(&mut socket, &json!({
                                "Text":{"input":{"required":{"text":["STRING"]}}},
                                "Save":{"output_node":true,"input":{"required":{"text":["STRING"]}}}
                            })).await;
                        },
                        "/system_stats" => {
                            assert_eq!(request.method, "GET");
                            json_reply(&mut socket, &json!({"devices":[{"type":"cuda"}]})).await;
                        },
                        "/prompt" => {
                            assert_eq!(request.method, "POST");
                            posts.fetch_add(1, Ordering::SeqCst);
                            let body = Value::parse_with_limit(
                                std::str::from_utf8(&request.body).unwrap(), 1048576,
                            ).unwrap();
                            assert_eq!(body["prompt"], json!({
                                "text":{"class_type":"Text","inputs":{"text":"draw"}},
                                "save":{"class_type":"Save","inputs":{"text":"output"}}
                            }));
                            json_reply(&mut socket, &json!({"prompt_id":"race"})).await;
                        },
                        "/history/race" => {
                            assert_eq!(request.method, "GET");
                            history_calls += 1;
                            if history_calls == 1 {
                                first_history = Some(socket);
                                ready.take().unwrap().send(()).expect("Original waiter must reach held history");
                            } else {
                                json_reply(&mut socket, &history).await;
                            }
                        },
                        "/view" => {
                            assert_eq!(request.method, "GET");
                            reply(&mut socket, 200, "image/png", bytes).await;
                        },
                        _ => reply(&mut socket, 404, "application/json", b"{}").await,
                    }
                },
            }
        }
    };
    let client = async {
        let original = generation.run(
            json!({"prompt":"draw","request_id":"race"}),
            "generate",
            &signal,
        );
        tokio::pin!(original);
        tokio::select! {
            ready = &mut waiting => ready.expect("Fixture ended before first-history readiness"),
            result = original.as_mut() => panic!(
                "Original waiter ended before held history: {:?}",
                result.map(|receipt| receipt.status().to_owned()),
            ),
        }
        let recovered = jobs::get(&generation, "race", true, &signal, None).await;
        // Observe the original even if refresh failed, matching source finally settlement.
        release
            .send(())
            .expect("First-history responder must still be owned");
        original.as_mut().await.unwrap();
        let recovered = recovered.unwrap();
        assert_eq!(recovered.status(), "completed");
        let store = generation.store.lock().unwrap();
        let stored = store.get("race").unwrap();
        assert_eq!(stored.status(), "completed");
        assert_eq!(stored.value["outputs"].as_array().unwrap().len(), 1);
        assert_eq!(store.list_outputs(0, 100).unwrap().len(), 1);
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        // Source does not assert saved bytes or history/view counts in this race case.
    };
    tokio::select! {
        () = client => {},
        () = server => panic!("Scoped Comfy race server ended unexpectedly"),
    }
}

#[test]
fn exact_source_shared_comfy_claim_settles_original_waiter_racing_get_job_without_replay() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let bytes = original();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), scenario(&workspace, &bytes))
            .await
            .expect("Exact source Comfy waiter race exceeded three seconds");
    });
}
