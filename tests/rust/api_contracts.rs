#[path = "common/http.rs"]
mod http_fixture;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::http::api::{self, Body, Operation};
use reqwest::Method;
use std::{collections::HashMap, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn bad_headers_proxies_and_missing_destinations_fail_before_network() {
    let server = Server::new(|_| Reply {
        status: 200,
        bytes: None,
        value: json!({}),
    })
    .await;
    let action = async {
        let signal = CancellationToken::new();
        let base = json!({"baseUrl":server.origin,"baseUrlMode":"api-prefix","requestTimeoutMs":1000,"discoveryTimeoutMs":1000});
        for (headers, c, code) in [
            (
                HashMap::from([("x-fixture".into(), "bad\nheader".into())]),
                base.clone(),
                "outcome_unknown",
            ),
            (
                HashMap::from([("bad header".into(), "fixture".into())]),
                base.clone(),
                "outcome_unknown",
            ),
            (
                HashMap::new(),
                json!({"baseUrl":server.origin,"proxy":{"url":"://"}}),
                "invalid_configuration",
            ),
            (HashMap::new(), json!({}), "upstream_error"),
        ] {
            let result = api::json(
                &c,
                &headers,
                Operation {
                    operation: "fixture",
                    prefix: "",
                    method: Method::GET,
                    body: Body::Empty,
                    query: None,
                },
                &signal,
            )
            .await;
            assert_eq!(result.unwrap_err().code, code);
        }
        assert!(server.requests.lock().unwrap().is_empty());
        let result = api::json(
            &base,
            &HashMap::new(),
            Operation {
                operation: "fixture",
                prefix: "",
                method: Method::GET,
                body: Body::Empty,
                query: Some(&json!({"pageToken":"value"})),
            },
            &signal,
        )
        .await
        .unwrap();
        assert_eq!(result, json!({}));
        assert_eq!(
            *server.requests.lock().unwrap(),
            vec!["/fixture?pageToken=value".to_owned()]
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(5), action).await;
    server.close().await;
    result.unwrap();
}
