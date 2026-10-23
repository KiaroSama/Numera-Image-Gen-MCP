use crate::{
    error::{NumeraError, Result},
    logging::Logger,
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn invalid(code: &str, message: &str) -> NumeraError {
    NumeraError::new(code, message)
}

pub fn merge(
    text: &str,
    client: &str,
    remove: bool,
    executable: &Path,
    config: &Path,
) -> Result<String> {
    if client == "Codex" {
        return merge_toml(text, remove, executable, config);
    }
    if !["ClaudeDesktop", "ClaudeProject"].contains(&client) {
        return Err(invalid("INVALID_REQUEST", "Unknown registration client."));
    }
    let mut parsed: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text).map_err(|_| {
            invalid(
                "INVALID_HOST_CONFIG",
                "Host configuration must be valid JSON.",
            )
        })?
    };
    let object = parsed.as_object_mut().ok_or_else(|| {
        invalid(
            "INVALID_HOST_CONFIG",
            "Host configuration must be an object.",
        )
    })?;
    if !object.contains_key("mcpServers") {
        if remove {
            return Ok(text.into());
        }
        object.insert("mcpServers".into(), json!({}));
    }
    let servers = parsed["mcpServers"]
        .as_object_mut()
        .ok_or_else(|| invalid("INVALID_HOST_CONFIG", "mcpServers must be an object."))?;
    if remove {
        if servers.remove("numera-image-gen").is_none() {
            return Ok(text.into());
        }
    } else {
        let mut entry =
            json!({"command":executable,"args":["bridge"],"env":{"NUMERA_CONFIG":config}});
        if client == "ClaudeProject" {
            entry["type"] = json!("stdio");
        }
        servers.insert("numera-image-gen".into(), entry);
    }
    Ok(serde_json::to_string_pretty(&parsed)
        .map_err(|_| invalid("INVALID_HOST_CONFIG", "Cannot encode host configuration."))?
        + "\n")
}

fn bare_name(text: &str) -> Option<String> {
    let parts = text.split('.').map(str::trim).collect::<Vec<_>>();
    if parts.iter().any(|s| {
        s.is_empty()
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    }) {
        None
    } else {
        Some(parts.join("."))
    }
}
fn merge_toml(text: &str, remove: bool, executable: &Path, config: &Path) -> Result<String> {
    let invalid = || {
        invalid(
            "AMBIGUOUS_TOML",
            "Quoted, duplicate, multiline or ambiguous TOML must be updated manually.",
        )
    };
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let mut sections = Vec::new();
    let mut headers = HashSet::new();
    let mut section = String::new();
    let mut keys = HashSet::new();
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains("\"\"\"") || line.contains("'''") {
            return Err(invalid());
        }
        if line.starts_with('[') {
            let end = line.find(']').ok_or_else(invalid)?;
            if !line[end + 1..].trim().is_empty() && !line[end + 1..].trim().starts_with('#') {
                return Err(invalid());
            }
            section = bare_name(&line[1..end]).ok_or_else(invalid)?;
            if !headers.insert(section.clone()) {
                return Err(invalid());
            }
            sections.push((i, section.clone()));
            keys.clear();
            continue;
        }
        let (raw, value) = line.split_once('=').ok_or_else(invalid)?;
        let key = bare_name(raw).ok_or_else(invalid)?;
        if (section.is_empty() && (key == "mcp_servers" || key.starts_with("mcp_servers.")))
            || (section == "mcp_servers"
                && (key == "numera-image-gen" || key.starts_with("numera-image-gen.")))
        {
            return Err(invalid());
        }
        if (section == "mcp_servers.numera-image-gen"
            || section.starts_with("mcp_servers.numera-image-gen."))
            && !keys.insert(key)
        {
            return Err(invalid());
        }
        let mut quote = None;
        let mut escaped = false;
        let mut depth = 0i32;
        for c in value.chars() {
            if let Some(q) = quote {
                if escaped {
                    escaped = false;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            } else {
                match c {
                    '#' => break,
                    '"' | '\'' => quote = Some(c),
                    '[' | '{' => depth += 1,
                    ']' | '}' => depth -= 1,
                    _ => {}
                }
            }
            if depth < 0 {
                return Err(invalid());
            }
        }
        if quote.is_some() || depth != 0 {
            return Err(invalid());
        }
    }
    let owned = |s: &str| {
        s == "mcp_servers.numera-image-gen" || s.starts_with("mcp_servers.numera-image-gen.")
    };
    if sections.iter().any(|(_, s)| owned(s)) && !headers.contains("mcp_servers.numera-image-gen") {
        return Err(invalid());
    }
    let mut omitted = HashSet::new();
    for (i, (start, s)) in sections.iter().enumerate() {
        if owned(s) {
            for n in *start..sections.get(i + 1).map_or(lines.len(), |(n, _)| *n) {
                omitted.insert(n);
            }
        }
    }
    let mut retained = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !omitted.contains(i))
        .map(|(_, s)| *s)
        .collect::<String>();
    if remove {
        return Ok(retained);
    }
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    if !retained.is_empty() && !retained.ends_with('\n') {
        retained.push_str(nl);
    }
    let command = serde_json::to_string(&executable.to_string_lossy()).map_err(|_| invalid())?;
    let config = serde_json::to_string(&config.to_string_lossy()).map_err(|_| invalid())?;
    retained.push_str(
        &[
            "[mcp_servers.numera-image-gen]".into(),
            format!("command = {command}"),
            "args = [\"bridge\"]".into(),
            "startup_timeout_sec = 10".into(),
            "tool_timeout_sec = 360".into(),
            format!("env = {{ NUMERA_CONFIG = {config} }}"),
            String::new(),
        ]
        .join(nl),
    );
    Ok(retained)
}

