mod common;
#[path = "common/http.rs"]
mod http_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::adapters::{comfy, types::Input};
use numera_image_gen_mcp::compat::value::json;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn installed_comfy_graph_gpu_preflight_upload_submission_and_terminal_history_are_explicit() {
    let workspace = Workspace::new();
    let cpu = Arc::new(AtomicBool::new(false));
    let mode = Arc::new(AtomicUsize::new(0));
    let original = image_fixture::original();
    let served = original.clone();
    let (device, history) = (cpu.clone(), mode.clone());
    let server=Server::new(move |path| Reply { bytes: path.starts_with("/view?").then(||served.clone()), status:200,value: match path {
        "/object_info"=>json!({"Text":{},"Sampler":{},"Load":{},"Save":{}}),
        "/system_stats"=>json!({"devices":[{"type":if device.load(Ordering::SeqCst){"cpu"}else{"cuda"}}]}),
        "/upload/image"=>json!({"name":"owned.png","subfolder":"inputs"}),
        "/prompt"=>json!({"prompt_id":"original-job","node_errors":{}}),
        "/history/original-job"=>match history.load(Ordering::SeqCst) {
            0=>json!({}),
            1=>json!({"original-job":{"status":{"completed":false},"outputs":{}}}),
            2=>json!({"original-job":{"status":{"status_str":"error"},"outputs":{}}}),
            3=>json!({"original-job":{"status":{"completed":true},"outputs":{"save":{"images":[]}}}}),
            _=>json!({"original-job":{"status":{"completed":true},"outputs":{"save":{"images":[{"filename":"original.png","subfolder":"owned","type":"output"}]}}}}),
        },
        path if path.starts_with("/view?")=>json!(null),
        _=>panic!("unexpected Comfy fixture path"),
    }}).await;
    let config = json!({"adapter":"comfyui","baseUrl":server.origin,"baseUrlMode":"api-prefix","discoveryTimeoutMs":2000,"requestTimeoutMs":2000,
        "workflow":{"graph":{"text":{"class_type":"Text","inputs":{"text":"old"}},"sampler":{"class_type":"Sampler","inputs":{"seed":0,"batch_size":1}},"load":{"class_type":"Load","inputs":{"image":"old.png","mask":"old-mask.png"}},"save":{"class_type":"Save","inputs":{"images":["sampler",0]}}},"bindings":{"prompt":{"node":"text","input":"text"},"seed":{"node":"sampler","input":"seed"},"count":{"node":"sampler","input":"batch_size"},"image":{"node":"load","input":"image"},"mask":{"node":"load","input":"mask"}},"outputNodes":["save"],"requireGpu":true}});
    let image =
        numera_image_gen_mcp::files::images::inspect(image_fixture::original(), &json!({}), None)
            .unwrap();
    let references = [image.clone()];
    let request = json!({"prompt":"fixture","count":1,"seed":42,"provider_options":{}});
    let input = Input {
        connection: &config,
        request: &request,
        model: &"workflow".into(),
        operation: "edit",
        references: &references,
        mask: Some(&image),
    };
    let cancel = CancellationToken::new();
    let action = async {
        let graph = comfy::prepare(&input, &HashMap::new(), &cancel)
            .await
            .unwrap();
        assert_eq!(graph["text"]["inputs"]["text"], "fixture");
        assert_eq!(graph["sampler"]["inputs"]["seed"], 42);
        let (job, warnings) = comfy::submit(&input, graph, &HashMap::new(), &cancel)
            .await
            .unwrap();
        assert_eq!(job.id, "original-job");
        assert!(warnings.is_empty());
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.as_str() == "/upload/image")
                .count(),
            2
        );
        for state in 0..4 {
            mode.store(state, Ordering::SeqCst);
            let status = comfy::status(
                &config,
                &HashMap::new(),
                &job.id,
                &cancel,
                comfy::OutputLimits {
                    item: 1048576,
                    aggregate: 2097152,
                },
                |_, _| panic!("empty history must not retain bytes"),
                || Ok(()),
            )
            .await;
            match state {
                0 | 1 => assert!(status.unwrap().result.is_none()),
                2 => assert_eq!(status.unwrap().terminal, Some("failed")),
                _ => assert_eq!(status.err().unwrap().code, "no_image_returned"),
            }
        }
        mode.store(4, Ordering::SeqCst);
        let mut retained = Vec::new();
        let mut completed = false;
        let status = comfy::status(
            &config,
            &HashMap::new(),
            &job.id,
            &cancel,
            comfy::OutputLimits {
                item: 1048576,
                aggregate: 2097152,
            },
            |index, bytes| {
                assert_eq!(index, 0);
                retained = bytes.to_vec();
                Ok(())
            },
            || {
                completed = true;
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(completed);
        assert_eq!(retained, original);
        assert_eq!(
            status.result.unwrap().images[0].bytes.as_ref().unwrap(),
            &original
        );
        cpu.store(true, Ordering::SeqCst);
        assert_eq!(
            comfy::prepare(&input, &HashMap::new(), &cancel)
                .await
                .unwrap_err()
                .code,
            "unsupported_operation"
        );
        assert!(
            !server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|path| path.contains("interrupt"))
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    drop(workspace);
    result.unwrap();
}
