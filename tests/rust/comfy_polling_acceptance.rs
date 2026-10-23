mod common;
#[path = "common/comfy_source.rs"]
mod source;
#[path = "common/wire.rs"]
mod wire;

use common::Workspace;
use numera_image_gen_mcp::{
    adapters::comfy,
    compat::value::{Value, json},
    files::images::{decode_base64, inspect},
    services::{polling, request},
};
use std::{collections::HashMap, io, path::Path};
use tokio_util::sync::CancellationToken;
use wire::{Reply, json_reply, serve};

#[tokio::test]
async fn source_comfy_wait_completes_first_poll_then_pre_cancel_preserves_selected_error() {
    let workspace = Workspace::new();
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
    assert!(std::fs::metadata(&file).unwrap().len() <= 1048576);
    let rows = Value::parse_with_limit(&std::fs::read_to_string(file).unwrap(), 1048576).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "png")
        .unwrap();
    let bytes = decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes, &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    let served = image.bytes.clone();
    serve(
        |request| {
            if request.path.starts_with("/history") {
                json_reply(json!({"job":{
                    "status":{"completed":true},
                    "outputs":{"save":{"images":[{"filename":"final.png","type":"output"}]}}
                }}))
            } else {
                Reply {
                    status: 200,
                    mime: "image/png",
                    body: served.clone(),
                }
            }
        },
        |origin, requests| async move {
            let config = source::config(&workspace, &origin, json!({}));
            let (_, connection) = config.select(None).unwrap();
            assert_eq!(connection["requestTimeoutMs"], 2000);
            assert_eq!(connection["discoveryTimeoutMs"], 2000);
            let parsed = request::parse(json!({"prompt":"Draw a café"})).unwrap();
            let adapter = source::input(&connection, &parsed, &[], None, "generate");
            let connection = adapter.connection;
            let headers = HashMap::new();
            let job = "job".into();
            let normal = CancellationToken::new();
            let limits = || comfy::OutputLimits {
                item: 100 * 1024 * 1024,
                aggregate: 100 * 1024 * 1024,
            };
            let result = polling::wait(
                &normal,
                || async {
                    let status = comfy::status(
                        connection,
                        &headers,
                        &job,
                        &normal,
                        limits(),
                        |_, _| Ok(()),
                        || Ok(()),
                    )
                    .await
                    .map_err(io::Error::other)?;
                    assert!(status.terminal.is_none());
                    Ok::<_, io::Error>(status.result)
                },
                || io::Error::other("fixture cancellation"),
            )
            .await
            .unwrap();
            assert_eq!(result.images.len(), 1);
            let item = &result.images[0];
            assert_eq!(item.bytes.as_ref().unwrap(), &image.bytes);
            assert!(item.base64.is_none() && item.url.is_none() && item.error.is_none());
            {
                let calls = requests.lock().unwrap();
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].path, "/history/job");
                assert_eq!(
                    calls[1].path,
                    "/view?filename=final.png&subfolder=&type=output"
                );
                for call in calls.iter() {
                    assert_eq!(call.method, "GET");
                    assert!(call.body.is_empty());
                    assert!(!call.headers.to_ascii_lowercase().contains("authorization:"));
                }
            }
            let cancelled = CancellationToken::new();
            cancelled.cancel();
            let error = polling::wait(
                &cancelled,
                || async {
                    let status = comfy::status(
                        connection,
                        &headers,
                        &job,
                        &cancelled,
                        limits(),
                        |_, _| Ok(()),
                        || Ok(()),
                    )
                    .await
                    .map_err(io::Error::other)?;
                    Ok::<_, io::Error>(status.result)
                },
                || io::Error::other("fixture cancellation"),
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.to_string(), "fixture cancellation");
            assert_eq!(requests.lock().unwrap().len(), 2);
        },
    )
    .await;
}