struct Snapshot {
    bytes: Vec<u8>,
    signature: Option<String>,
    mode: u32,
}
fn signature(info: &fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{}:{}:{}:{}:{}:{}",
            info.dev(),
            info.ino(),
            info.len(),
            info.mtime_nsec(),
            info.ctime(),
            info.ctime_nsec()
        )
    }
    #[cfg(not(unix))]
    {
        format!(
            "{}:{:?}:{:?}",
            info.len(),
            info.modified().ok(),
            info.created().ok()
        )
    }
}
fn snapshot(path: &Path) -> Result<Snapshot> {
    let info = match fs::symlink_metadata(path) {
        Ok(info) => info,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Snapshot {
                bytes: vec![],
                signature: None,
                mode: 0o600,
            });
        }
        Err(_) => {
            return Err(invalid(
                "UNSAFE_HOST_PATH",
                "Cannot inspect host configuration.",
            ));
        }
    };
    if !info.is_file() || info.is_symlink() || info.len() > 16 * 1024 * 1024 {
        return Err(invalid(
            "UNSAFE_HOST_PATH",
            "Host target must be a bounded regular, unlinked file.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if info.nlink() != 1 {
            return Err(invalid(
                "UNSAFE_HOST_PATH",
                "Host target must be a bounded regular, unlinked file.",
            ));
        }
    }
    let mut file = fs::File::open(path)
        .map_err(|_| invalid("UNSAFE_HOST_PATH", "Cannot open host configuration."))?;
    #[cfg(windows)]
    {
        if crate::files::windows_identity::links(&file)
            .map_err(|_| invalid("UNSAFE_HOST_PATH", "Cannot verify host file links."))?
            != 1
        {
            return Err(invalid(
                "UNSAFE_HOST_PATH",
                "Host target must be a bounded regular, unlinked file.",
            ));
        }
    }
    let opened = file
        .metadata()
        .map_err(|_| invalid("HOST_CHANGED", "Cannot inspect opened host configuration."))?;
    if signature(&info) != signature(&opened) {
        return Err(invalid(
            "HOST_CHANGED",
            "Host target changed while opening.",
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("INVALID_HOST_CONFIG", "Cannot read host configuration."))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid(
            "INVALID_HOST_CONFIG",
            "Host configuration exceeds the byte limit.",
        ));
    }
    let current = fs::symlink_metadata(path)
        .map_err(|_| invalid("HOST_CHANGED", "Host target changed while reading."))?;
    if signature(&opened) != signature(&current)
        || signature(&opened)
            != signature(
                &file
                    .metadata()
                    .map_err(|_| invalid("HOST_CHANGED", "Cannot inspect host configuration."))?,
            )
    {
        return Err(invalid(
            "HOST_CHANGED",
            "Host target changed while reading.",
        ));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        opened.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mode = 0o600;
    Ok(Snapshot {
        bytes,
        signature: Some(signature(&opened)),
        mode,
    })
}
struct Lock {
    path: PathBuf,
    signature: String,
    file: Option<fs::File>,
}
impl Drop for Lock {
    fn drop(&mut self) {
        self.file.take();
        if fs::symlink_metadata(&self.path).is_ok_and(|m| signature(&m) == self.signature) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(path).map_err(|_| {
        invalid(
            "REGISTRATION_LOCKED",
            "Cannot create exclusive registration file.",
        )
    })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            invalid(
                "HOST_WRITE_FAILED",
                "Cannot write host configuration safely.",
            )
        })?;
    Ok(file)
}

