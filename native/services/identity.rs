use crate::compat::value::json;
use crate::{
    compat::canonical_json::fingerprint,
    config::PinnedConnection,
    error::{NumeraError, Result},
};

pub fn connection_identity(name: &str, connection: &PinnedConnection) -> Result<String> {
    let c = connection.value();
    let origin = url::Url::parse(&String::from_utf16_lossy(
        c["baseUrl"].as_utf16().ok_or_else(NumeraError::runtime)?,
    ))
    .map_err(|_| NumeraError::runtime())?
    .origin()
    .ascii_serialization();
    let mut identity = json!({"connection":name,"adapter":c["adapter"],"origin":origin,"base":c["baseUrl"],"auth":c["auth"],
        "account_fingerprint":fingerprint(&json!(connection.headers()))?});
    if let Some(gateway) = c.get("gateway") {
        identity["gateway"] = gateway.clone();
    }
    fingerprint(&identity)
}
