mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    files::images::{decode_base64, inspect},
    http::{api, assets},
};
use reqwest::Method;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

struct Reply {
    status: u16,
    mime: Option<&'static str>,
    location: Option<&'static str>,
    body: Vec<u8>,
}

fn json_reply(status: u16, value: Value) -> Reply {
    Reply {
        status,
        mime: Some("application/json"),
        location: None,
        body: value.to_string().into_bytes(),
    }
}

async fn with_server<F, A>(reply: impl Fn(&str) -> Reply, action: F)
where
    F: FnOnce(String, Arc<Mutex<Vec<String>>>) -> A,
    A: Future<Output = ()>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    // Scoped futures own every socket; return, panic and timeout all drop the listener.
    let server = async {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                    assert!(bytes.len() <= 65536, "fixture header limit");
                    if bytes.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = std::str::from_utf8(&bytes).unwrap();
                let first = text.lines().next().unwrap();
                let mut parts = first.split_whitespace();
                assert_eq!(parts.next(), Some("GET"));
                let path = parts.next().unwrap();
                captured.lock().unwrap().push(path.to_owned());
                let response = reply(path);
                let location = response
                    .location
                    .map(|value| format!("Location: {value}\r\n"))
                    .unwrap_or_default();
                let mime = response
                    .mime
                    .map(|value| format!("Content-Type: {value}\r\n"))
                    .unwrap_or_default();
                let headers = format!(
                    "HTTP/1.1 {} Fixture\r\n{mime}Content-Length: {}\r\n{location}Connection: close\r\n\r\n",
                    response.status,
                    response.body.len()
                );
                if socket.write_all(headers.as_bytes()).await.is_ok()
                    && socket.write_all(&response.body).await.is_ok()
                {
                    let _ = socket.shutdown().await;
                }
            })
            .await
            .expect("fixture socket exceeded three seconds");
        }
    };
    tokio::time::timeout(Duration::from_secs(8), async {
        tokio::select! {
            () = action(origin, requests) => {},
            () = server => panic!("fixture listener ended unexpectedly"),
        }
    })
    .await
    .expect("source network acceptance exceeded eight seconds");
}

fn configuration(workspace: &Workspace, origin: &str) -> Config {
    Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":workspace.0.join("outputs").to_str().unwrap(),
            "stateDir":workspace.0.join("state").to_str().unwrap(),
            "logging":{"directory":workspace.0.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[workspace.0.join("inputs").to_str().unwrap()]},
            "connections":{"local":{
                "adapter":"openai-images","baseUrl":format!("{origin}/v1"),
                "auth":{"type":"none"},"discoveryTimeoutMs":2000
            }}
        })
        .to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

fn get(operation: &str) -> api::Operation<'_> {
    api::Operation {
        operation,
        prefix: "/v1",
        method: Method::GET,
        body: api::Body::Empty,
        query: None,
    }
}

