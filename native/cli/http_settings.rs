use crate::{
    error::{NumeraError, Result},
    runtime_paths::private_directory,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

const SETTINGS_LIMIT: usize = 4096;
fn invalid(message: &str) -> NumeraError {
    NumeraError::new("invalid_configuration", message)
}
fn denied() -> NumeraError {
    NumeraError::new(
        "permission_denied",
        "HTTP settings must be private, unlinked files inside the installation root.",
    )
}

pub fn parse_port(value: &str) -> Result<u16> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid(
            "HTTP port must be a decimal integer between 1 and 65535.",
        ));
    }
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| invalid("HTTP port must be a decimal integer between 1 and 65535."))
}

pub fn split_port_args(args: &[String]) -> Result<(Option<u16>, Vec<String>)> {
    let mut selected = None;
    let mut remaining = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--" {
            remaining.extend_from_slice(&args[index..]);
            break;
        }
        if args[index] == "--port" {
            index += 1;
            selected =
                Some(parse_port(args.get(index).ok_or_else(|| {
                    invalid("--port requires a decimal port value.")
                })?)?);
        } else if let Some(value) = args[index].strip_prefix("--port=") {
            selected = Some(parse_port(value)?);
        } else {
            remaining.push(args[index].clone());
        }
        index += 1;
    }
    Ok((selected, remaining))
}

pub fn port(root: &Path, args: &[String], env: &HashMap<String, String>) -> Result<u16> {
    if let Some(port) = split_port_args(args)?.0 {
        return Ok(port);
    }
    if let Some(value) = env.get("NUMERA_HTTP_PORT") {
        return parse_port(value);
    }
    let directory = config_directory(root)?;
    let _lock = settings_lock(&directory)?;
    let path = directory.join("http.json");
    match read_private(&path, SETTINGS_LIMIT)? {
        None => Ok(1969),
        Some(text) => settings_port(&text),
    }
}

fn settings_port(text: &str) -> Result<u16> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| invalid("HTTP settings must contain valid UTF-8 JSON."))?;
    let object = value
        .as_object()
        .filter(|object| object.len() == 1 && object.contains_key("port"))
        .ok_or_else(|| invalid("HTTP settings must contain only a port field."))?;
    object["port"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| invalid("HTTP settings port must be an integer between 1 and 65535."))
}

pub fn save_port(root: &Path, port: u16) -> Result<()> {
    if port == 0 {
        return Err(invalid("HTTP port must be between 1 and 65535."));
    }
    let directory = config_directory(root)?;
    let _lock = settings_lock(&directory)?;
    let path = directory.join("http.json");
    let previous = read_private(&path, SETTINGS_LIMIT)?;
    if let Some(text) = &previous {
        settings_port(text)?;
    }
    atomic_write(
        &directory,
        &path,
        &(json!({"port":port}).to_string() + "\n"),
        previous.as_deref(),
    )
}

pub fn token_path(root: &Path) -> Result<PathBuf> {
    let path = config_directory(root)?.join("http-token");
    if let Some(file) = opened_private(&path, false, false)? {
        drop(file);
    }
    Ok(path)
}

pub fn token(root: &Path, env: &HashMap<String, String>) -> Result<String> {
    if let Some(value) = env.get("NUMERA_HTTP_TOKEN") {
        validate_token(value)?;
        return Ok(value.clone());
    }
    let directory = config_directory(root)?;
    let _lock = settings_lock(&directory)?;
    let path = directory.join("http-token");
    if let Some(value) = read_private(&path, 256)? {
        validate_token(&value)?;
        return Ok(value);
    }
    // Two UUIDv4 values contain 244 random bits after their version/variant bits are fixed.
    let value = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    atomic_write(&directory, &path, &value, None)?;
    Ok(value)
}

fn validate_token(value: &str) -> Result<()> {
    if !(32..=256).contains(&value.len())
        || !value.bytes().all(|byte| (b'!'..=b'~').contains(&byte))
    {
        return Err(invalid(
            "Local HTTP token must contain 32 to 256 visible ASCII characters.",
        ));
    }
    Ok(())
}

fn config_directory(root: &Path) -> Result<PathBuf> {
    if root
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(denied());
    }
    if !root.is_absolute() || !root.is_dir() {
        return Err(invalid(
            "Installation root must be an existing absolute directory.",
        ));
    }
    let root = fs::canonicalize(root).map_err(|_| denied())?;
    let path = root.join("config");
    match fs::symlink_metadata(&path) {
        Ok(info) => {
            if !info.is_dir() || info.is_symlink() || reparse(&info) {
                return Err(denied());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(NumeraError::storage()),
        },
        Err(_) => return Err(denied()),
    }
    let resolved = fs::canonicalize(&path).map_err(|_| denied())?;
    if resolved != path || !resolved.starts_with(root) {
        return Err(denied());
    }
    private_directory(&path)?;
    restrict(&path)?;
    Ok(path)
}

fn reparse(info: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        info.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = info;
        false
    }
}

