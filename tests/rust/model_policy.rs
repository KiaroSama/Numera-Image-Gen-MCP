#[path = "common/http.rs"]
mod http_fixture;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::services::{model_policy, request};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn endpoint_descriptors_pin_compatible_provider_and_refuse_fallback_or_dropped_requirements()
{
    let failing = Arc::new(AtomicBool::new(false));
    let failure = failing.clone();
    let server = Server::new(move |_| Reply {
        bytes: None,
        status: if failure.load(Ordering::SeqCst) { 503 } else { 200 },
        value: json!({"endpoints":[
            {"provider_tag":"small","provider_slug":"small-provider","supported_parameters":{"resolution":{"type":"enum","values":["1K"]}}},
            {"provider_tag":"large","provider_slug":"large-provider","allowed_passthrough_parameters":["strength"],"supported_parameters":{"resolution":{"type":"enum","values":["2K"]},"n":{"type":"range","min":1,"max":2},"stream":{"type":"boolean"}}}
        ]}),
    }).await;
    let c = json!({"adapter":"openrouter-images","baseUrl":format!("{}/api/v1",server.origin),"discoveryTimeoutMs":2000});
    let signal = CancellationToken::new();
    let action = async {
        let selected = request::parse(json!({"prompt":"draw","image_size":"2K","count":2,"provider_options":{"stream":true,"provider":{"options":{"large-provider":{"strength":0.7}}}}})).unwrap();
        let (effective, warnings) = model_policy::validate(
            &c,
            &HashMap::new(),
            &"family/model".into(),
            selected,
            &signal,
        )
        .await
        .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(
            effective["provider_options"]["provider"]["only"],
            json!(["large"])
        );
        assert_eq!(
            effective["provider_options"]["provider"]["allow_fallbacks"],
            false
        );
        assert_eq!(
            effective["provider_options"]["provider"]["options"]["large-provider"]["strength"],
            0.7
        );
        for raw in [
            json!({"prompt":"draw","quality":"high"}),
            json!({"prompt":"draw","image_size":"2K","count":3}),
            json!({"prompt":"draw","provider_options":{"unrouted":1}}),
            json!({"prompt":"draw","image_size":"2K","provider_options":{"provider":{"only":["small"]}}}),
            json!({"prompt":"draw","image_size":"2K","provider_options":{"provider":{"options":{"wrong-provider":{"strength":0.7}}}}}),
            json!({"prompt":"draw","image_size":"2K","provider_options":{"provider":{"options":{"large-provider":{"unknown":0.7}}}}}),
        ] {
            assert_eq!(
                model_policy::validate(
                    &c,
                    &HashMap::new(),
                    &"family/model".into(),
                    request::parse(raw).unwrap(),
                    &signal
                )
                .await
                .unwrap_err()
                .code,
                "unsupported_parameter"
            );
        }
        failing.store(true, Ordering::SeqCst);
        let (effective, warnings) = model_policy::validate(
            &c,
            &HashMap::new(),
            &"chosen".into(),
            request::parse(json!({"prompt":"draw"})).unwrap(),
            &signal,
        )
        .await
        .unwrap();
        assert!(warnings[0].contains("unknown"));
        assert!(effective.get("model").is_none());
        assert!(server.requests.lock().unwrap().iter().all(|path| path
            == "/api/v1/images/models/family/model/endpoints"
            || path == "/api/v1/images/models/chosen/endpoints"));
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}
