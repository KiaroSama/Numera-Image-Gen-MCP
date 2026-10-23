pub const PUBLISHER: &str = "KiaroSama";
pub const PRODUCT: &str = "Numera Image-Gen MCP";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SOURCE: &str = match option_env!("NUMERA_BUILD_SHA") {
    Some(value) => value,
    None => "unverified-source",
};
pub const LOGO: &[u8] = include_bytes!("../docs/assets/logo.png");
pub const SIGNER: &str = "31ccb588ae7e529535dcccd94f06d8d39ae1b8847b7003e884ad8f44cd340c80";

pub fn version(product: &str) -> String {
    format!(
        "{product} {VERSION}\nPublisher: {PUBLISHER}\nSource: {SOURCE}\nSigner: {SIGNER}\nTrust: self-signed; not public OS trust or Apple notarization\nLogo: {} embedded bytes",
        LOGO.len()
    )
}
