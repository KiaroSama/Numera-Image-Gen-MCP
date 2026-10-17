use crate::error::{NumeraError, Result};
use crate::runtime_paths::private_directory;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub fn safe_relative(value: &str) -> Result<&str> {
    let invalid = || NumeraError::new("invalid_input", "Unsafe relative output path or filename.");
    if value.is_empty()
        || Path::new(value).is_absolute()
        || value.chars().any(|c| c < ' ' || ":<>\"|?*".contains(c))
    {
        return Err(invalid());
    }
    for part in value.split(['/', '\\']) {
        let first = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with(['.', ' '])
            || matches!(first.as_str(), "con" | "prn" | "aux" | "nul")
            || first
                .strip_prefix("com")
                .or_else(|| first.strip_prefix("lpt"))
                .is_some_and(|v| v.len() == 1 && matches!(v.as_bytes()[0], b'1'..=b'9'))
        {
            return Err(invalid());
        }
    }
    Ok(value)
}

pub fn input(raw: &str, roots: &[PathBuf], limit: usize) -> Result<Vec<u8>> {
    let path = if raw.starts_with("file:") {
        url::Url::parse(raw)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .ok_or_else(|| NumeraError::new("input_file_error", "Invalid file URL."))?
    } else {
        PathBuf::from(raw)
    };
    let text = path.to_string_lossy();
    let tail = if cfg!(windows) && text.as_bytes().get(1) == Some(&b':') {
        text.get(2..).unwrap_or(text.as_ref())
    } else {
        text.as_ref()
    };
    if !path.is_absolute()
        || text.contains('\0')
        || text.starts_with("\\\\?")
        || text.starts_with("\\\\.")
        || tail.contains(':')
    {
        return Err(NumeraError::new(
            "input_file_error",
            "Expected an absolute regular image path without device paths or streams.",
        ));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|_| NumeraError::new("input_file_error", "Cannot resolve image file."))?;
    let allowed: Vec<_> = roots
        .iter()
        .filter_map(|r| fs::canonicalize(r).ok())
        .collect();
    if !allowed.iter().any(|r| canonical.starts_with(r)) {
        return Err(NumeraError::new(
            "permission_denied",
            "Image is outside allowed input roots.",
        ));
    }
    let before = fs::symlink_metadata(&canonical).map_err(|_| {
        NumeraError::new("input_file_error", "Input is not a bounded regular file.")
    })?;
    #[cfg(windows)]
    let before_handle = fs::File::open(&canonical)
        .map_err(|_| NumeraError::new("input_file_error", "Cannot open bounded image file."))?;
    let mut file = fs::File::open(&canonical)
        .map_err(|_| NumeraError::new("input_file_error", "Cannot open bounded image file."))?;
    let opened = file
        .metadata()
        .map_err(|_| NumeraError::new("input_file_error", "Cannot inspect bounded image file."))?;
    if !before.is_file()
        || !opened.is_file()
        || before.len() > limit as u64
        || opened.len() > limit as u64
    {
        return Err(NumeraError::new(
            "input_file_error",
            "Input is not a bounded regular file.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != opened.dev() || before.ino() != opened.ino() {
            return Err(NumeraError::new(
                "input_file_error",
                "Input changed or is not a bounded regular file.",
            ));
        }
    }
    #[cfg(windows)]
    {
        let identity = super::windows_identity::identity(&before_handle).map_err(|_| {
            NumeraError::new("input_file_error", "Cannot verify input file identity.")
        })?;
        if super::windows_identity::identity(&file).map_err(|_| {
            NumeraError::new("input_file_error", "Cannot verify input file identity.")
        })? != identity
        {
            return Err(NumeraError::new(
                "input_file_error",
                "Input changed or is not a bounded regular file.",
            ));
        }
    }
    let after = fs::canonicalize(&path)
        .map_err(|_| NumeraError::new("input_file_error", "Input path changed during opening."))?;
    if after != canonical || !allowed.iter().any(|r| after.starts_with(r)) {
        return Err(NumeraError::new(
            "permission_denied",
            "Input path changed outside allowed roots.",
        ));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NumeraError::new("input_file_error", "Cannot read bounded image file."))?;
    if bytes.len() > limit {
        return Err(NumeraError::new(
            "input_file_error",
            "Input exceeds byte limit.",
        ));
    }
    Ok(bytes)
}

pub fn output_directory(root: &Path, subdirectory: Option<&str>) -> Result<PathBuf> {
    output_directory_observed(root, subdirectory, |path| fs::symlink_metadata(path))
}

fn output_directory_observed(
    root: &Path,
    subdirectory: Option<&str>,
    mut metadata: impl FnMut(&Path) -> std::io::Result<fs::Metadata>,
) -> Result<PathBuf> {
    private_directory(root)?;
    let base = fs::canonicalize(root).map_err(|_| NumeraError::storage())?;
    let subdirectory = subdirectory
        .map(safe_relative)
        .transpose()?
        .map(str::to_owned)
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
    let mut current = base.clone();
    for part in subdirectory.split(['/', '\\']) {
        current.push(part);
        match metadata(&current) {
            Ok(info) if info.is_symlink() || !info.is_dir() => {
                return Err(NumeraError::new(
                    "output_file_error",
                    "Output path contains a link or non-directory.",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(&current) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(NumeraError::storage()),
            },
            Err(_) => return Err(NumeraError::storage()),
            _ => {}
        }
        let info = metadata(&current).map_err(|_| NumeraError::storage())?;
        if info.is_symlink() || !info.is_dir() {
            return Err(NumeraError::new(
                "output_file_error",
                "Output path contains a link or non-directory.",
            ));
        }
        let resolved = fs::canonicalize(&current).map_err(|_| NumeraError::storage())?;
        if resolved != current || !resolved.starts_with(&base) {
            return Err(NumeraError::new(
                "output_file_error",
                "Output path escapes output root.",
            ));
        }
        private_directory(&current)?;
    }
    Ok(current)
}

#[cfg(test)]
#[path = "paths_races.rs"]
mod races;
