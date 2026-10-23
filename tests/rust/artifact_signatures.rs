mod common;
#[path = "common/devtool_signed.rs"]
mod signed_fixture;
use numera_image_gen_mcp::devtools::artifact::verify;

#[test]
fn unsigned_or_tampered_prepared_artifact_is_never_installable() {
    let workspace = common::Workspace::new();
    let manifest = workspace.0.join("manifest.json");
    std::fs::write(
        &manifest,
        b"{\"version\":\"0.2.0\",\"publisher\":\"KiaroSama\",\"files\":[]}",
    )
    .unwrap();
    let error = verify(&workspace.0)
        .err()
        .expect("Unsigned artifact must fail closed.");
    assert_eq!(error.code, "invalid_artifact");
    std::fs::write(workspace.0.join("manifest.sig"), [0u8; 384]).unwrap();
    let error = verify(&workspace.0)
        .err()
        .expect("A fabricated signature must fail closed.");
    assert_eq!(error.code, "invalid_artifact");
    assert!(!workspace.0.join(".numera-install.lock").exists());
}

#[cfg(unix)]
#[test]
fn special_manifest_input_rejects_without_waiting_for_a_fifo_writer() {
    use numera_image_gen_mcp::{
        devtools::supervisor::{Bounds, run},
        logging::Logger,
    };
    use std::{ffi::CString, os::unix::ffi::OsStrExt, process::Command, sync::atomic::AtomicBool};
    let workspace = common::Workspace::new();
    let source = workspace.0.join("prepared");
    let target = workspace.0.join("installed");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&target).unwrap();
    let path = CString::new(source.join("manifest.json").as_os_str().as_bytes()).unwrap();
    // SAFETY: valid NUL-terminated fixture path, isolated empty directory; no other process owns it.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut command = Command::new(env!("CARGO_BIN_EXE_numera-dev"));
    command
        .args(["setup", "Install", "--root"])
        .arg(&target)
        .arg("--artifact")
        .arg(&source);
    let logger = Logger::new(&workspace.0.join("logs"), "INFO");
    let outcome = run(
        &mut command,
        &workspace.0,
        Bounds::new(5000, 2000).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.reason.is_none(),
        "Special-file rejection must not rely on a timeout."
    );
    assert!(outcome.cleanup_verified);
    assert!(!target.join(".numera-install.lock").exists());
    assert!(!target.join("manifest.json").exists());
}

#[test]
fn genuine_historical_manifest_cannot_substitute_an_unapproved_logo() {
    let workspace = common::Workspace::new();
    signed_fixture::prepared(&workspace.0);
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-signing");
    let target = numera_image_gen_mcp::devtools::artifact::target();
    std::fs::copy(
        fixtures.join(format!("{target}.json")),
        workspace.0.join("manifest.json"),
    )
    .unwrap();
    std::fs::copy(
        fixtures.join(format!("{target}.sig")),
        workspace.0.join("manifest.sig"),
    )
    .unwrap();
    std::fs::write(
        workspace.0.join("docs/assets/logo.png"),
        b"synthetic logo fixture; not branding proof",
    )
    .unwrap();
    assert!(verify(&workspace.0).is_err());
}

#[test]
fn approved_manifest_accepts_exact_bytes_and_rejects_changed_signature_or_member() {
    let workspace = common::Workspace::new();
    signed_fixture::prepared(&workspace.0);
    let artifact = verify(&workspace.0).unwrap();
    assert_eq!(artifact.manifest.publisher, "KiaroSama");
    assert_eq!(artifact.manifest.version, "0.1.0");
    let signature = std::fs::read(workspace.0.join("manifest.sig")).unwrap();
    let mut wrong = signature.clone();
    wrong[0] ^= 1;
    std::fs::write(workspace.0.join("manifest.sig"), wrong).unwrap();
    assert!(verify(&workspace.0).is_err());
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-signing/approved");
    let unrelated = std::fs::read(fixtures.join(format!(
        "{}.wrong.sig",
        numera_image_gen_mcp::devtools::artifact::target()
    )))
    .unwrap();
    std::fs::write(workspace.0.join("manifest.sig"), unrelated).unwrap();
    assert!(
        verify(&workspace.0).is_err(),
        "A genuinely signed manifest from an unrelated key must fail."
    );
    std::fs::write(workspace.0.join("manifest.sig"), signature).unwrap();
    assert_eq!(
        std::fs::read(workspace.0.join("docs/assets/logo.png")).unwrap(),
        numera_image_gen_mcp::build_identity::LOGO
    );
    std::fs::write(workspace.0.join("docs/assets/logo.png"), b"modified").unwrap();
    assert!(verify(&workspace.0).is_err());
}
