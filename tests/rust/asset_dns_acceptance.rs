mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    files::images::{decode_base64, inspect},
    http::assets,
};
use std::{
    collections::HashMap,
    future::{Future, pending},
    net::{IpAddr, Ipv4Addr},
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

fn config(root: &Path) -> Config {
    Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":{"adapter":"openai-images",
                "baseUrl":"https://api.example/v1","auth":{"type":"none"}}}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
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

async fn headers(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = socket.read(&mut buffer).await.unwrap();
        assert!(count > 0, "Fixture request ended before headers");
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() <= 65536, "Bounded fixture request headers");
        if bytes.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
            let text = String::from_utf8(bytes).unwrap();
            assert!(!text.to_ascii_lowercase().contains("authorization:"));
            return text;
        }
    }
}

fn no_more_requests(listener: &TcpListener) {
    assert!(
        listener
            .poll_accept(&mut std::task::Context::from_waker(std::task::Waker::noop()))
            .is_pending(),
        "Rejected destination must not open a socket"
    );
}

fn run<F: Future<Output = ()>>(action: F) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(5), action)
                .await
                .expect("DNS acceptance exceeded five seconds");
        });
}

#[test]
fn source_hostname_pin_preserves_original_and_refuses_unallowed_path_without_credentials() {
    let workspace = Workspace::new();
    run(async {
        let bytes = original();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let origin = format!("http://fixture.invalid:{port}");
        let mut config = config(&workspace.0).into_value();
        config["files"]["assetAllowances"] = json!([{"origin":origin,"pathPrefix":"/image"}]);
        let resolves = AtomicUsize::new(0);
        let resolve = |host: String, selected_port| {
            assert_eq!(host, "fixture.invalid");
            assert_eq!(selected_port, port);
            resolves.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]))
        };
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                let text = headers(&mut socket).await;
                assert_eq!(text.lines().next(), Some("GET /image HTTP/1.1"));
                assert!(
                    text.to_ascii_lowercase()
                        .contains(&format!("host: fixture.invalid:{port}\r\n"))
                );
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                );
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.write_all(&bytes).await.unwrap();
                socket.shutdown().await.unwrap();
            })
            .await
            .expect("Fixture socket exceeded three seconds");
        };
        let client = async {
            let signal = CancellationToken::new();
            let url = format!("{origin}/image").into();
            assert_eq!(
                assets::fetch_with_resolver(&url, &config, 4096, &signal, &resolve)
                    .await
                    .unwrap(),
                bytes
            );
            let forbidden = format!("{origin}/not-allowed").into();
            let error = assets::fetch_with_resolver(&forbidden, &config, 4096, &signal, &resolve)
                .await
                .unwrap_err();
            assert_eq!(error.code, "unsafe_destination");
        };
        tokio::join!(server, client);
        assert_eq!(resolves.load(Ordering::SeqCst), 2);
        no_more_requests(&listener);
    });
}

#[test]
fn resolver_empty_private_and_error_results_fail_before_any_socket() {
    let workspace = Workspace::new();
    run(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://fixture.invalid:{port}/image").into();
        let config = config(&workspace.0).into_value();
        let signal = CancellationToken::new();
        for addresses in [
            vec![],
            vec![
                IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
            ],
            vec![
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            ],
        ] {
            let error = assets::fetch_with_resolver(&url, &config, 4096, &signal, |_, _| {
                std::future::ready(Ok(addresses.clone()))
            })
            .await
            .unwrap_err();
            assert_eq!(error.code, "unsafe_destination");
        }
        let error = assets::fetch_with_resolver(&url, &config, 4096, &signal, |_, _| {
            std::future::ready(Err(std::io::Error::other("Controlled resolver failure")))
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, "input_file_error");
        assert!(!error.message.contains("Controlled resolver failure"));
        no_more_requests(&listener);
    });
}

#[test]
fn redirect_rechecks_hostname_resolution_and_path_allowance_before_connecting() {
    let workspace = Workspace::new();
    run(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let origin = format!("http://fixture.invalid:{port}");
        let mut config = config(&workspace.0).into_value();
        config["files"]["assetAllowances"] = json!([{"origin":origin,"pathPrefix":"/image"}]);
        let resolves = AtomicUsize::new(0);
        let signal = CancellationToken::new();
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                let text = headers(&mut socket).await;
                assert_eq!(text.lines().next(), Some("GET /image HTTP/1.1"));
                socket
                    .write_all(b"HTTP/1.1 302 Found\r\nLocation: /not-allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
                socket.shutdown().await.unwrap();
            })
            .await
            .expect("Fixture redirect socket exceeded three seconds");
        };
        let client = async {
            let url = format!("{origin}/image").into();
            let error =
                assets::fetch_with_resolver(&url, &config, 4096, &signal, |host, selected_port| {
                    assert_eq!(host, "fixture.invalid");
                    assert_eq!(selected_port, port);
                    resolves.fetch_add(1, Ordering::SeqCst);
                    std::future::ready(Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]))
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, "unsafe_destination");
        };
        tokio::join!(server, client);
        assert_eq!(resolves.load(Ordering::SeqCst), 2);
        no_more_requests(&listener);
    });
}

#[test]
fn cancellation_stops_pending_resolution_without_opening_a_socket() {
    let workspace = Workspace::new();
    run(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://fixture.invalid:{port}/image").into();
        let config = config(&workspace.0).into_value();
        let signal = CancellationToken::new();
        let (ready, seen) = tokio::sync::oneshot::channel();
        let ready = std::sync::Mutex::new(Some(ready));
        let fetching = assets::fetch_with_resolver(&url, &config, 4096, &signal, |_, _| {
            ready.lock().unwrap().take().unwrap().send(()).unwrap();
            pending::<std::io::Result<Vec<IpAddr>>>()
        });
        let cancelling = async {
            seen.await.unwrap();
            signal.cancel();
        };
        let (result, ()) = tokio::join!(fetching, cancelling);
        assert_eq!(result.unwrap_err().code, "cancelled");
        no_more_requests(&listener);
    });
}
