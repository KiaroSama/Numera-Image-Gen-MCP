use crate::common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};

impl Workspace {
    pub fn config(&self, base: &str, adapter: &str) -> Value {
        json!({"schemaVersion":1,"defaultConnection":"test","outputDir":self.0.join("outputs").to_str().expect("fixture path"),"stateDir":self.0.join("state").to_str().expect("fixture path"),
            "logging":{"directory":self.0.join("logs").to_str().expect("fixture path")},"files":{"allowedInputRoots":[self.0.join("inputs").to_str().expect("fixture path")]},
            "connections":{"test":{"adapter":adapter,"baseUrl":base,"auth":{"type":"none"},"defaultModel":"opaque/image","orchestrationModel":"opaque/orchestrator","requestTimeoutMs":3000}}})
    }
}
