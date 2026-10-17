use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::{build_request, types::Input},
    files::images::{decode_base64, inspect},
    http::api::{self, Body},
};
use reqwest::Method;
use std::{collections::HashMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|part| part == needle)
}

fn multipart(headers: &str, body: &[u8]) -> Value {
    let content_type = headers
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("content-type:"))
        .unwrap();
    let boundary = content_type
        .split("boundary=")
        .nth(1)
        .unwrap()
        .trim_matches('"');
    let delimiter = format!("--{boundary}");
    let mut offset = delimiter.len() + 2;
    let mut parts = Vec::new();
    while offset < body.len() {
        let end = offset + find(&body[offset..], b"\r\n\r\n").unwrap();
        let header = std::str::from_utf8(&body[offset..end]).unwrap();
        let start = end + 4;
        let next = start + find(&body[start..], format!("\r\n{delimiter}").as_bytes()).unwrap();
        let bytes = &body[start..next];
        let quoted = |field: &str| {
            header
                .split(&format!("{field}=\""))
                .nth(1)
                .and_then(|s| s.split('"').next())
        };
        let name = quoted("name").unwrap();
        let part = if let Some(filename) = quoted("filename") {
            let mime = header
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Content-Type: ")
                        .or_else(|| line.strip_prefix("content-type: "))
                })
                .unwrap();
            json!({"name":name,"filename":filename,"mime":mime,"hex":bytes.iter().map(|b|format!("{b:02x}")).collect::<String>()})
        } else {
            json!({"name":name,"text":std::str::from_utf8(bytes).unwrap()})
        };
        parts.push(part);
        offset = next + 2 + delimiter.len();
        if body.get(offset..offset + 2) == Some(b"--".as_slice()) {
            break;
        }
        offset += 2;
    }
    json!(parts)
}

#[tokio::test]
async fn actual_multipart_wire_preserves_source_field_names_order_filenames_and_original_bytes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let oracle: Value =
        serde_json::from_slice(&std::fs::read(root.join(".ci-work/parity/adapters.json")).unwrap())
            .unwrap();
    let exact = Value::parse_with_limit(
        &std::fs::read_to_string(root.join(".ci-work/parity/stream-acceptance.json")).unwrap(),
        4 * 1024 * 1024,
    )
    .unwrap();
    let exact_cases = exact["multipart"].as_array().unwrap();
    assert_eq!(exact_cases.len(), 2);
    let action = async {
        let mut checked = 0;
        for case in oracle["wires"]
            .as_array()
            .unwrap()
            .iter()
            .chain(exact_cases)
        {
            let Some(expected) = case["expected"]["value"].get("multipart") else {
                continue;
            };
            assert!(expected.is_array());
            let references = case["references"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| {
                    inspect(
                        decode_base64(item["base64"].as_str().unwrap(), 1048576).unwrap(),
                        &json!({}),
                        None,
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            let mask = case["mask_base64"].as_str().map(|encoded| {
                inspect(decode_base64(encoded, 1048576).unwrap(), &json!({}), None).unwrap()
            });
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let server = async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                let boundary = loop {
                    let n = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut buffer))
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    assert!(bytes.len() <= 2097152);
                    if let Some(boundary) = find(&bytes, b"\r\n\r\n") {
                        let header = std::str::from_utf8(&bytes[..boundary]).unwrap();
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .expect("bounded multipart length");
                        if bytes.len() >= boundary + 4 + length {
                            break boundary;
                        }
                    }
                };
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                socket.shutdown().await.unwrap();
                let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
                assert!(headers.starts_with("POST /v1/images/edits "));
                multipart(headers, &bytes[boundary + 4..])
            };
            let input = Input {
                connection: &case["connection"],
                request: &case["request"],
                model: case["model"].as_js_string().unwrap(),
                operation: case["operation"].as_str().unwrap(),
                references: &references,
                mask: mask.as_ref(),
            };
            let built = build_request(&input).unwrap();
            assert!(matches!(&built.body, Body::Multipart(_)));
            let c = json!({"baseUrl":format!("{origin}/v1"),"baseUrlMode":"api-prefix","requestTimeoutMs":2000});
            let headers = HashMap::new();
            let cancel = CancellationToken::new();
            let request = api::json(
                &c,
                &headers,
                api::Operation {
                    operation: &built.operation,
                    prefix: built.prefix,
                    method: Method::POST,
                    body: built.body,
                    query: None,
                },
                &cancel,
            );
            let (response, captured) = tokio::join!(request, server);
            response.unwrap();
            assert_eq!(captured, *expected, "{}", case["name"]);
            if case["name"] == "multipart-image-array-mask" {
                let parts = captured.as_array().unwrap();
                let field = |name: &str| parts.iter().find(|part| part["name"] == name).unwrap();
                assert_eq!(field("prompt")["text"], "A café");
                assert_eq!(field("n")["text"], "1");
                assert_eq!(field("quality")["text"], "high");
                let files = parts
                    .iter()
                    .filter(|part| part["name"] == "image[]")
                    .collect::<Vec<_>>();
                assert_eq!(files.len(), 2);
                let original = decode_base64(exact["original"].as_str().unwrap(), 1048576).unwrap();
                let hex = original
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                for file in files {
                    assert_eq!(file["mime"], "image/png");
                    assert_eq!(file["hex"], hex);
                }
                assert_eq!(field("mask")["hex"], hex);
                assert!(mask.as_ref().unwrap().alpha);
            }
            checked += 1;
        }
        assert!(
            checked > 2,
            "broad and both exact source multipart cases required"
        );
    };
    tokio::time::timeout(Duration::from_secs(8), action)
        .await
        .unwrap();
}
