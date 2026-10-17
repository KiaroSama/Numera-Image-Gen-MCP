mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::{Config, ConfigReload},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{discovery::Discovery, generation::Generation},
};
use std::{
    collections::HashMap,
    io::{BufRead, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

fn generation(workspace: &Workspace, raw: &Value, env: HashMap<String, String>) -> Generation {
    Generation {
        config: Arc::new(
            Config::parse(
                &raw.to_string(),
                &workspace.0.join("config.json"),
                &[],
                &env,
            )
            .unwrap(),
        ),
        store: Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: env.into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
}

async fn wire(socket: &mut tokio::net::TcpStream) -> (String, Value) {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let boundary = loop {
        let count = socket.read(&mut buffer).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() <= 65536, "bounded fixture request");
        if let Some(boundary) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= boundary + 4 + length {
                break boundary;
            }
        }
    };
    (
        String::from_utf8(bytes[..boundary].to_vec()).unwrap(),
        Value::parse(std::str::from_utf8(&bytes[boundary + 4..]).unwrap()).unwrap(),
    )
}

async fn respond(socket: &mut tokio::net::TcpStream, encoded: &str) {
    let body = json!({"data":[{"b64_json":encoded}]}).to_string();
    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    socket.shutdown().await.unwrap();
}

#[tokio::test]
async fn queued_file_and_explicit_environment_snapshots_keep_old_headers_until_next_operation() {
    for kind in ["file", "env-snapshot"] {
        let workspace = Workspace::new();
        let secret = workspace.0.join("synthetic-key");
        std::fs::write(&secret, b"synthetic-a").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/v1", listener.local_addr().unwrap());
        let mut raw = workspace.config(&origin, "openai-images");
        raw["maxConcurrentRequests"] = json!(1);
        raw["connections"]["test"]["auth"] = if kind == "file" {
            json!({"type":"bearer","secretFile":secret.to_str().unwrap()})
        } else {
            json!({"type":"bearer","secretEnv":"SELECTED"})
        };
        raw["connections"]["test"]["requestTimeoutMs"] = json!(7000);
        let env = if kind == "file" {
            HashMap::new()
        } else {
            HashMap::from([("SELECTED".into(), "synthetic-a".into())])
        };
        let generation = generation(&workspace, &raw, env);
        let store = generation.store.clone();
        let blocker = Receipt::new(
            json!({"request_id":"blocker","connection":"test","status":"prepared","outputs":[],"errors":[],"warnings":[],"deviations":[]}),
        );
        let (_, mut owner) = store
            .lock()
            .unwrap()
            .prepare(blocker, "blocker", "fixture")
            .unwrap();
        assert!(
            store
                .lock()
                .unwrap()
                .admit("blocker", "test", 1, 1)
                .unwrap()
        );
        owner.value["status"] = json!("running");
        owner.value["upstream_job"] = json!({"id":"blocker","kind":"responses"});
        store.lock().unwrap().update(&mut owner).unwrap();
        store
            .lock()
            .unwrap()
            .release("blocker", Some(&owner))
            .unwrap();
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            image_fixture::original(),
        );
        let server = async {
            let mut accounts = Vec::new();
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (headers, _) = wire(&mut socket).await;
                accounts.push(
                    headers
                        .to_ascii_lowercase()
                        .contains("authorization: bearer synthetic-a"),
                );
                respond(&mut socket, &encoded).await;
            }
            accounts
        };
        let queued = async {
            generation
                .run(
                    json!({"prompt":"fixture","request_id":"queued"}),
                    "generate",
                    &CancellationToken::new(),
                )
                .await
                .unwrap()
        };
        let rotate = async {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if store
                    .lock()
                    .unwrap()
                    .get("queued")
                    .is_ok_and(|r| r.status() == "prepared")
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "actual prepared queue checkpoint"
                );
                tokio::task::yield_now().await;
            }
            let identity = store.lock().unwrap().identity("queued").unwrap();
            std::fs::write(&secret, b"synthetic-b").unwrap();
            store
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
        let client = async {
            let (first, original_identity) = tokio::join!(queued, rotate);
            let rotated = Generation {
                config: generation.config.clone(),
                store: generation.store.clone(),
                logger: generation.logger.clone(),
                runtime: generation.runtime.clone(),
                active: generation.active.clone(),
                env: (if kind == "file" {
                    HashMap::new()
                } else {
                    HashMap::from([("SELECTED".into(), "synthetic-b".into())])
                })
                .into(),
            };
            // Environment branch injects the next operation's snapshot, never mutates process env.
            let next = rotated
                .run(
                    json!({"prompt":"fixture","request_id":"next"}),
                    "generate",
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(first.status(), "completed");
            assert_eq!(next.status(), "completed");
            assert_eq!(
                store.lock().unwrap().identity("queued").unwrap(),
                original_identity
            );
            assert_ne!(
                store.lock().unwrap().identity("next").unwrap(),
                original_identity
            );
            assert!(
                !json!([first.value, next.value, generation.config.value])
                    .to_string()
                    .contains("synthetic-a")
            );
            assert!(
                !json!([first.value, next.value, generation.config.value])
                    .to_string()
                    .contains("synthetic-b")
            );
        };
        let (accounts, ()) = tokio::time::timeout(Duration::from_secs(8), async {
            tokio::join!(server, client)
        })
        .await
        .expect("owned file rotation fixture lifetime");
        assert_eq!(accounts, [true, false]);
    }
}

