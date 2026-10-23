mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    files::{images, paths},
    http::{api, assets},
    logging::Logger,
};
use reqwest::Method;
use std::{
    collections::HashMap,
    fs, io,
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

#[test]
fn log_runs_are_distinct_redacted_at_all_levels_immutable_and_closed() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("logs with spaces");
    {
        let first = Logger::new(&directory, "DEBUG");
        let second = Logger::new(&directory, "DEBUG");
        let names: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(
            names.iter().all(|name| name.contains("_UTC-")),
            "UTC filename format"
        );
        for level in ["INFO", "WARNING", "ERROR", "DEBUG"] {
            first.log(
                level,
                "fixture",
                "Safe event.",
                json!({
                    "authorization":"Bearer synthetic-private-value",
                    "signature":"synthetic-private-value",
                    "prompt":"synthetic-private-value"
                }),
            );
        }
        first.log(
            "INFO",
            "fixture",
            "First.",
            json!({
                "run_id":"forged", "timestamp":"forged", "level":"forged",
                "component":"forged", "message":"forged"
            }),
        );
        first.log(
            "ERROR",
            "fixture",
            "Second.",
            json!({
                "api_key":"do-not-log", "private_path":"do-not-log",
                "nested":{"authorization":"do-not-log"}
            }),
        );
        second.log("INFO", "second", "Independent run.", json!({}));
        let paths: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(
            paths.len(),
            2,
            "same-second runs cannot overwrite each other"
        );
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(&directory).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("synthetic-private-value"));
        assert!(!text.contains("do-not-log"));
        let lines: Vec<Value> = text
            .lines()
            .map(|line| Value::parse(line).unwrap())
            .collect();
        assert!(
            lines
                .iter()
                .all(|line| line["timestamp"].as_str().unwrap().ends_with('Z'))
        );
        let run_id = lines[0]["run_id"].as_str().unwrap();
        assert_eq!(run_id.len(), 36);
        assert!(uuid::Uuid::parse_str(run_id).is_ok());
        assert!(
            lines
                .iter()
                .all(|line| line["run_id"].as_str() == Some(run_id))
        );
        if lines.len() == 6 {
            let levels: Vec<_> = lines[..4]
                .iter()
                .map(|line| line["level"].as_str().unwrap())
                .collect();
            assert_eq!(levels, ["INFO", "WARNING", "ERROR", "DEBUG"]);
            assert_eq!(lines[4]["level"], "INFO");
            assert_eq!(lines[4]["component"], "fixture");
            assert_eq!(lines[4]["message"], "First.");
            assert_ne!(lines[4]["timestamp"], "forged");
        } else {
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0]["component"], "second");
        }
        runs.push(run_id.to_owned());
        fs::rename(&path, path.with_extension("closed"))
            .expect("logger Drop releases file handler");
    }
    assert_eq!(runs.len(), 2);
    assert_ne!(runs[0], runs[1]);
}

#[test]
fn concurrent_output_directory_creation_and_file_collision_fail_closed() {
    let workspace = Workspace::new();
    let root = workspace.0.join("outputs");
    numera_image_gen_mcp::runtime_paths::private_directory(&root).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    std::thread::scope(|scope| {
        let a = barrier.clone();
        let a_root = &root;
        let first = scope.spawn(move || {
            a.wait();
            paths::output_directory(a_root, Some("batch/nested"))
        });
        let b = barrier.clone();
        let b_root = &root;
        let second = scope.spawn(move || {
            b.wait();
            paths::output_directory(b_root, Some("batch/nested"))
        });
        barrier.wait();
        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();
        let expected = fs::canonicalize(root.join("batch").join("nested")).unwrap();
        assert_eq!(first, expected);
        assert_eq!(second, expected);
    });
    fs::write(root.join("file"), b"fixture").unwrap();
    assert_eq!(
        paths::output_directory(&root, Some("file/nested"))
            .unwrap_err()
            .code,
        "output_file_error"
    );
    assert!(!root.join("file/nested").exists());
}