pub fn register(
    client: &str,
    remove: bool,
    file: &Path,
    executable: &Path,
    config: &Path,
    dry_run: bool,
    logger: &Logger,
) -> Result<()> {
    if !file.is_absolute() || (!remove && (!executable.is_absolute() || !config.is_absolute())) {
        return Err(invalid(
            "INVALID_REQUEST",
            "Registration requires explicit absolute paths.",
        ));
    }
    let before = snapshot(file)?;
    let text = std::str::from_utf8(&before.bytes).map_err(|_| {
        invalid(
            "INVALID_HOST_CONFIG",
            "Host configuration must be valid UTF-8.",
        )
    })?;
    merge(text, client, remove, executable, config)?;
    if dry_run {
        logger.log(
            "INFO",
            "registration",
            "Registration dry-run validated; no host files changed.",
            crate::compat::value::json!({"client":client}),
        );
        return Ok(());
    }
    if remove && before.signature.is_none() {
        return Ok(());
    }
    let parent = file
        .parent()
        .ok_or_else(|| invalid("UNSAFE_HOST_PATH", "Host configuration has no parent."))?;
    fs::create_dir_all(parent).map_err(|_| {
        invalid(
            "HOST_WRITE_FAILED",
            "Cannot create host configuration directory.",
        )
    })?;
    let file = fs::canonicalize(parent)
        .map_err(|_| {
            invalid(
                "UNSAFE_HOST_PATH",
                "Cannot resolve host configuration directory.",
            )
        })?
        .join(file.file_name().ok_or_else(|| {
            invalid(
                "UNSAFE_HOST_PATH",
                "Host configuration filename is required.",
            )
        })?);
    let lock_path = file.with_file_name(format!(
        "{}.numera-lock",
        file.file_name().unwrap_or_default().to_string_lossy()
    ));
    let lock_file = write_new(
        &lock_path,
        json!({"pid":std::process::id(),"started":chrono::Utc::now().to_rfc3339()})
            .to_string()
            .as_bytes(),
        0o600,
    )?;
    let lock = Lock {
        signature: signature(
            &lock_file
                .metadata()
                .map_err(|_| invalid("REGISTRATION_LOCKED", "Cannot verify registration lock."))?,
        ),
        path: lock_path,
        file: Some(lock_file),
    };
    let before = snapshot(&file)?;
    let after = merge(
        std::str::from_utf8(&before.bytes).map_err(|_| {
            invalid(
                "INVALID_HOST_CONFIG",
                "Host configuration must be valid UTF-8.",
            )
        })?,
        client,
        remove,
        executable,
        config,
    )?;
    if after.as_bytes() == before.bytes {
        return Ok(());
    }
    let temporary = Temporary(file.with_file_name(format!(
        "{}.numera-{}.tmp",
        file.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    )));
    drop(write_new(&temporary.0, after.as_bytes(), before.mode)?);
    if before.signature.is_some() {
        drop(write_new(
            &file.with_file_name(format!(
                "{}.numera-backup-{}",
                file.file_name().unwrap_or_default().to_string_lossy(),
                uuid::Uuid::new_v4()
            )),
            &before.bytes,
            0o600,
        )?);
    }
    let current = snapshot(&file)?;
    if current.signature != before.signature || current.bytes != before.bytes {
        return Err(invalid(
            "HOST_CHANGED",
            "Host configuration changed concurrently; no overwrite performed.",
        ));
    }
    fs::rename(&temporary.0, &file)
        .map_err(|_| invalid("HOST_WRITE_FAILED", "Cannot publish host configuration."))?;
    drop(lock);
    logger.log(
        "INFO",
        "registration",
        "Only Numera registration updated; unrelated settings preserved.",
        crate::compat::value::json!({"client":client,"remove":remove}),
    );
    Ok(())
}