#[tokio::test]
async fn in_flight_generation_keeps_old_snapshot_while_reload_and_next_request_use_new_model() {
    let workspace = Workspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut first = workspace.config(&format!("{origin}/v1"), "openai-images");
    first["connections"]["test"]["defaultModel"] = json!("old");
    first["connections"]["test"]["configuredModels"] = json!([{"id":"old","name":"Old"}]);
    let old = generation(&workspace, &first, HashMap::new());
    let mut saved = first.clone();
    saved["connections"]["test"]["baseUrl"] = json!(format!("{origin}/new/v1"));
    saved["connections"]["test"]["defaultModel"] = json!("new");
    saved["connections"]["test"]["configuredModels"] = json!([{"id":"new","name":"New"}]);
    let mut reload = ConfigReload::new(
        (*old.config).clone(),
        workspace.0.join("config.json"),
        Vec::new(),
        HashMap::new(),
    );
    let submitted = tokio::sync::Notify::new();
    let release = tokio::sync::Notify::new();
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        image_fixture::original(),
    );
    let server = async {
        let mut wires = Vec::new();
        for index in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let captured = wire(&mut socket).await;
            if index == 0 {
                submitted.notify_one();
                release.notified().await;
            }
            wires.push(captured);
            respond(&mut socket, &encoded).await;
        }
        wires
    };
    let client = async {
        let signal = CancellationToken::new();
        let pending = old.run(
            json!({"prompt":"fixture","request_id":"captured-config"}),
            "generate",
            &signal,
        );
        let reload_action = async {
            submitted.notified().await;
            let accepted = reload
                .refresh_text(Some(&saved.to_string()), |_| true)
                .unwrap();
            let list = Discovery::default()
                .list(
                    &accepted.config,
                    &old.runtime,
                    &HashMap::new(),
                    None,
                    false,
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(list["models"][0]["id"], "new");
            assert_eq!(list["models"][0]["name"], "New");
            release.notify_one();
            accepted.config
        };
        let (first, current) = tokio::join!(pending, reload_action);
        let first = first.unwrap();
        assert_eq!(first.value["requested_model"], "old");
        let next = Generation {
            config: current,
            store: old.store.clone(),
            logger: old.logger.clone(),
            runtime: old.runtime.clone(),
            env: HashMap::new().into(),
            active: old.active.clone(),
        };
        assert_eq!(
            next.run(
                json!({"prompt":"fixture","request_id":"new-config"}),
                "generate",
                &CancellationToken::new()
            )
            .await
            .unwrap()
            .value["requested_model"],
            "new"
        );
    };
    let (wires, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(server, client)
    })
    .await
    .expect("owned reload fixture lifetime");
    assert!(wires[0].0.starts_with("POST /v1/images/generations "));
    assert!(wires[1].0.starts_with("POST /new/v1/images/generations "));
    assert_eq!(wires[0].1, json!({"model":"old","prompt":"fixture","n":1}));
    assert_eq!(wires[1].1, json!({"model":"new","prompt":"fixture","n":1}));
}

