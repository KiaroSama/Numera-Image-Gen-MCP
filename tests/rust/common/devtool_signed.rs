use numera_image_gen_mcp::{compat::hash::sha256, devtools::artifact};
use std::path::Path;

pub fn prepared(root: &Path) {
    variant(root, false);
}

pub fn variant(root: &Path, update: bool) {
    for name in ["bin", "docs/assets"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    let binary = if cfg!(windows) {
        "bin/numera-image-gen-mcp.exe"
    } else {
        "bin/numera-image-gen-mcp"
    };
    let bytes: &[u8] = if update {
        b"updated synthetic install fixture; never executed"
    } else {
        b"synthetic install fixture; never executed"
    };
    let logo = include_bytes!("../../../docs/assets/logo.png");
    std::fs::write(root.join(binary), bytes).unwrap();
    std::fs::write(root.join("docs/assets/logo.png"), logo).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-signing")
        .join(if update { "update" } else { "approved" });
    let manifest = std::fs::read(fixtures.join(format!("{}.json", artifact::target()))).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    assert_eq!(parsed["binary_sha256"], sha256(bytes));
    assert_eq!(parsed["files"][1]["sha256"], sha256(logo));
    std::fs::write(root.join("manifest.json"), manifest).unwrap();
    std::fs::copy(
        fixtures.join(format!("{}.sig", artifact::target())),
        root.join("manifest.sig"),
    )
    .unwrap();
}