#[cfg(unix)]
#[test]
fn linked_output_directory_is_rejected_without_writing_outside() {
    let workspace = Workspace::new();
    let root = workspace.0.join("outputs");
    let outside = workspace.0.join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    assert_eq!(
        paths::output_directory(&root, Some("link/nested"))
            .unwrap_err()
            .code,
        "output_file_error"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}

async fn request_headers(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "request must finish its headers");
        bytes.extend_from_slice(&buffer[..read]);
        assert!(bytes.len() <= 65536, "bounded fixture headers");
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return bytes;
        }
    }
}

async fn hanging_response(mode: &str, asset: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let stop = CancellationToken::new();
    let rescue = stop.clone();
    let server = async {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = request_headers(&mut socket).await;
        assert!(
            !String::from_utf8(request)
                .unwrap()
                .to_ascii_lowercase()
                .contains("authorization:")
        );
        let status = if mode == "redirect" {
            "302 Found"
        } else {
            "200 OK"
        };
        let mime = if mode == "sse-error" {
            "text/event-stream"
        } else {
            "application/json"
        };
        let extra = if mode == "redirect" {
            "Location: /unapproved\r\n"
        } else if mode == "oversize" {
            "Content-Length: 9999999\r\n"
        } else {
            "Transfer-Encoding: chunked\r\n"
        };
        socket
            .write_all(
                format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\n{extra}\r\n").as_bytes(),
            )
            .await
            .unwrap();
        let body = if mode == "sse-error" {
            "event: error\ndata: {\"error\":\"fixture\"}\n\n".to_owned()
        } else if mode == "chunk-limit" {
            "x".repeat(200)
        } else {
            "x".into()
        };
        let body = if ["chunk-limit", "sse-error"].contains(&mode) {
            format!("{:x}\r\n{body}\r\n", body.len())
        } else {
            body
        };
        socket.write_all(body.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        // No body terminator: client rejection must release the actual socket promptly.
        let mut probe = [0u8; 1];
        let released = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut probe)).await;
        assert!(
            matches!(released, Ok(Ok(0)))
                || matches!(released, Ok(Err(ref e)) if matches!(e.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe)),
            "client must release the hanging response socket"
        );
        assert!(
            listener
                .poll_accept(&mut std::task::Context::from_waker(std::task::Waker::noop()))
                .is_pending(),
            "no redirect/retry request"
        );
    };
    let client = async {
        let started = Instant::now();
        let result = if asset {
            let config =
                json!({"files":{"assetAllowances":[{"origin":origin,"pathPrefix":"/image"}]}});
            assets::fetch(&format!("{origin}/image").into(), &config, 100, &stop)
                .await
                .map(|_| ())
        } else {
            let connection = json!({"baseUrl":format!("{origin}/v1"),"discoveryTimeoutMs":15000});
            api::request(
                &connection,
                &HashMap::new(),
                api::Operation {
                    operation: "probe",
                    prefix: "/v1",
                    method: Method::GET,
                    body: api::Body::Empty,
                    query: None,
                },
                &stop,
                100,
            )
            .await
            .map(|_| ())
        };
        let error = result.expect_err("hanging body must be rejected");
        assert_eq!(
            error.code,
            if mode == "redirect" {
                "unsafe_destination"
            } else if mode == "sse-error" {
                "outcome_unknown"
            } else {
                "invalid_response"
            }
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "rejection must not wait for provider deadline"
        );
        rescue.cancel();
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server, client)
    })
    .await
    .expect("owned socket/action wall bound");
}

#[tokio::test]
async fn rejected_hanging_api_bodies_release_sockets_without_redirect_or_submission_retry() {
    for mode in ["redirect", "oversize", "chunk-limit", "sse-error"] {
        hanging_response(mode, false).await;
    }
}

