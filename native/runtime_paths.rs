use crate::error::{NumeraError, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub temp: PathBuf,
    pub cache: PathBuf,
}

impl RuntimePaths {
    pub fn new(root: &Path) -> Result<Self> {
        if !root.is_absolute() || !root.is_dir() {
            return Err(NumeraError::new(
                "invalid_configuration",
                "Project root must be an existing absolute directory.",
            ));
        }
        let root = fs::canonicalize(root).map_err(|_| NumeraError::storage())?;
        let temp = root.join(".runtime/temp");
        let cache = root.join(".runtime/cache");
        let paths = Self { root, temp, cache };
        for directory in [&paths.temp, &paths.cache] {
            paths.verify_existing_ancestors(directory)?;
            private_directory(directory)?;
            paths.verify(directory)?;
        }
        Ok(paths)
    }

    pub fn verify(&self, path: &Path) -> Result<()> {
        if !path.is_absolute()
            || !path.starts_with(&self.root)
            || path.components().any(|c| matches!(c, Component::ParentDir))
        {
            return Err(NumeraError::new(
                "permission_denied",
                "Managed temporary/cache path must remain inside the project.",
            ));
        }
        self.verify_existing_ancestors(path)
    }

    fn verify_existing_ancestors(&self, path: &Path) -> Result<()> {
        for ancestor in path.ancestors() {
            if ancestor.exists() {
                let resolved = fs::canonicalize(ancestor).map_err(|_| NumeraError::storage())?;
                if !resolved.starts_with(&self.root) {
                    return Err(NumeraError::new(
                        "permission_denied",
                        "Managed path resolves outside the project.",
                    ));
                }
                return Ok(());
            }
        }
        Err(NumeraError::storage())
    }

    pub fn child_environment(&self) -> [(String, String); 3] {
        let path = self.temp.to_string_lossy().into_owned();
        [
            ("TEMP".into(), path.clone()),
            ("TMP".into(), path.clone()),
            ("TMPDIR".into(), path),
        ]
    }
}

pub fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|_| NumeraError::storage())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| NumeraError::storage())?;
    }
    Ok(())
}