#[tokio::test]
async fn actual_region_edit_wire_preserves_original_and_mask_polarity_before_submission() {
    let workspace = Workspace::new();
    let original = image_fixture::original();
    let decoded = image::load_from_memory(&original).unwrap();
    let width = decoded.width();
    let height = decoded.height();
    assert!(width >= 3 && height >= 2, "real mask fixture dimensions");
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &original);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = async {
        let mut bodies = Vec::new();
        for _ in 0..3 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (headers, body) = wire(&mut socket).await;
            assert!(headers.starts_with("POST /v1/images/edits "));
            bodies.push(body);
            respond(&mut socket, &encoded).await;
        }
        bodies
    };
    let client = async {
        for (index, polarity) in ["transparent-edit", "white-edit", "black-edit"]
            .into_iter()
            .enumerate()
        {
            let mut raw = workspace.config(&origin, "openai-images");
            raw["connections"]["test"]["edit"] =
                json!({"mode":"json","encoding":"image","masks":true,"maskPolarity":polarity});
            let generation = generation(&workspace, &raw, HashMap::new());
            let input = json!({"prompt":"Preserve the comic panels 日本語","request_id":format!("region-{index}"),"target_language":"Persian",
                "reference_images":[{"type":"data_url","data_url":format!("data:image/png;base64,{encoded}")}],
                "edit_region":{"x":1,"y":0,"width":1,"height":1}});
            let receipt = generation
                .run(input, "edit", &CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(receipt.status(), "completed");
            assert_eq!(
                std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
                original
            );
        }
    };
    let (bodies, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(server, client)
    })
    .await
    .expect("owned mask-wire fixture lifetime");
    for (body, polarity) in bodies
        .iter()
        .zip(["transparent-edit", "white-edit", "black-edit"])
    {
        assert_eq!(body["image"], format!("data:image/png;base64,{encoded}"));
        let prompt = body["prompt"].as_str().unwrap();
        assert!(prompt.contains("Preserve the comic panels 日本語"));
        assert!(prompt.contains("Translate the text inside the selected region into Persian"));
        let bytes =
            numera_image_gen_mcp::files::images::data_url(body["mask"].as_str().unwrap(), 1048576)
                .unwrap();
        let mask = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!((mask.width(), mask.height()), (width, height));
        let outside = mask.get_pixel(0, 0).0;
        let inside = mask.get_pixel(1, 0).0;
        assert_eq!(outside[3], 255);
        assert_eq!(
            inside[3],
            if polarity == "transparent-edit" {
                0
            } else {
                255
            }
        );
        if polarity != "transparent-edit" {
            assert_eq!(outside[0], if polarity == "white-edit" { 0 } else { 255 });
            assert_eq!(inside[0], if polarity == "white-edit" { 255 } else { 0 });
        }
    }
}

#[test]
fn reservation_process_child() {
    let Some(root) = std::env::var_os("NUMERA_RESERVATION_FIXTURE_ROOT") else {
        return;
    };
    let id = std::env::var("NUMERA_RESERVATION_FIXTURE_ID").unwrap();
    let mut store = Store::open(Path::new(&root)).unwrap();
    let exact = std::env::var_os("NUMERA_RESERVATION_EXACT_SOURCE").is_some();
    let initial = if exact {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        Receipt::new(json!({
            "request_id":id,"connection":"local","adapter":"openai-images","operation":"generate",
            "status":"prepared","requested_model":"image-model","upstream_model":null,
            "outputs":[],"warnings":[],"deviations":[],"errors":[],
            "generation_outcome":"not_submitted","storage_outcome":"not_started",
            "another_submission_may_charge":true,"created_at":now,"updated_at":now
        }))
    } else {
        Receipt::new(
            json!({"request_id":id,"connection":"test","status":"prepared","outputs":[],"errors":[],"warnings":[],"deviations":[]}),
        )
    };
    let (_, owner) = store
        .prepare(initial, &id, if exact { "identity" } else { "fixture" })
        .unwrap();
    println!("reservation-ready");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "reserve");
    let result = store.reserve(&id, 2048, if exact { 2048 } else { 3072 });
    println!(
        "reservation-result:{}",
        match result {
            Ok(()) => "reserved".into(),
            Err(error) => error.code,
        }
    );
    std::io::stdout().flush().unwrap();
    if exact {
        drop(store);
        return;
    }
    std::io::stdin()
        .lock()
        .read_line(&mut String::new())
        .unwrap();
    store.release(&id, Some(&owner)).unwrap();
}