#[tokio::test]
async fn asset_oversize_and_unapproved_redirect_release_sockets() {
    for mode in ["oversize", "redirect"] {
        hanging_response(mode, true).await;
    }
}

#[derive(serde::Deserialize)]
struct AnimatedOracle {
    base64: String,
    pages: usize,
    error: String,
}

#[test]
fn actual_hosted_animated_webp_is_rejected_instead_of_returning_its_first_frame() {
    let file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/animated-webp.json");
    let case: AnimatedOracle = serde_json::from_slice(
        &fs::read(file).expect("hosted Sharp animated WebP oracle required"),
    )
    .expect("valid animated fixture envelope");
    assert_eq!(case.pages, 2, "real multi-frame input required");
    assert_eq!(case.error, "invalid_input");
    let bytes = images::decode_base64(&case.base64, 1048576).unwrap();
    assert!(bytes.starts_with(b"RIFF"));
    assert_eq!(bytes.get(8..12), Some(b"WEBP".as_slice()));
    assert_eq!(
        images::inspect(bytes, &json!({}), None).err().unwrap().code,
        case.error
    );
}

#[test]
fn exact_source_two_loggers_keep_four_levels_private_and_distinct_files() {
    let workspace = Workspace::new();
    {
        let first = Logger::new(&workspace.0, "DEBUG");
        let _second = Logger::new(&workspace.0, "DEBUG");
        let files: Vec<_> = fs::read_dir(&workspace.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(files.len(), 2);
        assert_ne!(files[0], files[1]);
        for level in ["INFO", "WARNING", "ERROR", "DEBUG"] {
            first.log(
                level,
                "fixture",
                "Safe event.",
                json!({"authorization":"Bearer private","signature":"private","prompt":"private"}),
            );
        }
    }
    let files: Vec<_> = fs::read_dir(&workspace.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 2);
    let contents: Vec<_> = files
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert_eq!(contents.iter().filter(|text| !text.is_empty()).count(), 1);
    let text = contents.iter().find(|text| !text.is_empty()).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|line| Value::parse(line).unwrap())
        .collect();
    let levels: Vec<_> = lines
        .iter()
        .map(|line| line["level"].as_str().unwrap())
        .collect();
    assert_eq!(levels, ["INFO", "WARNING", "ERROR", "DEBUG"]);
    assert!(!text.contains("private"));
    assert!(
        lines
            .iter()
            .all(|line| line["timestamp"].as_str().unwrap().ends_with('Z'))
    );
}

#[test]
fn exact_source_log_metadata_cannot_be_forged_and_closed_file_can_rename() {
    let workspace = Workspace::new();
    {
        let log = Logger::new(&workspace.0, "DEBUG");
        log.log(
            "INFO",
            "fixture",
            "First.",
            json!({"run_id":"forged","timestamp":"forged","level":"forged","component":"forged","message":"forged"}),
        );
        log.log(
            "ERROR",
            "fixture",
            "Second.",
            json!({"api_key":"do-not-log","private_path":"do-not-log","nested":{"authorization":"do-not-log"}}),
        );
    }
    let files: Vec<_> = fs::read_dir(&workspace.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let text = fs::read_to_string(&files[0]).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|line| Value::parse(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["level"], "INFO");
    assert_eq!(lines[0]["component"], "fixture");
    assert_eq!(lines[0]["message"], "First.");
    let run_id = lines[0]["run_id"].as_str().unwrap();
    assert_eq!(run_id.len(), 36);
    assert!(
        run_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || byte == b'-')
    );
    assert_eq!(lines[0]["run_id"], lines[1]["run_id"]);
    assert_ne!(lines[0]["timestamp"], "forged");
    assert!(!text.contains("do-not-log"));
    let renamed = std::path::PathBuf::from(format!("{}.closed", files[0].to_str().unwrap()));
    fs::rename(&files[0], &renamed).expect("logger Drop releases file handler");
    assert_eq!(fs::read_to_string(renamed).unwrap(), text);
}
