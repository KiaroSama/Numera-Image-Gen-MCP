use crate::compat::value::{Value, json};
use crate::{
    error::{NumeraError, Result},
    files::{
        images::{Image, inspect},
        paths::{output_directory, safe_relative},
    },
    jobs::{Receipt, Store},
};
use std::{fs, io::Read, path::Path};
use uuid::Uuid;

struct Temporary(std::path::PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub struct Publication<'a> {
    pub id: &'a str,
    pub index: u32,
    pub subdir: Option<&'a str>,
    pub prefix: Option<&'a str>,
    pub deviations: &'a [String],
    pub expected: Option<&'a Receipt>,
    pub stable: bool,
}

pub fn save(
    image: &Image,
    config: &Value,
    store: &mut Store,
    publication: Publication<'_>,
) -> Result<Value> {
    let Publication {
        id,
        index,
        subdir,
        prefix,
        deviations,
        expected,
        stable,
    } = publication;
    let error = || {
        NumeraError::new("output_file_error","Cannot save verified image; retained results can be recovered with get_job. Do not resubmit automatically.").stage("storage")
    };
    let root = Path::new(config["outputDir"].as_str().ok_or_else(error)?);
    let directory = output_directory(root, subdir)?;
    if let Some(prefix) = prefix {
        safe_relative(prefix)?;
        if prefix.contains(['/', '\\']) {
            return Err(NumeraError::new(
                "invalid_input",
                "Filename prefix cannot contain directories.",
            ));
        }
    }
    let filename = |output_id: &str| -> Result<String> {
        Ok(directory
            .join(format!(
                "{}{}-{}-{}.{}",
                prefix.map(|s| format!("{s}-")).unwrap_or_default(),
                id,
                index,
                output_id,
                image.extension
            ))
            .to_string_lossy()
            .into_owned())
    };
    let item = if stable {
        Some(store.publication(id, index, filename, expected)?)
    } else {
        None
    };
    let output_id = item
        .as_ref()
        .map(|item| item.output_id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let target = std::path::PathBuf::from(match &item {
        Some(item) => item.target.as_ref().ok_or_else(error)?.clone(),
        None => filename(&output_id)?,
    });
    let parent = fs::canonicalize(target.parent().ok_or_else(error)?).map_err(|_| error())?;
    if parent != directory || !parent.starts_with(fs::canonicalize(root).map_err(|_| error())?) {
        return Err(NumeraError::new(
            "permission_denied",
            "Retained output target does not match the configured output directory.",
        ));
    }
    if item
        .as_ref()
        .is_some_and(|item| item.sha256.as_deref() != Some(image.sha256.as_str()))
    {
        return Err(NumeraError::new(
            "output_file_error",
            "Retained output hash does not match image bytes.",
        )
        .stage("storage"));
    }
    if item.as_ref().is_some_and(|item| item.phase == "committed") {
        return store.output(&output_id);
    }
    let temporary = Temporary(directory.join(format!(".{}-{}.tmp", output_id, Uuid::new_v4())));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary.0).map_err(|_| error())?;
    store
        .persistence
        .write(&mut file, &image.bytes)
        .and_then(|_| store.persistence.sync(&file))
        .map_err(|_| error())?;
    drop(file);
    if stable && !store.owns(id, expected)? {
        return Err(NumeraError::new(
            "outcome_unknown",
            "Output writer lease changed before publication.",
        )
        .stage("storage"));
    }
    match store.persistence.link(&temporary.0, &target) {
        Ok(()) => {}
        Err(e) if stable && e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(error()),
    }
    store
        .persistence
        .unlink(&temporary.0)
        .map_err(|_| error())?;
    #[cfg(unix)]
    fs::File::open(&directory)
        .and_then(|f| f.sync_all())
        .map_err(|_| error())?;
    let resolved = fs::canonicalize(&target).map_err(|_| error())?;
    if !same_physical_path(&resolved, &target) || !resolved.starts_with(&parent) {
        return Err(NumeraError::new(
            "permission_denied",
            "Published image path changed during storage.",
        ));
    }
    let saved = read_image(&target, config).map_err(|_| error())?;
    if saved.sha256 != image.sha256 {
        return Err(
            NumeraError::new("output_file_error", "Saved image hash mismatch.").stage("storage"),
        );
    }
    let output = json!({"output_id":output_id,"request_id":id,"path":target.to_str().ok_or_else(error)?,
        "mime_type":saved.mime,"width":saved.width,"height":saved.height,"has_alpha":saved.alpha,"bytes":saved.bytes.len(),"sha256":saved.sha256});
    if stable {
        store
            .commit(id, index, &output, deviations, expected)
            .map_err(|_| error())?;
    } else {
        store.add_output(&output).map_err(|_| error())?;
    }
    Ok(output)
}

fn same_physical_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        let normalize = |path: &Path| {
            let text = path.to_string_lossy();
            if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
                format!("\\\\{unc}")
            } else {
                text.strip_prefix("\\\\?\\").unwrap_or(&text).to_owned()
            }
        };
        normalize(left) == normalize(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn read_image(path: &Path, config: &Value) -> Result<Image> {
    let limit = config["files"]["maxOutputBytes"]
        .as_u64()
        .unwrap_or(50 * 1024 * 1024);
    let mut file = fs::File::open(path)
        .map_err(|_| NumeraError::new("output_file_error", "Owned output file is missing."))?;
    let metadata = file.metadata().map_err(|_| NumeraError::storage())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(NumeraError::new(
            "output_file_error",
            "Owned output exceeds bounded regular-file limits.",
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NumeraError::storage())?;
    inspect(bytes, config, None)
}

pub fn owned(config: &Value, store: &Store, id: &str) -> Result<(Value, Image)> {
    let output = store.output(id)?;
    let root = fs::canonicalize(
        config["outputDir"]
            .as_str()
            .ok_or_else(NumeraError::storage)?,
    )
    .map_err(|_| NumeraError::storage())?;
    let path = fs::canonicalize(output["path"].as_str().ok_or_else(NumeraError::storage)?)
        .map_err(|_| NumeraError::new("output_file_error", "Owned output file is missing."))?;
    if !path.starts_with(root) {
        return Err(NumeraError::new(
            "permission_denied",
            "Owned output path escapes its root.",
        ));
    }
    let image = read_image(&path, config)?;
    if Some(image.sha256.as_str()) != output["sha256"].as_str() {
        return Err(NumeraError::new(
            "output_file_error",
            "Owned output bytes changed after storage.",
        ));
    }
    Ok((output, image))
}