struct OwnedChild {
    child: Child,
    reader: Option<std::thread::JoinHandle<()>>,
    events: mpsc::Receiver<String>,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.child.stdin.take();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            reader.join().expect("owned reader cleanup");
        }
    }
}
fn child(root: &Path, id: &str, exact: bool) -> OwnedChild {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "reservation_process_child", "--nocapture"])
        .env("NUMERA_RESERVATION_FIXTURE_ROOT", root)
        .env("NUMERA_RESERVATION_FIXTURE_ID", id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    if exact {
        command.env("NUMERA_RESERVATION_EXACT_SOURCE", "1");
    } else {
        command.env_remove("NUMERA_RESERVATION_EXACT_SOURCE");
    }
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, events) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.starts_with("reservation-") && sender.send(line).is_err() {
                break;
            }
        }
    });
    OwnedChild {
        child,
        reader: Some(reader),
        events,
    }
}

#[test]
fn exact_source_two_process_reservations_close_before_reading_full_budget() {
    let workspace = Workspace::new();
    let state = workspace.0.join("state");
    let mut first = child(&state, "one", true);
    let mut second = child(&state, "two", true);
    for process in [&mut first, &mut second] {
        assert_eq!(
            process.events.recv_timeout(Duration::from_secs(3)).unwrap(),
            "reservation-ready"
        );
    }
    for process in [&mut first, &mut second] {
        let stdin = process.child.stdin.as_mut().unwrap();
        stdin.write_all(b"reserve\n").unwrap();
        stdin.flush().unwrap();
    }
    let mut results = Vec::new();
    for process in [&mut first, &mut second] {
        results.push(process.events.recv_timeout(Duration::from_secs(3)).unwrap());
    }
    for process in [&mut first, &mut second] {
        process.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = process.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "source reservation child exit deadline"
            );
            std::thread::yield_now();
        }
        process.reader.take().unwrap().join().unwrap();
    }
    results.sort();
    assert_eq!(
        results,
        [
            "reservation-result:rate_limited",
            "reservation-result:reserved"
        ]
    );
    let database = rusqlite::Connection::open_with_flags(
        state.join("receipts.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let used: i64 = database
        .query_row("SELECT SUM(reservation) FROM requests", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(used, 2048);
}

#[test]
fn two_actual_store_processes_serialize_shared_budget_reservations() {
    let workspace = Workspace::new();
    let state = workspace.0.join("state");
    drop(Store::open(&state).unwrap());
    let mut first = child(&state, "one", false);
    let mut second = child(&state, "two", false);
    for process in [&mut first, &mut second] {
        assert_eq!(
            process.events.recv_timeout(Duration::from_secs(3)).unwrap(),
            "reservation-ready"
        );
    }
    for process in [&mut first, &mut second] {
        process
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"reserve\n")
            .unwrap();
        process.child.stdin.as_mut().unwrap().flush().unwrap();
    }
    let mut results = Vec::new();
    for process in [&mut first, &mut second] {
        results.push(process.events.recv_timeout(Duration::from_secs(3)).unwrap());
    }
    results.sort();
    assert_eq!(
        results,
        [
            "reservation-result:rate_limited",
            "reservation-result:reserved"
        ]
    );
    let database = rusqlite::Connection::open(state.join("receipts.sqlite")).unwrap();
    let used: i64 = database
        .query_row("SELECT SUM(reservation) FROM requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(used, 2048);
    drop(database);
    for process in [&mut first, &mut second] {
        process
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"close\n")
            .unwrap();
        process.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = process.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "child exit deadline");
            std::thread::yield_now();
        }
    }
}