fn opened_private(path: &Path, write: bool, create: bool) -> Result<Option<File>> {
    let before = match fs::symlink_metadata(path) {
        Ok(info) => Some(info),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(denied()),
    };
    if before
        .as_ref()
        .is_some_and(|info| !info.is_file() || info.is_symlink() || reparse(info))
    {
        return Err(denied());
    }
    if before.is_none() && !create {
        return Ok(None);
    }
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    if create {
        options.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(path).map_err(|_| denied())?;
    let info = file.metadata().map_err(|_| denied())?;
    if !info.is_file() || reparse(&info) {
        return Err(denied());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if info.nlink() != 1
            || info.permissions().mode() & 0o077 != 0
            || before
                .as_ref()
                .is_some_and(|old| old.dev() != info.dev() || old.ino() != info.ino())
        {
            return Err(denied());
        }
    }
    #[cfg(windows)]
    {
        if crate::files::windows_identity::links(&file).map_err(|_| denied())? != 1 {
            return Err(denied());
        }
        restrict(path)?;
        use std::os::windows::fs::OpenOptionsExt;
        let current = OpenOptions::new()
            .read(true)
            .custom_flags(0x00200000)
            .open(path)
            .map_err(|_| denied())?;
        if crate::files::windows_identity::identity(&current).map_err(|_| denied())?
            != crate::files::windows_identity::identity(&file).map_err(|_| denied())?
        {
            return Err(denied());
        }
    }
    let current = fs::symlink_metadata(path).map_err(|_| denied())?;
    if current.is_symlink() || reparse(&current) || !current.is_file() {
        return Err(denied());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if current.dev() != info.dev() || current.ino() != info.ino() || current.nlink() != 1 {
            return Err(denied());
        }
    }
    Ok(Some(file))
}

fn read_private(path: &Path, limit: usize) -> Result<Option<String>> {
    let Some(file) = opened_private(path, false, false)? else {
        return Ok(None);
    };
    if file.metadata().map_err(|_| denied())?.len() > limit as u64 {
        return Err(invalid("HTTP settings file exceeds its byte limit."));
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| denied())?;
    if bytes.len() > limit {
        return Err(invalid("HTTP settings file exceeds its byte limit."));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| invalid("HTTP settings must be valid UTF-8."))
}

fn settings_lock(directory: &Path) -> Result<File> {
    let path = directory.join(".http-settings.lock");
    let existing = opened_private(&path, true, false)?;
    let file = match existing {
        Some(file) => file,
        None => match opened_private(&path, true, true) {
            Ok(Some(file)) => file,
            _ => opened_private(&path, true, false)?.ok_or_else(denied)?,
        },
    };
    file.try_lock().map_err(|_| {
        NumeraError::new(
            "configuration_busy",
            "Another process is updating HTTP settings; try again after it finishes.",
        )
    })?;
    Ok(file)
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn atomic_write(directory: &Path, target: &Path, text: &str, expected: Option<&str>) -> Result<()> {
    if fs::canonicalize(directory).map_err(|_| denied())? != directory {
        return Err(denied());
    }
    let temporary = Temporary(directory.join(format!(".http-{}.tmp", Uuid::new_v4())));
    let mut file = opened_private(&temporary.0, true, true)?.ok_or_else(denied)?;
    restrict(&temporary.0)?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| NumeraError::storage())?;
    drop(file);
    if fs::canonicalize(directory).map_err(|_| denied())? != directory {
        return Err(denied());
    }
    if read_private(target, SETTINGS_LIMIT)?.as_deref() != expected {
        return Err(NumeraError::new(
            "configuration_changed",
            "HTTP settings changed concurrently; no file was replaced.",
        ));
    }
    if expected.is_none() {
        // Publish a complete new file without replacing a concurrently created destination.
        fs::hard_link(&temporary.0, target).map_err(|_| NumeraError::storage())?;
        fs::remove_file(&temporary.0).map_err(|_| NumeraError::storage())?;
    } else {
        replace(&temporary.0, target)?;
    }
    #[cfg(unix)]
    {
        File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(|_| NumeraError::storage())?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn restrict(_path: &Path) -> Result<()> {
    Ok(())
}
#[cfg(not(windows))]
fn replace(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target).map_err(|_| NumeraError::storage())
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(path: *const u16, information: u32, descriptor: *const c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
        fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
    }
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    struct SecurityDescriptor(*mut c_void);
    impl Drop for SecurityDescriptor {
        fn drop(&mut self) {
            // SAFETY: pointer is the live LocalAlloc-compatible result of the security descriptor API.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    pub(super) fn restrict(path: &Path) -> Result<()> {
        let sddl: Vec<u16> = "D:P(A;;FA;;;OW)".encode_utf16().chain(Some(0)).collect();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: NUL-terminated UTF-16 input, valid output pointer, revision1; allocation freed below.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(denied());
        }
        let descriptor = SecurityDescriptor(descriptor);
        let name = wide(path);
        // SAFETY: descriptor from the API is live; path UTF-16 is NUL-terminated. Protect the DACL.
        let success = unsafe { SetFileSecurityW(name.as_ptr(), 0x80000004, descriptor.0) };
        if success == 0 {
            return Err(denied());
        }
        Ok(())
    }
    pub(super) fn replace(source: &Path, target: &Path) -> Result<()> {
        let source = wide(source);
        let target = wide(target);
        // SAFETY: both strings stay live and NUL-terminated; REPLACE_EXISTING|WRITE_THROUGH.
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x9) } == 0 {
            return Err(NumeraError::storage());
        }
        Ok(())
    }
}
#[cfg(windows)]
use windows::{replace, restrict};
