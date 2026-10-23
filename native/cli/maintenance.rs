use crate::{
    error::{NumeraError, Result},
    logging::Logger,
};
use std::{collections::HashMap, path::Path};

fn invalid() -> NumeraError {
    NumeraError::new(
        "INVALID_REQUEST",
        "Pass explicit supported maintenance options and absolute paths.",
    )
}
pub(super) fn flags(
    args: &[String],
    booleans: &[&str],
    strings: &[&str],
) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].strip_prefix("--").ok_or_else(invalid)?;
        if booleans.contains(&name) {
            result.insert(name.into(), "true".into());
        } else if strings.contains(&name) {
            index += 1;
            let value = args
                .get(index)
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(invalid)?;
            result.insert(name.into(), value.clone());
        } else {
            return Err(invalid());
        }
        index += 1;
    }
    Ok(result)
}

pub fn register(args: &[String], root: &Path, executable: &Path) -> Result<()> {
    let f = flags(
        args,
        &["dry-run", "remove"],
        &["client", "host-config", "config"],
    )?;
    let client = f.get("client").ok_or_else(invalid)?;
    let host = Path::new(f.get("host-config").ok_or_else(invalid)?);
    let config = Path::new(f.get("config").ok_or_else(invalid)?);
    let logger = Logger::new(&root.join("logs"), "INFO");
    super::registration::register(
        client,
        f.contains_key("remove"),
        host,
        executable,
        config,
        f.contains_key("dry-run"),
        &logger,
    )
}
