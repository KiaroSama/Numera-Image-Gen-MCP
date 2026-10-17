use crate::compat::value::{JsString, Value, json};
use crate::{
    capabilities,
    config::{Config, EnvironmentSource, snapshot_credentials},
    dimensions,
    error::{NumeraError, Result, array, object},
    http::api::{self, Body},
    runtime_paths::RuntimePaths,
};
use reqwest::Method;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub fn prefix(c: &Value) -> &'static str {
    match c["adapter"].as_str().unwrap_or("") {
        "comfyui" => "",
        "gemini" | "gemini-interactions" => "/v1beta",
        "openrouter-images" => "/api/v1",
        _ => "/v1",
    }
}
fn time() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
#[derive(Default)]
pub struct Discovery {
    cache: HashMap<String, (Instant, Value)>,
}

impl Discovery {
    pub async fn list(
        &mut self,
        config: &Config,
        runtime: &RuntimePaths,
        env: &impl EnvironmentSource,
        name: Option<&str>,
        refresh: bool,
        cancel: &CancellationToken,
    ) -> Result<Value> {
        let (id, c) = config.select(name)?;
        let old = self.cache.get(&id);
        if !refresh && let Some((until, catalog)) = old.filter(|(until, _)| *until > Instant::now())
        {
            let _ = until;
            let mut catalog = catalog.clone();
            catalog["cached"] = json!(true);
            return Ok(catalog);
        }
        let configured=array(&c["configuredModels"]).iter().map(|m|json!({"id":m["id"],"name":m["name"],"configured":true,
            "metadata":{"source":"configured","account_availability":"unknown","generation_verified":false}})).collect::<Vec<_>>();
        if !refresh && !configured.is_empty() {
            return Ok(
                json!({"models":configured,"complete":true,"warnings":["Configured model list only; provider availability is unverified. Use refresh for provider discovery."],"cached":false,"checked_at":time()}),
            );
        }
        let mut result = json!({"models":configured,"complete":true,"warnings":[],"cached":false,"checked_at":time()});
        let route = if c["gateway"] == "omniroute" {
            "images/generations"
        } else if c["gateway"] == "9router" {
            "models/image"
        } else if c["adapter"] == "openrouter-images" {
            "images/models"
        } else if c["adapter"] == "comfyui" {
            "object_info"
        } else {
            "models"
        };
        let mut operation = route.to_owned();
        let mut cursor: Option<JsString> = None;
        let mut seen = HashSet::new();
        let action=async {
            let pinned=snapshot_credentials(&c,env,runtime).await?;
            let headers=pinned.headers();
            for page in 0..20 {
                let mut query=json!({});
                if let Some(cursor)=&cursor {query[if c["adapter"].as_str().is_some_and(|s|s.starts_with("gemini")){"pageToken"}else{"after"}]=json!(cursor);}
                if c["gateway"]=="omniroute" && operation=="models" {query["limit"]=json!("100");}
                let value=api::json(
                    pinned.value(),
                    &headers,
                    api::Operation {
                        operation: &operation,
                        prefix: prefix(&c),
                        method: Method::GET,
                        body: Body::Empty,
                        query: Some(&query),
                    },
                    cancel,
                ).await?;
                object(&value)?;
                if c["adapter"]=="comfyui" {
                    let mut models=Vec::new();for (id,value) in object(&value)?{object(value)?;models.push(json!({"id":id,"metadata":value}));}
                    result["models"]=json!(models);break;
                }
                for item in array(value.get("data").filter(|v|!v.is_null()).unwrap_or(&value["models"])) {
                    object(item)?;
                    let Some(model)=item["id"].as_js_string().or_else(||item["name"].as_js_string()) else {continue;};
                    if model.is_empty() || (c["gateway"]=="omniroute" && operation=="models" && item["type"]!="image"){continue;}
                    let models=result["models"].as_array_mut().ok_or_else(NumeraError::runtime)?;
                    if let Some(existing)=models.iter_mut().find(|v|v["id"]==model) {existing["metadata"]["advertised"]=item.clone();}
                    else {models.push(json!({"id":model,"metadata":item}));}
                }
                if c["gateway"]=="omniroute" && operation==route {
                    operation="models".into();result["warnings"].as_array_mut().ok_or_else(NumeraError::runtime)?.push(json!("Image specialty catalog does not preserve pagination; explicitly supplementing with paginated unified image entries."));continue;
                }
                let next=value.get("nextPageToken").filter(|v|!v.is_null()).or_else(||value.get("next_page_token").filter(|v|!v.is_null())).or_else(||if value["has_more"]==true{value.get("last_id")}else{None});
                let Some(next)=next.filter(|v|!v.is_null() && **v!=false && **v!="") else {break;};
                let token=next.as_js_string().cloned().unwrap_or_else(||next.to_string().into());
                if !seen.insert(token.clone()){result["complete"]=json!(false);result["warnings"].as_array_mut().ok_or_else(NumeraError::runtime)?.push(json!("Repeated catalog cursor."));break;}
                cursor=Some(token);
                if c["adapter"].as_str().is_some_and(|s|s.starts_with("gemini")){operation=c["paths"]["models"].as_str().unwrap_or("models").into();}
                if page==19 {result["complete"]=json!(false);result["warnings"].as_array_mut().ok_or_else(NumeraError::runtime)?.push(json!("Catalog page limit reached."));}
            }
            Ok::<(),NumeraError>(())
        }.await;
        if let Err(error) = action {
            result["complete"] = json!(false);
            result["warnings"]
                .as_array_mut()
                .ok_or_else(NumeraError::runtime)?
                .push(json!(error));
            if let Some((_, old)) = old {
                result["models"] = old["models"].clone();
                result["warnings"]
                    .as_array_mut()
                    .ok_or_else(NumeraError::runtime)?
                    .push(json!("Using stale catalog; availability is unknown."));
            }
        }
        self.cache.insert(
            id,
            (Instant::now() + Duration::from_secs(60), result.clone()),
        );
        Ok(result)
    }

