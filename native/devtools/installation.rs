use super::artifact::{self, Prepared};
use crate::{
    compat::hash::sha256,
    error::{NumeraError, Result},
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

fn invalid(message: &str) -> NumeraError {
    NumeraError::new("installation_blocked", message)
}
struct Lock {
    path: PathBuf,
    file: Option<File>,
}
impl Drop for Lock {
    fn drop(&mut self) {
        self.file.take();
        if fs::remove_file(&self.path).is_err() {
            eprintln!("WARNING: Installation lock cleanup failed.");
        }
    }
}
struct Tree {
    path: PathBuf,
    preserve: bool,
}
impl Drop for Tree {
    fn drop(&mut self) {
        if !self.preserve && fs::remove_dir_all(&self.path).is_err() {
            eprintln!("WARNING: Installation staging retained for recovery.");
        }
    }
}

fn checked_path(root: &Path, name: &str, create: bool) -> Result<PathBuf> {
    crate::files::paths::safe_relative(name)?;
    let mut path = root.to_path_buf();
    let pieces = name.split('/').collect::<Vec<_>>();
    for (index, piece) in pieces.iter().enumerate() {
        path.push(piece);
        let parent = index + 1 != pieces.len();
        if parent && create && !path.exists() {
            fs::create_dir(&path).map_err(|_| invalid("Cannot create installation directory."))?;
        }
        match fs::symlink_metadata(&path) {
            Ok(info) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if info.file_attributes() & 0x400 != 0 {
                        return Err(invalid("Installation path contains a reparse point."));
                    }
                }
                if info.is_symlink() || parent && !info.is_dir() || !parent && !info.is_file() {
                    return Err(invalid(
                        "Installation path contains a link or unexpected file type.",
                    ));
                }
                if !fs::canonicalize(&path)
                    .map_err(|_| invalid("Cannot resolve installation path."))?
                    .starts_with(root)
                {
                    return Err(invalid("Installation path escapes its root."));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid("Cannot inspect installation destination.")),
        }
    }
    Ok(path)
}
fn write_new(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o755 } else { 0o600 });
    }
    #[cfg(not(unix))]
    let _ = executable;
    let mut file = options
        .open(path)
        .map_err(|_| invalid("Cannot stage an exclusive installation file."))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| invalid("Cannot persist installation staging."))
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let from = from
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let to = to.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
    // SAFETY: live NUL-terminated strings; replace/write-through applies only verified owned files.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 9) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub fn install(prepared: &Prepared, destination: &Path, update: bool) -> Result<()> {
    let root =
        fs::canonicalize(destination).map_err(|_| invalid("Installation root is unavailable."))?;
    if root == prepared.root || !root.is_dir() {
        return Err(invalid(
            "Prepared source and installation destination must be separate.",
        ));
    }
    let current = if update {
        Some(artifact::verify(&root)?)
    } else {
        if root.join("manifest.json").exists() {
            return Err(invalid("Installation already exists; use Update."));
        }
        None
    };
    let lock_path = checked_path(&root, ".numera-install.lock", false)?;
    let lock_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| invalid("Another installation owns the destination lock."))?;
    let _lock = Lock {
        path: lock_path,
        file: Some(lock_file),
    };
    // Verify again after taking the lock, including source bytes and current installation ownership.
    let verified = artifact::verify(&prepared.root)?;
    if verified.manifest_bytes != prepared.manifest_bytes {
        return Err(invalid("Prepared artifact changed before installation."));
    }
    if let Some(current) = &current
        && artifact::verify(&root)?.manifest_bytes != current.manifest_bytes
    {
        return Err(invalid("Installed artifact changed before update."));
    }
    let staging = root.join(format!(".numera-staging-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|_| invalid("Cannot create exclusive staging directory."))?;
    let mut tree = Tree {
        path: staging,
        preserve: false,
    };
    let mut entries = Vec::new();
    for member in &verified.manifest.files {
        let bytes = artifact::read(&verified.root, &member.path, member.bytes)?;
        if bytes.len() as u64 != member.bytes || sha256(&bytes) != member.sha256 {
            return Err(invalid("Prepared member changed during staging."));
        }
        entries.push((member.path.clone(), bytes));
    }
    entries.push((
        "manifest.sig".into(),
        artifact::read(&verified.root, "manifest.sig", 1024)?,
    ));
    entries.push(("manifest.json".into(), verified.manifest_bytes));
    let mut snapshots = Vec::new();
    for (index, (name, bytes)) in entries.iter().enumerate() {
        let target = checked_path(&root, name, false)?;
        let before = if target.exists() {
            if current.as_ref().is_none_or(|old| {
                !old.manifest.files.iter().any(|member| member.path == *name)
                    && !matches!(name.as_str(), "manifest.json" | "manifest.sig")
            }) {
                return Err(invalid(
                    "Installation would overwrite a file not owned by the existing distribution.",
                ));
            }
            Some(artifact::read(&root, name, 256 * 1024 * 1024)?)
        } else {
            None
        };
        write_new(
            &tree.path.join(format!("new-{index}")),
            bytes,
            name.starts_with("bin/"),
        )?;
        if let Some(before) = &before {
            write_new(
                &tree.path.join(format!("old-{index}")),
                before,
                name.starts_with("bin/"),
            )?;
        }
        snapshots.push(before);
    }
    let mut published = 0;
    let attempt = (|| -> Result<()> {
        for (index, (name, _)) in entries.iter().enumerate() {
            let target = checked_path(&root, name, true)?;
            let now = if target.exists() {
                Some(artifact::read(&root, name, 256 * 1024 * 1024)?)
            } else {
                None
            };
            if now != snapshots[index] {
                return Err(invalid(
                    "Installation changed concurrently; replacement aborted.",
                ));
            }
            replace(&tree.path.join(format!("new-{index}")), &target).map_err(|_| {
                invalid("Prepared replacement is locked or unavailable; rollback required.")
            })?;
            published += 1;
        }
        artifact::verify(&root)?;
        Ok(())
    })();
    if let Err(error) = attempt {
        let mut restored = true;
        for index in (0..published).rev() {
            let name = &entries[index].0;
            let target = match checked_path(&root, name, false) {
                Ok(target) => target,
                Err(_) => {
                    restored = false;
                    continue;
                }
            };
            // Do not overwrite another writer that changed a just-published file.
            if artifact::read(&root, name, 256 * 1024 * 1024)
                .ok()
                .as_deref()
                != Some(entries[index].1.as_slice())
            {
                restored = false;
                continue;
            }
            restored &= if snapshots[index].is_some() {
                replace(&tree.path.join(format!("old-{index}")), &target).is_ok()
            } else {
                fs::remove_file(&target).is_ok()
            };
        }
        if !restored {
            tree.preserve = true;
            return Err(invalid(
                "Rollback incomplete; preserve the owned staging directory for recovery.",
            ));
        }
        return Err(error);
    }
    Ok(())
}
