use super::{application::Application, tools};
use crate::{
    compat::value::{Value, json},
    error::{NumeraError, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use std::sync::Arc;

#[derive(Clone)]
pub struct Handler {
    pub application: Arc<Application>,
    pub tools: Vec<Tool>,
    _lifetime: Option<super::handler_lifetime::HandlerLease>,
    modern_http: bool,
}
impl Handler {
    pub fn new(application: Arc<Application>) -> Result<Self> {
        Ok(Self {
            application,
            tools: tools::definitions()?,
            _lifetime: None,
            modern_http: false,
        })
    }
    pub(super) fn tracked(
        mut self,
        lifetime: &Arc<super::handler_lifetime::HandlerLifetime>,
    ) -> Self {
        self._lifetime = Some(lifetime.acquire());
        self.modern_http = true;
        self
    }
}
fn protocol() -> ErrorData {
    ErrorData::internal_error(
        "The operation failed. Inspect the redacted local log.",
        None,
    )
}

impl ServerHandler for Handler {
    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        if self.modern_http {
            std::borrow::Cow::Borrowed(ProtocolVersion::KNOWN_VERSIONS)
        } else {
            let versions = serde_json::from_str::<Vec<ProtocolVersion>>(
                r#"["2025-11-25","2025-06-18","2025-03-26","2024-11-05","2024-10-07"]"#,
            )
            .expect("fixed source protocol versions");
            std::borrow::Cow::Owned(versions)
        }
    }
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "numera-image-gen-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(tools::INSTRUCTIONS)
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.clone()))
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let name = request.name.as_ref();
        let definition = self
            .tools
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| ErrorData::invalid_params("Unknown tool name.", None))?;
        let original = context
            .extensions
            .get::<super::js_envelope::OriginalArguments>()
            .cloned()
            .or_else(|| {
                context
                    .extensions
                    .get::<hyper::http::request::Parts>()
                    .and_then(|parts| {
                        parts
                            .extensions
                            .get::<super::js_envelope::OriginalArguments>()
                    })
                    .cloned()
            });
        let args = if let Some(original) = &original {
            Value::from_js(&original.arguments)
        } else {
            Value::from_serde(&serde_json::Value::Object(
                request.arguments.unwrap_or_default(),
            ))
            .map_err(|_| protocol())?
        };
        let id = uuid::Uuid::new_v4().to_string();
        let start = std::time::Instant::now();
        self.application.logger.log(
            "INFO",
            "tool",
            "Tool started.",
            json!({"tool":name,"call_id":id}),
        );
        let mut return_mode = String::from("files");
        let action = async {
            if !matches!(name, "generate_image" | "edit_image")
                && args.as_object().is_some_and(|a| {
                    a.keys().any(|key| {
                        !definition.input_schema["properties"]
                            .as_object()
                            .is_some_and(|p| key.as_str().is_some_and(|key| p.contains_key(key)))
                    })
                })
            {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Tool arguments do not match the public schema.",
                ));
            }
            let snapshot = self.application.current()?;
            return_mode = snapshot.config.value["returnMode"]
                .as_str()
                .unwrap_or("files")
                .into();
            self.application
                .call_snapshot(snapshot, name, args.clone(), &context.ct)
                .await
        }
        .await;
        let (mut value, failed) = match action {
            Ok(value) => {
                self.application.logger.log("INFO","tool","Tool completed.",json!({"tool":name,"call_id":id,"duration_ms":start.elapsed().as_millis() as u64,"warning_count":value["warnings"].as_array().map_or(0,Vec::len)}));
                (value, false)
            }
            Err(error) => {
                self.application.logger.log("ERROR","tool","Tool failed.",json!({"tool":name,"call_id":id,"duration_ms":start.elapsed().as_millis() as u64,"code":error.code,"stage":error.stage,"http_status":error.http_status}));
                (json!({"error":error}), true)
            }
        };
        let mut content = vec![ContentBlock::text(value.to_string())];
        if matches!(name, "generate_image" | "edit_image") && !failed {
            let outputs = crate::error::array(&value["outputs"]).to_vec();
            let previews =
                args["return_mode"].as_str().unwrap_or(&return_mode) == "files_and_preview";
            for output in outputs {
                let id = output["output_id"].as_str().ok_or_else(protocol)?;
                content.push(ContentBlock::resource_link(
                    Resource::new(format!("numera-image://outputs/{id}"), id).with_mime_type(
                        output["mime_type"]
                            .as_str()
                            .unwrap_or("application/octet-stream"),
                    ),
                ));
                if previews {
                    match self.application.preview(id) {
                        Ok(preview) => {
                            if value.get("previews").is_none() {
                                value["previews"] = json!([]);
                            }
                            if let Some(previews) = value["previews"].as_array_mut() {
                                previews.push(json!({"output_id":id,"width":preview["width"],"height":preview["height"],"mime_type":preview["mimeType"]}));
                            }
                            content.push(ContentBlock::image(
                                preview["data"].as_str().unwrap_or(""),
                                preview["mimeType"].as_str().unwrap_or("image/png"),
                            ));
                        }
                        Err(error) => self.application.logger.log(
                            "WARNING",
                            "preview",
                            "Preview unavailable.",
                            json!({"error":error}),
                        ),
                    }
                }
            }
        }
        if name == "get_output_info" && args["preview"] == true && !failed {
            match args["output_id"]
                .as_str()
                .ok_or_else(|| NumeraError::new("invalid_input", "Output ID is required."))
                .and_then(|id| self.application.preview(id))
            {
                Ok(preview) => {
                    value["preview"] = json!({"width":preview["width"],"height":preview["height"],"mime_type":preview["mimeType"]});
                    content.push(ContentBlock::image(
                        preview["data"].as_str().unwrap_or(""),
                        preview["mimeType"].as_str().unwrap_or("image/png"),
                    ));
                }
                Err(error) => value["preview_error"] = json!(error),
            }
        }
        let failed = failed
            || (matches!(name, "generate_image" | "edit_image")
                && matches!(value["status"].as_str(), Some("failed" | "outcome_unknown")));
        let mut result = if failed {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        };
        if let Some(original) = original {
            *original.response.lock().map_err(|_| protocol())? = Some(value);
            // The authoritative result is inserted at this request's actual wire boundary.
            result.structured_content = None;
        } else {
            result.structured_content = Some(value.to_serde().map_err(|_| protocol())?);
        }
        Ok(result.into())
    }
    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new("numera-image://outputs/{id}", "output")
                .with_description("Validated original bytes of an owned output.")
                .with_mime_type("application/octet-stream"),
        ]))
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, ErrorData> {
        let id = request
            .uri
            .strip_prefix("numera-image://outputs/")
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
            .ok_or_else(|| ErrorData::invalid_params("Unknown owned output resource.", None))?;
        let image = self
            .application
            .original(id)
            .map_err(|_| ErrorData::invalid_params("Cannot read verified owned output.", None))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::blob(STANDARD.encode(&image.bytes), request.uri)
                .with_mime_type(image.mime),
        ])
        .into())
    }
}