    pub async fn model(
        config: &Config,
        runtime: &RuntimePaths,
        env: &impl EnvironmentSource,
        name: Option<&str>,
        model: &JsString,
        cancel: &CancellationToken,
    ) -> Result<Value> {
        let (id, c) = config.select(name)?;
        let mut cap = capabilities::capabilities(&c, model);
        let mut advertised = Value::Null;
        if (c["gateway"] == "9router" || c["adapter"] == "openrouter-images")
            && let Ok(pinned) = snapshot_credentials(&c, env, runtime).await
        {
            let operation = if c["gateway"] == "9router" {
                "models/info".into()
            } else {
                format!(
                    "images/models/{}/endpoints",
                    model
                        .checked_str()?
                        .split('/')
                        .map(crate::adapters::encode_component)
                        .collect::<Vec<_>>()
                        .join("/")
                )
            };
            let query = (c["gateway"] == "9router").then(|| json!({"id":model}));
            if let Ok(value) = api::json(
                pinned.value(),
                &pinned.headers(),
                api::Operation {
                    operation: &operation,
                    prefix: if c["gateway"] == "9router" {
                        "/v1"
                    } else {
                        "/api/v1"
                    },
                    method: Method::GET,
                    body: Body::Empty,
                    query: query.as_ref(),
                },
                cancel,
            )
            .await
            {
                advertised = value;
            }
        }
        let mut dimensions = dimensions::image_dimensions(
            &c,
            model,
            config.value["files"]["maxPixels"]
                .as_u64()
                .unwrap_or(64000000),
        );
        dimensions["documented_limits"] = dimensions::evidence(&c, model);
        dimensions["guidance"] = json!(
            "Inspect documented model limits before generation. Route safety max_pixels is not a model maximum; verify original returned dimensions and do not infer account entitlement."
        );
        cap["connection"] = json!(id);
        cap["model_id"] = json!(model);
        cap["display_name"] = array(&c["configuredModels"])
            .iter()
            .find(|m| m["id"] == model)
            .map_or(Value::Null, |m| m["name"].clone());
        cap["dimensions"] = dimensions;
        cap["advertised"] = advertised;
        Ok(cap)
    }
}
