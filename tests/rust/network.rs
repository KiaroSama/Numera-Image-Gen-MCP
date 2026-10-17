#[path = "common/http.rs"]
mod http_fixture;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::http::{
    api::{self, Body},
    assets,
};
use reqwest::Method;
use std::{collections::HashMap, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn api_status_errors_redirects_limits_and_cancel_never_retry() {
    let server = Server::new(|path| Reply {
        bytes: None,
        status: path.trim_start_matches('/').parse().unwrap_or(200),
        value: json!({"data":"bounded fixture response"}),
    })
    .await;
    let cancel = CancellationToken::new();
    let connection = json!({"baseUrl":server.origin,"baseUrlMode":"api-prefix","requestTimeoutMs":2000,"discoveryTimeoutMs":2000});
    let action = async {
        for (status, code) in [
            (401, "authentication_error"),
            (403, "permission_denied"),
            (404, "model_unavailable"),
            (429, "rate_limited"),
            (500, "upstream_error"),
            (302, "unsafe_destination"),
        ] {
            let before = server.requests.lock().unwrap().len();
            let error = api::json(
                &connection,
                &HashMap::new(),
                api::Operation {
                    operation: &status.to_string(),
                    prefix: "",
                    method: Method::GET,
                    body: Body::Empty,
                    query: None,
                },
                &cancel,
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(server.requests.lock().unwrap().len(), before + 1);
        }
        let response = api::json(
            &connection,
            &HashMap::new(),
            api::Operation {
                operation: "200",
                prefix: "",
                method: Method::GET,
                body: Body::Empty,
                query: None,
            },
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(response["data"], "bounded fixture response");
        let error = api::request(
            &connection,
            &HashMap::new(),
            api::Operation {
                operation: "200",
                prefix: "",
                method: Method::GET,
                body: Body::Empty,
                query: None,
            },
            &cancel,
            1,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.code, "invalid_response");
        cancel.cancel();
        let error = api::json(
            &connection,
            &HashMap::new(),
            api::Operation {
                operation: "200",
                prefix: "",
                method: Method::GET,
                body: Body::Empty,
                query: None,
            },
            &cancel,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "cancelled");
    };
    let result = tokio::time::timeout(Duration::from_secs(6), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn api_and_asset_absent_bodies_reject_but_empty_present_body_is_allowed() {
    let server = Server::new(|path| Reply {
        bytes: Some(Vec::new()),
        status: path.trim_start_matches('/').parse().unwrap_or(200),
        value: json!({}),
    })
    .await;
    let connection =
        json!({"baseUrl":server.origin,"baseUrlMode":"api-prefix","discoveryTimeoutMs":2000});
    let config = json!({"files":{"assetAllowances":[{"origin":server.origin,"pathPrefix":"/"}]}});
    let cancel = CancellationToken::new();
    let result = tokio::time::timeout(Duration::from_secs(6), async {
        let mut results = Vec::new();
        for status in [204, 205, 200] {
            let path = status.to_string();
            let api = api::request(
                &connection,
                &HashMap::new(),
                api::Operation {
                    operation: &path,
                    prefix: "",
                    method: Method::GET,
                    body: Body::Empty,
                    query: None,
                },
                &cancel,
                1024,
            )
            .await
            .map(|response| response.bytes);
            let asset = assets::fetch(
                &format!("{}/{path}", server.origin).into(),
                &config,
                1024,
                &cancel,
            )
            .await;
            results.push((status, api, asset));
        }
        results
    })
    .await;
    server.close().await;
    for (status, api, asset) in result.unwrap() {
        for result in [api, asset] {
            if status == 200 {
                assert!(result.unwrap().is_empty());
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.code, "invalid_response");
                assert_eq!(error.message, "Response body is empty.");
                assert_eq!(error.stage, "response");
            }
        }
    }
}

#[tokio::test]
async fn credential_free_assets_require_exact_allowance_and_revalidate_redirects() {
    let server = Server::new(|path| Reply {
        bytes: None,
        status: if path.ends_with("missing") {
            404
        } else if path.ends_with("redirect") {
            302
        } else {
            200
        },
        value: json!({"image":"fixture"}),
    })
    .await;
    let action = async {
        let blocked = json!({"files":{"assetAllowances":[]}});
        assert!(server.requests.lock().unwrap().is_empty());
        let cancel = CancellationToken::new();
        for url in [
            format!("{}/image", server.origin),
            "file:///unowned/image".into(),
            "http://user:password@127.0.0.1/image".into(),
            "not a url".into(),
            "http://127.0.0.1/image#fragment".into(),
            "http://127.0.0.1/image?token=fixture".into(),
            "http://127.0.0.1/%2e%2e/private".into(),
            "http://127.0.0.1/%2fprivate".into(),
            "http://[ff02::1]/image".into(),
        ] {
            assert_eq!(
                assets::fetch(&url.into(), &blocked, 1024, &cancel)
                    .await
                    .unwrap_err()
                    .code,
                "unsafe_destination"
            );
        }
        let allowed =
            json!({"files":{"assetAllowances":[{"origin":server.origin,"pathPrefix":"/owned"}]}});
        let bytes = assets::fetch(
            &format!("{}/owned/image", server.origin).into(),
            &allowed,
            1024,
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<numera_image_gen_mcp::compat::value::Value>(&bytes).unwrap(),
            json!({"image":"fixture"})
        );
        assert_eq!(
            assets::fetch(
                &format!("{}/owned-other/image", server.origin).into(),
                &allowed,
                1024,
                &cancel
            )
            .await
            .unwrap_err()
            .code,
            "unsafe_destination"
        );
        assert_eq!(
            assets::fetch(
                &format!("{}/owned/missing", server.origin).into(),
                &allowed,
                1024,
                &cancel
            )
            .await
            .unwrap_err()
            .code,
            "input_file_error"
        );
        assert_eq!(
            assets::fetch(
                &format!("{}/owned/redirect", server.origin).into(),
                &allowed,
                1024,
                &cancel
            )
            .await
            .unwrap_err()
            .code,
            "unsafe_destination"
        );
        assert_eq!(
            assets::fetch(
                &format!("{}/owned/image", server.origin).into(),
                &allowed,
                1,
                &cancel
            )
            .await
            .unwrap_err()
            .code,
            "invalid_response"
        );
        cancel.cancel();
        assert_eq!(
            assets::fetch(
                &format!("{}/owned/image", server.origin).into(),
                &allowed,
                1024,
                &cancel
            )
            .await
            .unwrap_err()
            .code,
            "cancelled"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(6), action).await;
    server.close().await;
    result.unwrap();
}
