#[path = "common/wire.rs"]
mod wire;
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::http::{api, assets::private_address, sse::events, url::operation_url};
use reqwest::Method;
use std::{collections::HashMap, sync::Mutex};
use tokio_util::sync::CancellationToken;

#[test]
fn transport_observer_runs_once_before_send_and_not_for_invalid_configuration() {
    let observations = Mutex::new(Vec::new());
    let seen = &observations;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(wire::serve(
            |request| {
                assert_eq!(observations.lock().unwrap().len(), 1);
                assert_eq!(request.method, "GET");
                assert_eq!(request.path, "/v1/probe");
                assert!(
                    !request
                        .headers
                        .to_ascii_lowercase()
                        .contains("authorization:")
                );
                assert!(request.body.is_empty());
                wire::json_reply(json!({"ok":true}))
            },
            |origin, requests| async move {
                let signal = CancellationToken::new();
                let invalid = api::request_observed(
                    &json!({"baseUrl":"https://user:private@example.invalid/v1"}),
                    &HashMap::new(),
                    api::Operation {
                        operation: "probe",
                        prefix: "/v1",
                        method: Method::GET,
                        body: api::Body::Empty,
                        query: None,
                    },
                    &signal,
                    4096,
                    |_, _, _| panic!("Invalid destination must not prepare transport"),
                )
                .await;
                assert_eq!(invalid.err().unwrap().code, "invalid_configuration");
                assert!(seen.lock().unwrap().is_empty());
                let result = api::request_observed(
                    &json!({"baseUrl":format!("{origin}/v1"),"discoveryTimeoutMs":2000}),
                    &HashMap::new(),
                    api::Operation {
                        operation: "probe",
                        prefix: "/v1",
                        method: Method::GET,
                        body: api::Body::Empty,
                        query: None,
                    },
                    &signal,
                    4096,
                    |build, prepare, timeout| {
                        assert!(prepare >= build);
                        assert_eq!(timeout, 2000);
                        seen.lock().unwrap().push((build, prepare, timeout));
                    },
                )
                .await
                .unwrap();
                assert_eq!(result.bytes, b"{\"ok\":true}");
                assert_eq!(seen.lock().unwrap().len(), 1);
                assert_eq!(requests.lock().unwrap().len(), 1);
            },
        ));
}

#[test]
fn operation_prefix_and_sensitive_query_are_preserved() {
    let url = operation_url(
        "https://example.org/v1",
        "api-prefix",
        "images/generations",
        "/v1",
        &json!({}),
    )
    .expect("valid prefix");
    assert_eq!(url.as_str(), "https://example.org/v1/images/generations");
    assert!(
        operation_url(
            "https://example.org/v1",
            "api-prefix",
            "images/generations",
            "/v1",
            &json!({"api_key":"secret"})
        )
        .is_err()
    );
    assert!(
        operation_url(
            "https://example.org/v1",
            "origin",
            "models",
            "/v1",
            &json!({})
        )
        .is_err()
    );
}

#[test]
fn exact_source_operation_prefix_base_and_relative_path_inputs_preserve_the_trusted_destination_contract()
 {
    for (base, mode, expected) in [
        (
            "http://127.0.0.1:20100/v1",
            "api-prefix",
            "http://127.0.0.1:20100/v1/images/generations",
        ),
        (
            "http://127.0.0.1:20100",
            "origin",
            "http://127.0.0.1:20100/v1/images/generations",
        ),
        (
            "https://gateway.example/api/v1/",
            "api-prefix",
            "https://gateway.example/api/v1/images/generations",
        ),
        (
            "https://gateway.example/company/v1",
            "api-prefix",
            "https://gateway.example/company/v1/images/generations",
        ),
    ] {
        assert_eq!(
            operation_url(base, mode, "images/generations", "/v1", &json!({}))
                .unwrap()
                .as_str(),
            expected
        );
    }
    for base in [
        "https://key:secret@example.com/v1",
        "https://example.com/v1#x",
        "https://example.com/v1/images/generations",
        "https://example.com/v1?api_key=x",
    ] {
        assert!(
            operation_url(base, "api-prefix", "images/generations", "/v1", &json!({})).is_err()
        );
    }
    for path in ["../secret", "%2e%2e/secret", "//other.example/path", "a\\b"] {
        assert!(
            operation_url(
                "https://example.com/v1",
                "api-prefix",
                path,
                "/v1",
                &json!({})
            )
            .is_err()
        );
    }
}

#[test]
fn exact_source_private_and_public_addresses_are_classified_without_network() {
    assert!(private_address("192.168.1.1"));
    for address in ["1.1.1.1", "2606:4700:4700::1111"] {
        assert!(!private_address(address), "{address}");
    }
}

#[test]
fn private_asset_destinations_remain_blocked() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "::1",
        "::ffff:127.0.0.1",
        "::ffff:7f00:1",
        "2001:db8::1",
        "0.0.0.0",
        "100.64.0.1",
        "100.127.255.255",
        "172.16.0.1",
        "172.31.255.255",
        "192.0.0.1",
        "192.168.0.1",
        "198.18.0.1",
        "198.19.255.255",
        "224.0.0.1",
        "255.255.255.255",
        "::",
        "fc00::1",
        "fd00::1",
        "fe80::1",
        "febf::1",
        "ff02::1",
        "not-an-ip",
    ] {
        assert!(private_address(address), "{address}");
    }
    for address in [
        "8.8.8.8",
        "100.63.255.255",
        "100.128.0.1",
        "172.15.255.255",
        "172.32.0.1",
        "192.0.1.1",
        "198.17.255.255",
        "198.20.0.1",
        "2001:4860:4860::8888",
        "::ffff:8.8.8.8",
    ] {
        assert!(!private_address(address), "{address}");
    }
}

#[test]
fn terminal_sse_is_not_discarded_as_generic_error() {
    let value=events(b"event: response.failed\ndata: {\"response\":{\"id\":\"same-job\",\"status\":\"failed\"}}\n\n",1024).expect("terminal frame");
    assert_eq!(value[0].kind, "response.failed");
    assert!(events(b"data: {}", 1024).is_err());
    let mut reader = numera_image_gen_mcp::http::sse::Reader::new(1024);
    assert!(reader.push(b"event: error\ndata: {}\n\n").is_err());
    assert_eq!(
        events(b"event: error\ndata: {}\n\n", 1024).unwrap()[0].kind,
        "error"
    );
    for chunks in [1, 2, 7, 64] {
        let mut reader = numera_image_gen_mcp::http::sse::Reader::new(4096);
        let frames = "event: progress\r\ndata: {\"text\":\"日本語 🌍\"}\r\n\r\nevent: response.completed\ndata: {\"response\":{\"id\":\"same\",\"status\":\"completed\"}}\n\n";
        for chunk in frames.as_bytes().chunks(chunks) {
            reader.push(chunk).unwrap();
        }
        let retained = reader.finish().unwrap();
        let parsed = events(&retained, 4096).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].kind, "response.completed");
    }
}
