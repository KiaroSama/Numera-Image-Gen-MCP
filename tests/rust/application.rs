mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/http.rs"]
mod http_fixture;
#[path = "common/image.rs"]
mod image_fixture;
#[path = "common/wire.rs"]
mod reload_wire;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::{
    config::{Config, ConfigReload, snapshot_credentials},
    jobs::Receipt,
    logging::Logger,
    mcp::application::Application,
    runtime_paths::RuntimePaths,
    services::{identity::connection_identity, request},
};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn public_tools_validate_arguments_store_outputs_and_reload_last_good_config() {
    let workspace = Workspace::new();
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        image_fixture::original(),
    );
    let server = Server::new(move |path| Reply {
        bytes: None,
        status: 200,
        value: if path.contains("images/generations") {
            json!({"data":[{"b64_json":encoded}]})
        } else {
            json!({"data":[{"id":"opaque/image"},{"id":"another/image"}]})
        },
    })
    .await;
    let file = workspace.0.join("config.json");
    let raw = workspace.config(&format!("{}/v1", server.origin), "openai-images");
    std::fs::write(&file, raw.to_string()).unwrap();
    let config = Config::parse(&raw.to_string(), &file, &[], &HashMap::new()).unwrap();
    let args = vec!["--config".into(), file.to_string_lossy().into_owned()];
    let reload = ConfigReload::new(config.clone(), file.clone(), args, HashMap::new());
    let logger = Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR"));
    let app = Application::new(
        config,
        Some(reload),
        logger.clone(),
        RuntimePaths::new(&workspace.0).unwrap(),
        HashMap::new(),
        CancellationToken::new(),
    )
    .unwrap();
    let signal = CancellationToken::new();
    let action = async {
        assert_eq!(
            app.call("health_check", json!({"probe":true}), &signal)
                .await
                .unwrap()["generation_verified"],
            false
        );
        assert_eq!(
            app.call("list_connections", json!({}), &signal)
                .await
                .unwrap()["connections"][0]["name"],
            "test"
        );
        let listed = app
            .call("list_models", json!({"refresh":true,"limit":1}), &signal)
            .await
            .unwrap();
        assert_eq!(listed["models"].as_array().unwrap().len(), 1);
        assert_eq!(listed["next_offset"], 1);
        let next = app
            .call("list_models", json!({"offset":1,"limit":1}), &signal)
            .await
            .unwrap();
        assert_eq!(next["models"][0]["id"], "another/image");
        assert_eq!(
            app.call(
                "get_model_capabilities",
                json!({"model":"opaque/image"}),
                &signal
            )
            .await
            .unwrap()["account_availability"],
            "unknown"
        );
        let generated = app
            .call(
                "generate_image",
                json!({"request_id":"public-image","prompt":"fixture"}),
                &signal,
            )
            .await
            .unwrap();
        assert_eq!(generated["status"], "completed");
        let id = generated["outputs"][0]["output_id"].as_str().unwrap();
        assert_eq!(
            app.call("get_output_info", json!({"output_id":id}), &signal)
                .await
                .unwrap()["verified"],
            true
        );
        assert_eq!(app.original(id).unwrap().bytes, image_fixture::original());
        assert_eq!(app.preview(id).unwrap()["mimeType"], "image/png");
        assert_eq!(
            app.call("list_outputs", json!({"limit":1}), &signal)
                .await
                .unwrap()["outputs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            app.call(
                "get_job",
                json!({"request_id":"public-image","refresh":true}),
                &signal
            )
            .await
            .unwrap()["outputs"],
            generated["outputs"]
        );
        assert_eq!(
            app.call("cancel_job", json!({"request_id":"public-image"}), &signal)
                .await
                .unwrap()["upstream_requested"],
            false
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.ends_with("images/generations"))
                .count(),
            1
        );
        for (name, args) in [
            ("list_models", json!({"limit":0})),
            ("list_outputs", json!({"offset":-1})),
            ("health_check", json!({"probe":"yes"})),
            ("get_model_capabilities", json!({"model":null})),
            ("get_output_info", json!({"output_id":"not-uuid"})),
            ("unknown", json!({})),
        ] {
            assert_eq!(
                app.call(name, args, &signal).await.unwrap_err().code,
                "invalid_input"
            );
        }
        let mut changed = raw.clone();
        changed["returnMode"] = json!("files_and_preview");
        std::fs::write(&file, changed.to_string()).unwrap();
        assert_eq!(app.return_mode().unwrap(), "files_and_preview");
        std::fs::write(&file, "not-json").unwrap();
        assert_eq!(app.return_mode().unwrap(), "files_and_preview");
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    app.shutdown();
    drop(app);
    drop(logger);
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn retained_routes_recover_original_handles_and_refuse_ambiguous_history_without_network() {
    let workspace = Workspace::new();
    let server = Server::new(|_| Reply {
        status: 200,
        bytes: None,
        value: json!({"id":"owned-remote","status":"in_progress","data":[]}),
    })
    .await;
    let file = workspace.0.join("config.json");
    let raw = workspace.config(&format!("{}/v1", server.origin), "openai-responses");
    std::fs::write(&file, raw.to_string()).unwrap();
    let config = Config::parse(&raw.to_string(), &file, &[], &HashMap::new()).unwrap();
    let reload = ConfigReload::new(
        config.clone(),
        file.clone(),
        vec!["--config".into(), file.to_str().unwrap().into()],
        HashMap::new(),
    );
    let app = Application::new(
        config,
        Some(reload),
        Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        RuntimePaths::new(&workspace.0).unwrap(),
        HashMap::new(),
        CancellationToken::new(),
    )
    .unwrap();
    let action = async {
        let first = app.current().unwrap();
        let (_, connection) = first.config.select(Some("test")).unwrap();
        let pinned = numera_image_gen_mcp::config::snapshot_credentials(
            &connection,
            &first.env,
            &first.runtime,
        )
        .await
        .unwrap();
        let identity =
            numera_image_gen_mcp::services::identity::connection_identity("test", &pinned).unwrap();
        {
            let mut store = app.store.lock().unwrap();
            for (id, identity) in [
                ("retained-route", identity.as_str()),
                ("unknown-route", "unavailable-identity"),
            ] {
                let receipt = numera_image_gen_mcp::jobs::Receipt::new(json!({
                    "request_id":id,"connection":"test","adapter":"openai-responses","operation":"generate",
                    "status":"prepared","requested_model":"opaque/image","outputs":[],"errors":[],"deviations":[],"warnings":[],
                    "generation_outcome":"not_submitted","storage_outcome":"not_started",
                    "upstream_job":{"id":"owned-remote","kind":"responses"},"created_at":"2026-10-13T00:00:00.000Z"
                }));
                let (_, mut receipt) = store.prepare(receipt, id, identity).unwrap();
                assert!(store.admit(id, "test", 8, 8).unwrap());
                receipt.value["status"] = json!("running");
                receipt.value["generation_outcome"] = json!("running");
                store.update(&mut receipt).unwrap();
                store.release(id, Some(&receipt)).unwrap();
            }
        }
        let mut changed = raw.clone();
        changed["connections"]["test"]["baseUrl"] = json!(format!("{}/changed", server.origin));
        changed["connections"]["test"]["defaultModel"] = json!("new/model");
        std::fs::write(&file, changed.to_string()).unwrap();
        assert_eq!(
            app.current().unwrap().config.value["connections"]["test"]["defaultModel"],
            "new/model"
        );
        let signal = CancellationToken::new();
        let job = app
            .call(
                "get_job",
                json!({"request_id":"retained-route","refresh":true}),
                &signal,
            )
            .await
            .unwrap();
        assert_eq!(job["status"], "running");
        assert_eq!(job["upstream_job"]["id"], "owned-remote");
        assert_eq!(
            server.requests.lock().unwrap().as_slice(),
            ["/v1/responses/owned-remote"]
        );
        let rejected = app
            .call(
                "get_job",
                json!({"request_id":"unknown-route","refresh":true}),
                &signal,
            )
            .await
            .unwrap_err();
        assert_eq!(rejected.code, "permission_denied");
        let mut ambiguous = raw.clone();
        ambiguous["connections"]["test"]["paths"] = json!({"responses":"changed-responses"});
        std::fs::write(&file, ambiguous.to_string()).unwrap();
        let rejected = app
            .call(
                "get_job",
                json!({"request_id":"retained-route","refresh":true}),
                &signal,
            )
            .await
            .unwrap_err();
        assert_eq!(rejected.code, "permission_denied");
        assert_eq!(
            server.requests.lock().unwrap().len(),
            1,
            "ambiguous routes never dispatch"
        );
        assert_eq!(
            app.store
                .lock()
                .unwrap()
                .get("retained-route")
                .unwrap()
                .value["upstream_job"]["id"],
            "owned-remote"
        );
        let original = image_fixture::original();
        {
            let mut store = app.store.lock().unwrap();
            let seeded = numera_image_gen_mcp::jobs::Receipt::new(json!({
                "request_id":"local-retained","connection":"removed","adapter":"openai-images",
                "operation":"generate","status":"prepared","requested_model":"opaque/image",
                "outputs":[],"errors":[],"deviations":[],"warnings":[],
                "generation_outcome":"not_submitted","storage_outcome":"writing",
                "created_at":"2026-10-13T00:00:00.000Z","output_requirements":{"count":1}
            }));
            let (_, mut receipt) = store.prepare(seeded, "local", "removed-identity").unwrap();
            assert!(store.admit("local-retained", "removed", 8, 8).unwrap());
            receipt.value["status"] = json!("finalizing");
            receipt.value["generation_outcome"] = json!("completed");
            store.update(&mut receipt).unwrap();
            store.reserve("local-retained", 1048576, 268435456).unwrap();
            store
                .retain("local-retained", 0, &original, Some(&receipt))
                .unwrap();
            store.release("local-retained", Some(&receipt)).unwrap();
        }
        let recovered = app
            .call(
                "get_job",
                json!({"request_id":"local-retained","refresh":true}),
                &signal,
            )
            .await
            .unwrap();
        assert_eq!(recovered["status"], "completed");
        assert_eq!(
            std::fs::read(recovered["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            original
        );
        assert_eq!(
            server.requests.lock().unwrap().len(),
            1,
            "retained bytes need no credentials or network"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(5), action).await;
    app.shutdown();
    drop(app);
    server.close().await;
    result.unwrap();
}

#[test]
fn public_request_schema_rejects_invalid_inputs_without_coercion() {
    assert_eq!(
        request::parse(json!({"prompt":"café 日本語 🌍"})).unwrap()["count"],
        1
    );
    for value in [
        json!(null),
        json!({"prompt":""}),
        json!({"prompt":"ok","unexpected":true}),
        json!({"prompt":"ok","count":0}),
        json!({"prompt":"ok","count":1.5}),
        json!({"prompt":"ok","request_id":"../escape"}),
        json!({"prompt":"ok","model":""}),
        json!({"prompt":"ok","wait":"true"}),
        json!({"prompt":"ok","seed":1.5}),
        json!({"prompt":"ok","provider_options":[]}),
        json!({"prompt":"ok","reference_images":[{"type":"path","path":""}]}),
        json!({"prompt":"ok","edit_region":{"x":0,"y":0,"width":0,"height":1}}),
        json!({"prompt":"ok","target_language":" "}),
        json!({"prompt":"ok","return_mode":"url"}),
        json!({"prompt":"ok","output_format":"gif"}),
    ] {
        assert_eq!(request::parse(value).unwrap_err().code, "invalid_input");
    }
    for value in [
        json!({}),
        json!({"type":"unknown","path":"file"}),
        json!({"type":"output_id","output_id":"bad"}),
        json!({"type":"url","url":"bad"}),
        json!({"type":"path","path":"file","extra":true}),
    ] {
        assert_eq!(
            request::validate_source(&value).unwrap_err().code,
            "invalid_input"
        );
    }
    for value in [
        json!({"type":"path","path":"fixture"}),
        json!({"type":"url","url":"https://fixture.example/image"}),
        json!({"type":"data_url","data_url":"data:image/png;base64,YQ=="}),
        json!({"type":"output_id","output_id":"123e4567-e89b-42d3-a456-426614174000"}),
    ] {
        request::validate_source(&value).unwrap();
    }
}

struct ReloadHost(Application);
impl Drop for ReloadHost {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

// Same-owner application dispatch complements the separate SDK child fixture.
#[tokio::test]
async fn exact_comfy_reload_retains_the_prepared_application_store_owner_without_release() {
    let workspace = Workspace::new();
    let root = &workspace.0;
    reload_wire::serve(
        |request| {
            assert!(
                !request
                    .headers
                    .to_ascii_lowercase()
                    .contains("authorization:")
            );
            if request.method == "POST" {
                assert_eq!(request.body, b"{}");
            } else {
                assert_eq!(request.method, "GET");
                assert!(request.body.is_empty());
            }
            reload_wire::json_reply(if request.method == "POST" {
                json!({"cancelled":true})
            } else if request.path.starts_with("/api/jobs/") {
                json!({"id":"original-handle","status":"cancelled"})
            } else {
                json!({})
            })
        },
        |origin, requests| async move {
            let file = root.join("config.json");
            let raw = json!({
                "schemaVersion":1,"defaultConnection":"local",
                "outputDir":root.join("outputs").to_str().unwrap(),
                "stateDir":root.join("state").to_str().unwrap(),
                "logging":{"directory":root.join("logs").to_str().unwrap()},
                "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
                "connections":{"local":{
                    "adapter":"comfyui","baseUrl":origin,"baseUrlMode":"origin",
                    "auth":{"type":"none"}
                }}
            });
            std::fs::write(&file, raw.to_string()).unwrap();
            let config = Config::parse(&raw.to_string(), &file, &[], &HashMap::new()).unwrap();
            let reload = ConfigReload::new(
                config.clone(),
                file.clone(),
                vec!["--config".into(), file.to_str().unwrap().into()],
                HashMap::new(),
            );
            let runtime = RuntimePaths::new(root).unwrap();
            let host = ReloadHost(
                Application::new(
                    config.clone(),
                    Some(reload),
                    Arc::new(Logger::new(&root.join("logs"), "ERROR")),
                    runtime.clone(),
                    HashMap::new(),
                    CancellationToken::new(),
                )
                .unwrap(),
            );
            let app = &host.0;
            let original_store = app.store.clone();
            let owner = original_store.lock().unwrap().owner.clone();
            let (_, connection) = config.select(Some("local")).unwrap();
            assert_eq!(connection["requestTimeoutMs"], 300000);
            assert_eq!(connection["discoveryTimeoutMs"], 15000);
            let pinned = snapshot_credentials(&connection, &HashMap::new(), &runtime)
                .await
                .unwrap();
            let identity = connection_identity("local", &pinned).unwrap();
            let old = json!({
                "request_id":"existing","connection":"local","adapter":"comfyui",
                "operation":"generate","status":"running","requested_model":"image-model",
                "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
                "generation_outcome":"not_submitted","storage_outcome":"not_started",
                "another_submission_may_charge":false,
                "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z",
                "upstream_job":{"kind":"comfyui","id":"original-handle"}
            });
            {
                let mut store = app.store.lock().unwrap();
                for (id, identity) in [
                    ("existing", identity.as_str()),
                    ("unknown", "unavailable-account"),
                ] {
                    let mut value = old.clone();
                    value["request_id"] = json!(id);
                    let (fresh, prepared) = store
                        .prepare(Receipt::new(value.clone()), "fixture-fingerprint", identity)
                        .unwrap();
                    assert!(fresh);
                    assert_eq!(prepared.generation, Some(1));
                    assert!(store.owns(id, Some(&prepared)).unwrap());
                    assert_eq!(store.owner, owner);
                    assert_eq!(store.get(id).unwrap().value, value);
                }
            }
            let mut changed = raw.clone();
            changed["connections"]["local"]["baseUrl"] = json!(format!("{origin}/changed"));
            changed["connections"]["local"]["defaultModel"] = json!("new-model");
            std::fs::write(&file, changed.to_string()).unwrap();
            let signal = CancellationToken::new();
            tokio::time::timeout(
                Duration::from_secs(5),
                app.call(
                    "get_job",
                    json!({"request_id":"existing","refresh":true}),
                    &signal,
                ),
            )
            .await
            .expect("Source retained-handle poll exceeded five seconds")
            .unwrap();
            let cancelled = tokio::time::timeout(
                Duration::from_secs(5),
                app.call("cancel_job", json!({"request_id":"existing"}), &signal),
            )
            .await
            .expect("Source retained-handle cancellation exceeded five seconds")
            .unwrap();
            assert_eq!(cancelled["upstream_requested"], true);
            assert_eq!(cancelled["upstream_cancelled"], true);
            let paths = || {
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|request| request.path.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                paths(),
                [
                    "/history/original-handle",
                    "/api/jobs/original-handle/cancel",
                    "/api/jobs/original-handle",
                ]
            );
            let unknown = tokio::time::timeout(
                Duration::from_secs(5),
                app.call(
                    "get_job",
                    json!({"request_id":"unknown","refresh":true}),
                    &signal,
                ),
            )
            .await
            .expect("Source unavailable-identity refusal exceeded five seconds")
            .unwrap_err();
            assert_eq!(unknown.code, "permission_denied");
            assert_eq!(paths().len(), 3);
            let current = app.current().unwrap();
            assert!(Arc::ptr_eq(&original_store, &current.store));
            assert_eq!(current.store.lock().unwrap().owner, owner);
            assert_eq!(
                current.config.value["connections"],
                config.value["connections"]
            );
            let mut ambiguous = raw;
            ambiguous["connections"]["local"]["paths"] = json!({"history":"changed-history"});
            std::fs::write(&file, ambiguous.to_string()).unwrap();
            let error = tokio::time::timeout(
                Duration::from_secs(5),
                app.call(
                    "get_job",
                    json!({"request_id":"existing","refresh":true}),
                    &signal,
                ),
            )
            .await
            .expect("Source ambiguous-route refusal exceeded five seconds")
            .unwrap_err();
            assert_eq!(error.code, "permission_denied");
            assert_eq!(paths().len(), 3);
            assert!(Arc::ptr_eq(&original_store, &app.current().unwrap().store));
            assert_eq!(app.store.lock().unwrap().owner, owner);
        },
    )
    .await;
}