async fn status_case(status: u16, expected: &str) {
    let workspace = Workspace::new();
    with_server(
        |_| json_reply(status, json!({"error":"private body never emitted"})),
        |origin, requests| async move {
            let config = configuration(&workspace, &origin);
            let (_, connection) = config.select(None).unwrap();
            let error = api::request(
                &connection,
                &HashMap::new(),
                get("models"),
                &CancellationToken::new(),
                100 * 1024 * 1024,
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.code, expected);
            assert_eq!(error.http_status, Some(status));
            assert_eq!(requests.lock().unwrap().as_slice(), ["/v1/models"]);
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_http_401_is_authentication_error_without_retry() {
    status_case(401, "authentication_error").await;
}

#[tokio::test]
async fn exact_source_http_403_is_permission_denied_without_retry() {
    status_case(403, "permission_denied").await;
}

#[tokio::test]
async fn exact_source_http_404_is_model_unavailable_without_retry() {
    status_case(404, "model_unavailable").await;
}

#[tokio::test]
async fn exact_source_http_429_is_rate_limited_without_retry() {
    status_case(429, "rate_limited").await;
}

#[tokio::test]
async fn exact_source_http_500_is_upstream_error_without_retry() {
    status_case(500, "upstream_error").await;
}

#[tokio::test]
async fn exact_source_redirect_malformed_five_byte_limit_and_preabort_reject_in_order() {
    let workspace = Workspace::new();
    with_server(
        |path| {
            if path.ends_with("/redirect") {
                Reply {
                    status: 302,
                    mime: None,
                    location: Some("/v1/final"),
                    body: Vec::new(),
                }
            } else {
                Reply {
                    status: 200,
                    mime: if path.ends_with("/malformed") {
                        Some("application/json")
                    } else {
                        Some("application/octet-stream")
                    },
                    location: None,
                    body: if path.ends_with("/malformed") {
                        b"{".to_vec()
                    } else {
                        vec![0; 5]
                    },
                }
            }
        },
        |origin, _| async move {
            let config = configuration(&workspace, &origin);
            let (_, connection) = config.select(None).unwrap();
            let signal = CancellationToken::new();
            let error = api::request(
                &connection,
                &HashMap::new(),
                get("redirect"),
                &signal,
                100 * 1024 * 1024,
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.code, "unsafe_destination");
            let error = api::json(&connection, &HashMap::new(), get("malformed"), &signal)
                .await
                .unwrap_err();
            assert_eq!(error.code, "invalid_response");
            let error = api::request(&connection, &HashMap::new(), get("bytes"), &signal, 4)
                .await
                .err()
                .unwrap();
            assert_eq!(error.code, "invalid_response");
            let cancelled = CancellationToken::new();
            cancelled.cancel();
            let error = api::request(
                &connection,
                &HashMap::new(),
                get("bytes"),
                &cancelled,
                100 * 1024 * 1024,
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.code, "cancelled");
        },
    )
    .await;
}

fn original() -> Vec<u8> {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".ci-work/parity/image-acceptance.json");
    let cases = Value::parse_with_limit(&std::fs::read_to_string(file).unwrap(), 1048576).unwrap();
    let row = cases
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
async fn exact_source_asset_prefix_redirect_loop_limits_and_unsafe_urls_preserve_original_bytes() {
    let workspace = Workspace::new();
    let bytes = original();
    let served = bytes.clone();
    with_server(
        |path| {
            let location = if path.ends_with("/redirect") {
                Some("/private/image.png")
            } else if path.ends_with("/loop") {
                Some("/assets/loop")
            } else {
                None
            };
            if path.ends_with("/missing") {
                json_reply(404, json!({}))
            } else {
                Reply {
                    status: if location.is_some() { 302 } else { 200 },
                    mime: if location.is_some() {
                        None
                    } else {
                        Some("image/png")
                    },
                    location,
                    body: if location.is_some() {
                        Vec::new()
                    } else {
                        served.clone()
                    },
                }
            }
        },
        |origin, paths| async move {
            let mut config = configuration(&workspace, &origin);
            let signal = CancellationToken::new();
            let url = format!("{origin}/assets/image.png");
            let error = assets::fetch(&url.clone().into(), &config.value, 1000, &signal)
                .await
                .unwrap_err();
            assert_eq!(error.code, "unsafe_destination");
            config.value["files"]["assetAllowances"] =
                json!([{"origin":origin,"pathPrefix":"/assets"}]);
            let actual = assets::fetch(&url.clone().into(), &config.value, bytes.len(), &signal)
                .await
                .unwrap();
            assert_eq!(actual, bytes);
            for suffix in ["assets-other/image.png", "assets/redirect"] {
                let error = assets::fetch(
                    &format!("{origin}/{suffix}").into(),
                    &config.value,
                    1000,
                    &signal,
                )
                .await
                .unwrap_err();
                assert_eq!(error.code, "unsafe_destination");
            }
            assert!(
                !paths
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|path| path == "/private/image.png")
            );
            for (suffix, limit, code) in [
                ("missing", 1000, "input_file_error"),
                ("loop", 1000, "unsafe_destination"),
                ("image.png", bytes.len() - 1, "invalid_response"),
            ] {
                let error = assets::fetch(
                    &format!("{origin}/assets/{suffix}").into(),
                    &config.value,
                    limit,
                    &signal,
                )
                .await
                .unwrap_err();
                assert_eq!(error.code, code);
            }
            for unsafe_url in [
                "not a URL".to_owned(),
                "file:///image.png".to_owned(),
                format!("{url}#fragment"),
                origin.replace("http://", "http://user:password@") + "/assets/image.png",
            ] {
                let error = assets::fetch(&unsafe_url.into(), &config.value, 1000, &signal)
                    .await
                    .unwrap_err();
                assert_eq!(error.code, "unsafe_destination");
            }
        },
    )
    .await;
}
