mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::json,
    config::Config,
    http::{api, assets},
};
use reqwest::Method;
use std::{collections::HashMap, io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

fn config(workspace: &Workspace, origin: &str) -> Config {
    Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":workspace.0.join("outputs").to_str().unwrap(),
            "stateDir":workspace.0.join("state").to_str().unwrap(),
            "logging":{"directory":workspace.0.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[workspace.0.join("inputs").to_str().unwrap()]},
            "connections":{"local":{
                "adapter":"openai-images","baseUrl":format!("{origin}/v1"),
                "auth":{"type":"none"},"discoveryTimeoutMs":15000
            }}
        })
        .to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

async fn headers(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "fixture request ended before headers");
        bytes.extend_from_slice(&buffer[..read]);
        assert!(bytes.len() <= 65536, "bounded fixture request headers");
        if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            let text = String::from_utf8(bytes).unwrap();
            assert!(!text.to_ascii_lowercase().contains("authorization:"));
            return text;
        }
    }
}

async fn released(socket: &mut TcpStream) {
    let mut byte = [0u8; 1];
    let result = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte)).await;
    assert!(
        matches!(result, Ok(Ok(0)))
            || matches!(result, Ok(Err(ref error)) if matches!(error.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe)),
        "client must release the held response without its provider deadline"
    );
}

fn no_more_requests(listener: &TcpListener) {
    assert!(
        listener
            .poll_accept(&mut std::task::Context::from_waker(std::task::Waker::noop()))
            .is_pending(),
        "no redirect or retry request"
    );
}

async fn api_case(mode: &str) {
    let workspace = Workspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let config = config(&workspace, &origin);
    let (_, connection) = config.select(None).unwrap();
    assert_eq!(connection["discoveryTimeoutMs"], 15000);
    let signal = CancellationToken::new();
    let server = async {
        let (mut socket, _) = listener.accept().await.unwrap();
        let text = headers(&mut socket).await;
        assert_eq!(text.lines().next(), Some("GET /v1/probe HTTP/1.1"));
        let status = if mode == "redirect" { 302 } else { 200 };
        let mime = if mode == "sse-error" {
            "text/event-stream"
        } else {
            "application/json"
        };
        let extra = match mode {
            "redirect" => "Location: /elsewhere\r\nTransfer-Encoding: chunked\r\n",
            "oversize" => "Content-Length: 9999999\r\n",
            _ => "Transfer-Encoding: chunked\r\n",
        };
        let body = if mode == "sse-error" {
            b"event: error\ndata: {\"error\":\"fixture\"}\n\n".to_vec()
        } else if mode == "chunk-limit" {
            vec![b'x'; 200]
        } else {
            vec![b'x']
        };
        let head = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {mime}\r\n{extra}\r\n");
        let mut response = head.into_bytes();
        if mode == "oversize" {
            response.extend_from_slice(&body);
        } else {
            response.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
            response.extend_from_slice(&body);
            response.extend_from_slice(b"\r\n");
        }
        // No terminating chunk/EOF: an Err alone cannot establish socket release.
        let write = socket.write_all(&response).await;
        assert!(
            write.is_ok()
                || write.is_err_and(|error| {
                    matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe
                    )
                })
        );
        released(&mut socket).await;
        no_more_requests(&listener);
    };
    let client = async {
        let result = tokio::time::timeout(
            Duration::from_secs(2),
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
                &signal,
                100,
            ),
        )
        .await
        .expect("source rescue ceiling reached; provider wait did not reject promptly");
        let error = result.err().unwrap();
        assert_eq!(
            error.code,
            match mode {
                "redirect" => "unsafe_destination",
                "sse-error" => "outcome_unknown",
                _ => "invalid_response",
            }
        );
        assert!(!signal.is_cancelled());
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server, client)
    })
    .await
    .expect("owned hanging API fixture exceeded five seconds");
}

#[tokio::test]
async fn exact_source_redirect_releases_held_api_socket_before_provider_deadline() {
    api_case("redirect").await;
}

#[tokio::test]
async fn exact_source_oversize_releases_held_api_socket_before_provider_deadline() {
    api_case("oversize").await;
}

#[tokio::test]
async fn exact_source_chunk_limit_releases_held_api_socket_before_provider_deadline() {
    api_case("chunk-limit").await;
}

#[tokio::test]
async fn exact_source_sse_error_releases_held_api_socket_before_provider_deadline() {
    api_case("sse-error").await;
}

#[tokio::test]
async fn exact_source_oversize_asset_releases_socket_and_refuses_unapproved_redirect() {
    let workspace = Workspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut config = config(&workspace, &origin);
    config.value["files"]["assetAllowances"] = json!([
        {"origin":origin,"pathPrefix":"/image"},
        {"origin":origin,"pathPrefix":"/redirect"}
    ]);
    let server = async {
        for expected in ["GET /image HTTP/1.1", "GET /redirect HTTP/1.1"] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let text = headers(&mut socket).await;
            assert_eq!(text.lines().next(), Some(expected));
            let response: &[u8] = if expected.contains("/image ") {
                b"HTTP/1.1 200 OK\r\nContent-Length: 9999999\r\n\r\nx"
            } else {
                b"HTTP/1.1 302 Found\r\nLocation: /private\r\nContent-Length: 0\r\n\r\n"
            };
            let write = socket.write_all(response).await;
            assert!(
                write.is_ok()
                    || write.is_err_and(|error| {
                        matches!(
                            error.kind(),
                            io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe
                        )
                    })
            );
            if expected.contains("/image ") {
                released(&mut socket).await;
            } else {
                socket.shutdown().await.unwrap();
            }
        }
        no_more_requests(&listener);
    };
    let client = async {
        let signal = CancellationToken::new();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            assets::fetch(
                &format!("{origin}/image").into(),
                &config.value,
                100,
                &signal,
            ),
        )
        .await
        .expect("source rescue ceiling reached for oversized asset");
        assert_eq!(result.unwrap_err().code, "invalid_response");
        assert!(!signal.is_cancelled());
        let error = assets::fetch(
            &format!("{origin}/redirect").into(),
            &config.value,
            100,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "unsafe_destination");
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server, client)
    })
    .await
    .expect("owned hanging asset fixture exceeded five seconds");
}
