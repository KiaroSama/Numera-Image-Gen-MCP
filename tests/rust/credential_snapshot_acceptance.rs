mod common;
#[path = "common/wire.rs"]
mod wire;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::{Config, snapshot_credentials},
    files::images::{decode_base64, inspect},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, identity::connection_identity},
};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn config(root: &Path, connection: Value) -> Config {
    Config::parse(
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
    .unwrap()
}

fn write_key(path: &Path, key: &str) {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(key.as_bytes()).unwrap();
}

fn original() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
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
    bytes
}

#[tokio::test]
async fn exact_source_file_credentials_pin_across_queue_but_refresh_on_same_generation() {
    let workspace = Workspace::new();
    let file = workspace.0.join("fixture-key");
    write_key(&file, "synthetic-a");
    let encoded = STANDARD.encode(original());
    let mut accounts = Vec::new();
    wire::serve(
        |request| {
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/v1/images/generations");
            assert_eq!(
                Value::parse_with_limit(std::str::from_utf8(&request.body).unwrap(), 1048576)
                    .unwrap(),
                json!({"model":"image","prompt":"fixture","n":1})
            );
            let authorization = request.headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("authorization")
                    .then(|| value.trim())
            });
            accounts.push(authorization == Some("Bearer synthetic-a"));
            wire::json_reply(json!({"data":[{"b64_json":encoded}]}))
        },
        |origin, requests| async move {
            let mut config = config(
                &workspace.0,
                json!({
                    "adapter":"openai-images","baseUrl":format!("{origin}/v1"),
                    "defaultModel":"image","requestTimeoutMs":5000,
                    "auth":{"type":"bearer","secretFile":file.to_str().unwrap()}
                }),
            );
            config.value["maxConcurrentRequests"] = json!(1);
            let g = Generation {
                config: Arc::new(config),
                store: Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap())),
                logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
                runtime: RuntimePaths::new(&workspace.0).unwrap(),
                env: HashMap::new().into(),
                active: Arc::new(Mutex::new(HashMap::new())),
            };
            let blocker = Receipt::new(json!({
                "request_id":"blocker","connection":"local","adapter":"openai-images",
                "operation":"generate","status":"prepared","requested_model":"image-model",
                "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
                "generation_outcome":"not_submitted","storage_outcome":"not_started",
                "another_submission_may_charge":false,
                "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
            }));
            {
                let mut store = g.store.lock().unwrap();
                let (_, mut blocker) = store.prepare(blocker, "fixture", "fixture").unwrap();
                assert!(store.admit("blocker", "local", 1, 1).unwrap());
                blocker.value["status"] = json!("running");
                blocker.value["upstream_job"] = json!({"id":"blocker","kind":"responses"});
                store.update(&mut blocker).unwrap();
                store.release("blocker", Some(&blocker)).unwrap();
            }
            let signal = CancellationToken::new();
            let pending = g.run(
                json!({"prompt":"fixture","request_id":"queued"}),
                "generate",
                &signal,
            );
            let rotate = async {
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        let prepared = g
                            .store
                            .lock()
                            .unwrap()
                            .get("queued")
                            .is_ok_and(|receipt| receipt.status() == "prepared");
                        if prepared {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("source prepared queue checkpoint exceeded two seconds");
                let identity = g.store.lock().unwrap().identity("queued").unwrap();
                write_key(&file, "synthetic-b");
                g.store
                    .lock()
                    .unwrap()
                    .terminate(
                        "blocker",
                        "cancelled",
                        &json!({"code":"fixture"}),
                        None,
                        None,
                    )
                    .unwrap();
                identity
            };
            let (first, identity) = tokio::join!(pending, rotate);
            let first = first.unwrap();
            let next = g
                .run(
                    json!({"prompt":"fixture","request_id":"next"}),
                    "generate",
                    &signal,
                )
                .await
                .unwrap();
            assert_eq!(first.status(), "completed");
            assert_eq!(next.status(), "completed");
            let store = g.store.lock().unwrap();
            assert_eq!(store.identity("queued").unwrap(), identity);
            assert_ne!(store.identity("next").unwrap(), identity);
            let persisted = json!([first.value, next.value, g.config.value]).to_string();
            assert!(!persisted.contains("synthetic-a"));
            assert!(!persisted.contains("synthetic-b"));
            assert_eq!(requests.lock().unwrap().len(), 2);
        },
    )
    .await;
    assert_eq!(accounts, [true, false]);
}

#[tokio::test]
async fn exact_source_ephemeral_snapshot_preserves_identity_and_independent_headers() {
    let workspace = Workspace::new();
    let config = config(
        &workspace.0,
        json!({
            "adapter":"openai-images","baseUrl":"https://api.example/v1",
            "auth":{"type":"bearer","apiKey":"synthetic"}
        }),
    );
    let (_, connection) = config.select(None).unwrap();
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let before = snapshot_credentials(&connection, &HashMap::new(), &runtime)
        .await
        .unwrap();
    let identity = connection_identity("local", &before).unwrap();
    let pinned = snapshot_credentials(&connection, &HashMap::new(), &runtime)
        .await
        .unwrap();
    let mut headers = pinned.headers();
    headers.insert("Authorization".into(), "modified".into());
    assert_eq!(
        pinned.headers(),
        HashMap::from([("Authorization".into(), "Bearer synthetic".into())])
    );
    assert_eq!(connection_identity("local", &pinned).unwrap(), identity);
    let keys = |value: &Value| {
        value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(pinned.value()), keys(&connection));
    assert_eq!(pinned.value().to_string(), connection.to_string());
}
