mod common;
use base64::{Engine, engine::general_purpose::STANDARD};
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
    cell::Cell,
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
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
                "adapter":"openai-responses","baseUrl":format!("{origin}/v1"),
                "auth":{"type":"none"},"orchestrationModel":"text","defaultModel":"image"
            }}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let (_, connection) = config.select(None).unwrap();
    assert_eq!(connection["baseUrlMode"], "api-prefix");
    assert_eq!(connection["requestTimeoutMs"], 300000);
    assert_eq!(connection["discoveryTimeoutMs"], 15000);
    assert_eq!(connection["maxConcurrentRequests"], 2);
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

async fn request(socket: &mut TcpStream) -> (String, String) {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(
                read > 0,
                "race fixture request ended before its declared body"
            );
            bytes.extend_from_slice(&buffer[..read]);
            assert!(
                bytes.len() <= 1048576 + 65536,
                "bounded race fixture request"
            );
            let Some(boundary) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n") else {
                assert!(bytes.len() <= 65536, "bounded race fixture headers");
                continue;
            };
            assert!(boundary <= 65536);
            let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            assert!(length <= 1048576, "bounded race fixture body");
            if bytes.len() < boundary + 4 + length {
                continue;
            }
            assert_eq!(bytes.len(), boundary + 4 + length);
            let mut first = headers.lines().next().unwrap().split_whitespace();
            return (
                first.next().unwrap().to_owned(),
                first.next().unwrap().to_owned(),
            );
        }
    })
    .await
    .expect("race fixture socket read exceeded three seconds")
}

async fn reply(socket: &mut TcpStream, value: &Value) {
    let body = value.to_string();
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(body.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    })
    .await
    .expect("race fixture socket write exceeded three seconds");
}

#[test]
fn exact_source_refresher_fences_original_responses_waiter_without_duplicate_output() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(waiter_case(&workspace));
}

async fn waiter_case(workspace: &Workspace) {
    let bytes = original();
    let server_stage = Cell::new("setup");
    let client_stage = Cell::new("setup");
    let posts_seen = Cell::new(0);
    let polls_seen = Cell::new(0);
    let started = Instant::now();
    let completed = json!({"id":"owned","status":"completed","output":[{
        "type":"image_generation_call","status":"completed","result":STANDARD.encode(&bytes)
    }]});
    let scenario = async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let generation = generation(&workspace.0, &origin);
        let signal = CancellationToken::new();
        let (ready, mut waiting) = oneshot::channel::<()>();
        let (release, held) = oneshot::channel::<()>();
        // The first GET socket stays owned while the second GET is accepted and answered.
        let server = async {
            let mut posts = 0;
            let mut polls = 0;
            let (mut submission, _) = listener.accept().await.unwrap();
            let (method, path) = request(&mut submission).await;
            assert_eq!((method.as_str(), path.as_str()), ("POST", "/v1/responses"));
            posts += 1;
            posts_seen.set(posts);
            server_stage.set("post_received");
            reply(
                &mut submission,
                &json!({"id":"owned","status":"in_progress"}),
            )
            .await;
            drop(submission);
            server_stage.set("post_replied");
            let (mut first, _) = listener.accept().await.unwrap();
            let (method, path) = request(&mut first).await;
            assert_eq!(
                (method.as_str(), path.as_str()),
                ("GET", "/v1/responses/owned")
            );
            polls += 1;
            polls_seen.set(polls);
            server_stage.set("first_get_held");
            ready
                .send(())
                .expect("original waiter ended before its first GET was held");
            let (mut second, _) = listener.accept().await.unwrap();
            let (method, path) = request(&mut second).await;
            assert_eq!(
                (method.as_str(), path.as_str()),
                ("GET", "/v1/responses/owned")
            );
            polls += 1;
            polls_seen.set(polls);
            server_stage.set("second_get_received");
            reply(&mut second, &completed).await;
            drop(second);
            server_stage.set("second_get_replied");
            held.await
                .expect("refresher must release the held original waiter");
            server_stage.set("held_released");
            reply(&mut first, &completed).await;
            server_stage.set("first_get_replied");
            (posts, polls)
        };
        let client = async {
            let original = generation.run(
                json!({"prompt":"draw","request_id":"responses-race"}),
                "generate",
                &signal,
            );
            tokio::pin!(original);
            client_stage.set("waiting_ready");
            tokio::select! {
                ready = &mut waiting => ready.expect("fixture ended before its held first GET"),
                outcome = original.as_mut() => {
                    panic!("original generation ended before held-poll readiness: {:?}", outcome.map(|receipt| receipt.status().to_owned()));
                }
            }
            client_stage.set("refresher_waiting");
            let recovered = jobs::get(&generation, "responses-race", true, &signal, None).await;
            client_stage.set("refresher_returned");
            // Settle the original even when recovery returns an error, before asserting it.
            release
                .send(())
                .expect("held original socket must still be owned");
            client_stage.set("release_sent");
            original.as_mut().await.unwrap();
            client_stage.set("original_settled");
            let recovered = recovered.unwrap();
            assert_eq!(recovered.status(), "completed");
            let stored = generation
                .store
                .lock()
                .unwrap()
                .get("responses-race")
                .unwrap();
            assert_eq!(stored.value["outputs"].as_array().unwrap().len(), 1);
            assert_eq!(
                generation
                    .store
                    .lock()
                    .unwrap()
                    .list_outputs(0, 25)
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                std::fs::read(stored.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
                bytes
            );
        };
        let ((posts, polls), ()) = tokio::join!(server, client);
        assert_eq!(posts, 1);
        assert_eq!(polls, 2);
    };
    // Keep the source's three-second scenario bound, not the provider's default timeout.
    let result = tokio::time::timeout(Duration::from_secs(3), scenario).await;
    if result.is_err() {
        eprintln!(
            "Responses waiter timeout: server_stage={} client_stage={} posts={} polls={} elapsed_ms={}",
            server_stage.get(),
            client_stage.get(),
            posts_seen.get(),
            polls_seen.get(),
            started.elapsed().as_millis()
        );
    }
    result.expect("exact source Responses waiter scenario exceeded three seconds");
}
