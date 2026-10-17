#[path = "common/http.rs"]
mod http_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::adapters::comfy::{self, OutputLimits};
use numera_image_gen_mcp::compat::value::json;
use std::{collections::HashMap, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn comfy_history_rejects_malformed_or_excessive_outputs_and_retains_only_completed_originals()
{
    let bytes = image_fixture::original();
    for mode in [
        "absent",
        "pending",
        "failed",
        "empty",
        "malformed",
        "skip-file",
        "too-many",
        "aggregate",
        "retain-error",
        "complete-error",
        "valid",
    ] {
        let download = bytes.clone();
        let server=Server::new(move |path|Reply {status:200,bytes:path.starts_with("/view?").then(||download.clone()),value:match mode {
            "absent"=>json!({}),"pending"=>json!({"original":{"status":{"completed":false}}}),"failed"=>json!({"original":{"status":{"status_str":"error"}}}),"malformed"=>json!({"original":2}),
            _=>json!({"original":{"status":{"completed":true,"status_str":"success"},"outputs":{"save":{"images":if mode=="empty"{json!([])}else if mode=="skip-file"{json!([{"filename":2,"type":"output"},{"filename":"a.png","type":"input"}])}else if mode=="too-many"{json!((0..11).map(|i|json!({"filename":format!("{i}.png"),"type":"output"})).collect::<Vec<_>>())}else{json!([{"filename":"original.png","type":"output"}])}}}}}),
        }}).await;
        let action = async {
            let c = json!({"baseUrl":server.origin,"baseUrlMode":"origin","adapter":"comfyui","discoveryTimeoutMs":2000,"workflow":{"outputNodes":["save"]}});
            let mut retained = Vec::new();
            let mut completed = false;
            let result = comfy::status(
                &c,
                &HashMap::new(),
                &"original".into(),
                &CancellationToken::new(),
                OutputLimits {
                    item: 1048576,
                    aggregate: if mode == "aggregate" { 1 } else { 1048576 },
                },
                |_, bytes| {
                    if mode == "retain-error" {
                        return Err(numera_image_gen_mcp::error::NumeraError::new(
                            "output_file_error",
                            "Controlled retention failure.",
                        ));
                    }
                    retained.push(bytes.to_vec());
                    Ok(())
                },
                || {
                    completed = true;
                    if mode == "complete-error" {
                        return Err(numera_image_gen_mcp::error::NumeraError::storage());
                    }
                    Ok(())
                },
            )
            .await;
            match mode {
                "absent" | "pending" => {
                    let result = result.unwrap();
                    assert!(result.result.is_none());
                    assert!(result.terminal.is_none());
                    assert!(!completed);
                }
                "failed" => {
                    assert_eq!(result.unwrap().terminal, Some("failed"));
                    assert!(!completed);
                }
                "valid" => {
                    assert_eq!(
                        result.unwrap().result.unwrap().images[0]
                            .bytes
                            .as_ref()
                            .unwrap(),
                        &bytes
                    );
                    assert_eq!(retained, vec![bytes.clone()]);
                }
                "empty" | "skip-file" => {
                    assert_eq!(result.err().unwrap().code, "no_image_returned")
                }
                "retain-error" => assert_eq!(result.err().unwrap().code, "output_file_error"),
                "complete-error" => assert_eq!(result.err().unwrap().code, "storage_error"),
                _ => assert_eq!(result.err().unwrap().code, "invalid_response"),
            }
            if mode == "too-many" {
                assert_eq!(retained.len(), 10);
            }
            if matches!(mode, "empty" | "skip-file" | "complete-error") {
                assert_eq!(server.requests.lock().unwrap().len(), 1);
            }
        };
        let result = tokio::time::timeout(Duration::from_secs(5), action).await;
        server.close().await;
        result.unwrap();
    }
}
