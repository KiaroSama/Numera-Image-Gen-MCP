use numera_image_gen_mcp::compat::value::Value;
use std::path::Path;

pub fn original() -> Vec<u8> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/images.json");
    let value: Value =
        serde_json::from_slice(&std::fs::read(file).expect("source image oracle")).unwrap();
    numera_image_gen_mcp::files::images::decode_base64(
        value[0]["base64"].as_str().unwrap(),
        1048576,
    )
    .unwrap()
}
