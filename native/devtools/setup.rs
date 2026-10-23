use crate::{
    cli::{readiness, registration},
    compat::value::{Value, json},
    error::{NumeraError, Result},
    logging::Logger,
    runtime_paths::RuntimePaths,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

pub struct Report {
    pub dry_run: bool,
    pub value: Value,
}
struct Options {
    action: String,
    root: PathBuf,
    values: HashMap<String, String>,
    dry: bool,
}
fn invalid() -> NumeraError {
    NumeraError::new(
        "INVALID_REQUEST",
        "Use one supported setup action with explicit absolute paths and applicable options.",
    )
}
fn parse(args: &[String]) -> Result<Options> {
    let action = args
        .first()
        .filter(|action| {
            ["Install", "Update", "Diagnose", "Register", "Remove"].contains(&action.as_str())
        })
        .ok_or_else(invalid)?;
    let mut values: HashMap<String, String> = HashMap::new();
    let mut dry = false;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].strip_prefix("--").ok_or_else(invalid)?;
        if flag == "dry-run" {
            if dry {
                return Err(invalid());
            }
            dry = true;
        } else {
            if !["root", "artifact", "config", "client", "host-config"].contains(&flag) {
                return Err(invalid());
            }
            index += 1;
            let value = args
                .get(index)
                .filter(|value| !value.is_empty() && !value.starts_with("--"))
                .ok_or_else(invalid)?;
            if values.insert(flag.into(), value.clone()).is_some() {
                return Err(invalid());
            }
        }
        index += 1;
    }
    let allowed: &[&str] = match action.as_str() {
        "Install" | "Update" => &["root", "artifact"],
        "Diagnose" => &["root", "config"],
        _ => &["root", "config", "client", "host-config"],
    };
    if values.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid());
    }
    for key in allowed {
        if !values.contains_key(*key) {
            return Err(invalid());
        }
        if *key != "client" && !Path::new(&values[*key]).is_absolute() {
            return Err(invalid());
        }
    }
    if let Some(client) = values.get("client")
        && !["ClaudeDesktop", "ClaudeProject", "Codex"].contains(&client.as_str())
    {
        return Err(invalid());
    }
    let root = fs::canonicalize(&values["root"]).map_err(|_| invalid())?;
    if !root.is_dir() {
        return Err(invalid());
    }
    if let Some(path) = values.get("artifact") {
        let info = fs::symlink_metadata(path).map_err(|_| invalid())?;
        if !info.is_dir() || info.is_symlink() {
            return Err(invalid());
        }
    }
    if let Some(path) = values.get("config") {
        let info = fs::symlink_metadata(path).map_err(|_| invalid())?;
        if !info.is_file() || info.is_symlink() || info.len() > 16 * 1024 * 1024 {
            return Err(invalid());
        }
    }
    Ok(Options {
        action: action.clone(),
        root,
        values,
        dry,
    })
}

pub fn execute(
    args: &[String],
    executable: &Path,
    env: &HashMap<String, String>,
) -> Result<Report> {
    let options = parse(args)?;
    if !executable.is_absolute() {
        return Err(invalid());
    }
    let logger = Logger::new(&options.root.join("logs"), "INFO");
    logger.log(
        "INFO",
        "setup",
        "Native setup operation started.",
        json!({"action":options.action,"dry_run":options.dry}),
    );
    let result = (|| {
        if options.dry {
            if matches!(options.action.as_str(), "Register" | "Remove") {
                registration::register(
                    &options.values["client"],
                    options.action == "Remove",
                    Path::new(&options.values["host-config"]),
                    executable,
                    Path::new(&options.values["config"]),
                    true,
                    &logger,
                )?;
            }
            return Ok(Report {
                dry_run: true,
                value: json!({"action":options.action,"dry_run":true,"changed":false}),
            });
        }
        match options.action.as_str() {
            "Register" | "Remove" => {
                if options.action == "Register" {
                    let installed = super::artifact::verify(&options.root)?;
                    let expected = options.root.join("bin").join(if cfg!(windows) {
                        "numera-image-gen-mcp.exe"
                    } else {
                        "numera-image-gen-mcp"
                    });
                    if fs::canonicalize(executable).ok() != fs::canonicalize(&expected).ok()
                        || !expected.is_file()
                        || installed.manifest.publisher != "KiaroSama"
                    {
                        return Err(invalid());
                    }
                }
                registration::register(
                    &options.values["client"],
                    options.action == "Remove",
                    Path::new(&options.values["host-config"]),
                    executable,
                    Path::new(&options.values["config"]),
                    false,
                    &logger,
                )?;
                Ok(Report {
                    dry_run: false,
                    value: json!({"action":options.action,"dry_run":false}),
                })
            }
            "Diagnose" => {
                let paths = RuntimePaths::new(&options.root)?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| NumeraError::runtime())?;
                let (value, ready) = runtime.block_on(readiness::run(
                    &["--config".into(), options.values["config"].clone()],
                    &options.root,
                    env,
                    &paths,
                    executable,
                ))?;
                if !ready {
                    return Err(NumeraError::new(
                        "readiness_blocked",
                        "Read-only readiness has unresolved blockers.",
                    ));
                }
                Ok(Report {
                    dry_run: false,
                    value,
                })
            }
            "Install" | "Update" => {
                let artifact = super::artifact::verify(Path::new(&options.values["artifact"]))?;
                super::installation::install(&artifact, &options.root, options.action == "Update")?;
                Ok(Report {
                    dry_run: false,
                    value: json!({"action":options.action,"dry_run":false,"installed":true}),
                })
            }
            _ => Err(invalid()),
        }
    })();
    logger.log(
        if result.is_ok() { "INFO" } else { "ERROR" },
        "setup",
        "Native setup operation finished.",
        json!({"action":options.action,"success":result.is_ok()}),
    );
    result
}
