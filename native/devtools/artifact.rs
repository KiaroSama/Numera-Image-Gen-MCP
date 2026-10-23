use crate::{
    compat::hash::sha256,
    error::{NumeraError, Result},
    files::paths::safe_relative,
};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub target: String,
    pub source_sha: String,
    pub publisher: String,
    pub binary_sha256: String,
    pub files: Vec<Member>,
}
pub struct Prepared {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub manifest_bytes: Vec<u8>,
}
fn invalid() -> NumeraError {
    NumeraError::new(
        "invalid_artifact",
        "Prepared artifact identity, signature, target or member integrity is invalid; no installation was changed.",
    )
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
pub fn target() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        _ => "unsupported",
    }
}
pub(super) fn read(root: &Path, name: &str, limit: u64) -> Result<Vec<u8>> {
    safe_relative(name).map_err(|_| invalid())?;
    if name.contains('\\') {
        return Err(invalid());
    }
    let mut path = root.to_path_buf();
    for part in name.split('/') {
        path.push(part);
        let info = fs::symlink_metadata(&path).map_err(|_| invalid())?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if info.file_attributes() & 0x400 != 0 {
                return Err(invalid());
            }
        }
        if info.is_symlink() {
            return Err(invalid());
        }
    }
    let before = fs::symlink_metadata(&path).map_err(|_| invalid())?;
    if !before.is_file() || before.len() > limit {
        return Err(invalid());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(&path).map_err(|_| invalid())?;
    let info = file.metadata().map_err(|_| invalid())?;
    if !info.is_file() || info.len() > limit {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let current = fs::symlink_metadata(&path).map_err(|_| invalid())?;
        if info.nlink() != 1 || current.dev() != info.dev() || current.ino() != info.ino() {
            return Err(invalid());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0
            || crate::files::windows_identity::links(&file).map_err(|_| invalid())? != 1
        {
            return Err(invalid());
        }
    }
    if !fs::canonicalize(&path)
        .map_err(|_| invalid())?
        .starts_with(root)
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}
fn public_key() -> &'static [u8] {
    // The approved public key is a reviewed build input, never taken from the prepared artifact.
    include_bytes!("../../docs/signing/publisher-key.der")
}
fn valid_version(version: &str) -> bool {
    let parts = version.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u32>().is_ok()
        })
}
pub fn verify(directory: &Path) -> Result<Prepared> {
    if !directory.is_absolute() {
        return Err(invalid());
    }
    let info = fs::symlink_metadata(directory).map_err(|_| invalid())?;
    if !info.is_dir() || info.is_symlink() {
        return Err(invalid());
    }
    let root = fs::canonicalize(directory).map_err(|_| invalid())?;
    let bytes = read(&root, "manifest.json", 1024 * 1024)?;
    let signature = read(&root, "manifest.sig", 1024)?;
    let key = public_key();
    let identity: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../docs/signing/publisher.json"))
            .map_err(|_| invalid())?;
    let date = |name: &str| {
        identity[name]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .ok_or_else(invalid)
    };
    let now = chrono::Utc::now();
    if sha256(key) != crate::build_identity::SIGNER
        || identity["publisher"] != crate::build_identity::PUBLISHER
        || now < date("not_before")?
        || now > date("not_after")?
    {
        return Err(invalid());
    }
    ring::signature::UnparsedPublicKey::new(&ring::signature::RSA_PKCS1_2048_8192_SHA256, key)
        .verify(&bytes, &signature)
        .map_err(|_| invalid())?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if !valid_version(&manifest.version)
        || manifest.publisher != "KiaroSama"
        || manifest.target != target()
        || !hex(&manifest.source_sha, 40)
        || !hex(&manifest.binary_sha256, 64)
        || manifest.files.is_empty()
        || manifest.files.len() > 4096
    {
        return Err(invalid());
    }
    let binary = if cfg!(windows) {
        "bin/numera-image-gen-mcp.exe"
    } else {
        "bin/numera-image-gen-mcp"
    };
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut original = false;
    let mut logo = false;
    for member in &manifest.files {
        let first = member.path.split('/').next().unwrap_or("");
        if !names.insert(member.path.to_lowercase())
            || !hex(&member.sha256, 64)
            || member.bytes > MAX_BYTES
            || member.path.starts_with('.') && member.path != ".env.example"
            || ![
                "bin",
                "docs",
                "schemas",
                "examples",
                "licenses",
                "README.md",
                "LICENSE",
                "THIRD_PARTY_NOTICES.md",
                "SECURITY.md",
                ".env.example",
            ]
            .contains(&first)
            || member.path.split('/').any(|part| {
                matches!(
                    part,
                    "secrets.md" | "config.local.json" | "state" | "outputs" | "logs"
                ) || part.starts_with('.') && part != ".env.example"
            })
        {
            return Err(invalid());
        }
        total = total
            .checked_add(member.bytes)
            .filter(|total| *total <= MAX_BYTES)
            .ok_or_else(invalid)?;
        let data = read(&root, &member.path, member.bytes)?;
        if data.len() as u64 != member.bytes || sha256(&data) != member.sha256 {
            return Err(invalid());
        }
        if member.path == binary {
            original = member.sha256 == manifest.binary_sha256;
        }
        if member.path == "docs/assets/logo.png" {
            logo = data == crate::build_identity::LOGO;
        }
    }
    if !original || !logo {
        return Err(invalid());
    }
    Ok(Prepared {
        root,
        manifest,
        manifest_bytes: bytes,
    })
}
