#[cfg(unix)]
use super::process_unix::Owned;
#[cfg(windows)]
use super::process_windows::Owned;
use crate::{
    compat::value::{Value, json},
    logging::Logger,
    runtime_paths::RuntimePaths,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub struct Bounds {
    wall: Duration,
    idle: Duration,
}
impl Bounds {
    pub fn new(wall: u64, idle: u64) -> io::Result<Self> {
        if !(1..=3600000).contains(&wall) || !(1..=3600000).contains(&idle) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Positive wall and idle ceilings up to one hour are required.",
            ));
        }
        Ok(Self {
            wall: Duration::from_millis(wall),
            idle: Duration::from_millis(idle),
        })
    }
}
pub struct Outcome {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub reason: Option<String>,
    pub cleanup_verified: bool,
}

struct Ledger {
    file: File,
    started: String,
    run_id: String,
}
impl Ledger {
    fn record(&mut self, event: Value) -> io::Result<()> {
        let mut event = event;
        event["started"] = json!(self.started);
        event["run_id"] = json!(self.run_id);
        let mut line = serde_json::to_vec(&event)
            .map_err(|_| io::Error::other("Cannot encode process evidence."))?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.flush()
    }
}

pub fn run(
    command: &mut Command,
    root: &Path,
    bounds: Bounds,
    cancelled: &AtomicBool,
    logger: &Logger,
) -> io::Result<Outcome> {
    let paths = RuntimePaths::new(root)
        .map_err(|_| io::Error::other("Cannot resolve contained command scratch."))?;
    let work = paths.root.join(".ci-work");
    paths
        .verify(&work)
        .map_err(|_| io::Error::other("Process ledger escapes owning root."))?;
    crate::runtime_paths::private_directory(&work)
        .map_err(|_| io::Error::other("Cannot create private process ledger directory."))?;
    let ledger_path = work.join("processes.jsonl");
    paths
        .verify(&ledger_path)
        .map_err(|_| io::Error::other("Process ledger escapes owning root."))?;
    if let Ok(info) = fs::symlink_metadata(&ledger_path)
        && (!info.is_file() || info.is_symlink() || info.len() > 16 * 1024 * 1024)
    {
        return Err(io::Error::other(
            "Process ledger is not a bounded regular file.",
        ));
    }
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(&ledger_path)?;
    let info = file.metadata()?;
    if !info.is_file() || info.len() > 16 * 1024 * 1024 {
        return Err(io::Error::other("Process ledger changed during opening."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let current = fs::symlink_metadata(&ledger_path)?;
        if info.nlink() != 1 || current.dev() != info.dev() || current.ino() != info.ino() {
            return Err(io::Error::other("Process ledger identity is unsafe."));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0 || crate::files::windows_identity::links(&file)? != 1
        {
            return Err(io::Error::other("Process ledger identity is unsafe."));
        }
    }
    let mut ledger = Ledger {
        file,
        started: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        run_id: uuid::Uuid::new_v4().to_string(),
    };
    command
        .current_dir(&paths.root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in paths.child_environment() {
        command.env(key, value);
    }
    let begin = Instant::now();
    let mut child = Owned::spawn(command)?;
    let pid = child.id();
    let mut captures = child.capture()?;
    let operation = (|| -> io::Result<Option<String>> {
        ledger.record(
            json!({"event":"start","pid":pid,"owner_pid":std::process::id(),
            "wall_ms":bounds.wall.as_millis() as u64,"idle_ms":bounds.idle.as_millis() as u64}),
        )?;
        #[cfg(windows)]
        let started = child.diagnostics();
        #[cfg(unix)]
        let started = json!({"pid":pid});
        logger.log("DEBUG", "bounded", "Bounded child started.", started);
        let mut progress = begin;
        loop {
            if captures.poll()? {
                progress = Instant::now();
            }
            let reason = if cancelled.load(Ordering::Acquire) {
                Some("cancelled")
            } else if captures.exceeded {
                Some("output_limit")
            } else if begin.elapsed() >= bounds.wall {
                Some("wall")
            } else if progress.elapsed() >= bounds.idle {
                Some("idle")
            } else {
                None
            };
            if let Some(reason) = reason {
                ledger.record(json!({"event":"terminate","pid":pid,"reason":reason}))?;
                #[cfg(windows)]
                let mut diagnostic = child.diagnostics();
                #[cfg(unix)]
                let mut diagnostic = json!({"pid":pid});
                diagnostic["reason"] = json!(reason);
                diagnostic["elapsed_ms"] = json!(begin.elapsed().as_millis() as u64);
                diagnostic["idle_ms"] = json!(progress.elapsed().as_millis() as u64);
                diagnostic["stdout_bytes"] = json!(captures.stdout_bytes.len());
                diagnostic["stderr_bytes"] = json!(captures.stderr_bytes.len());
                #[cfg(windows)]
                if matches!(reason, "idle" | "wall")
                    && let Some(fields) = child.thread_snapshot().as_object()
                {
                    for (key, value) in fields {
                        diagnostic[key] = value.clone();
                    }
                }
                logger.log(
                    "ERROR",
                    "bounded",
                    "Bounded command terminated.",
                    diagnostic,
                );
                return Ok(Some(reason.into()));
            }
            if child.exited()? {
                return Ok(None);
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    })();
    let stopped = child.stop(Instant::now() + Duration::from_secs(2));
    let verified = stopped.is_ok();
    let drain_until = Instant::now() + Duration::from_millis(100);
    let drained = (|| -> io::Result<()> {
        while !captures.exceeded && captures.poll()? {
            if Instant::now() >= drain_until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Owned output drain exceeded its ceiling.",
                ));
            }
        }
        Ok(())
    })();
    let mut operation = operation.and_then(|reason| drained.map(|()| reason));
    if operation.as_ref().is_ok_and(|reason| reason.is_none()) && captures.exceeded {
        ledger.record(json!({"event":"terminate","pid":pid,"reason":"output_limit"}))?;
        operation = Ok(Some("output_limit".into()));
    }
    let code = match (&operation, &stopped) {
        (_, Err(_)) | (Err(_), _) => 1,
        (Ok(Some(_)), Ok(_)) => 124,
        (Ok(None), Ok(code)) => *code,
    };
    ledger.record(
        json!({"event":"finish","pid":pid,"exit_code":code,"cleanup_verified":verified,
        "finished":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}),
    )?;
    logger.log(
        if code == 0 { "INFO" } else { "ERROR" },
        "bounded",
        "Bounded child finished.",
        json!({"pid":pid,"exit_code":code,"cleanup_verified":verified}),
    );
    stopped?;
    let reason = operation?;
    Ok(Outcome {
        code,
        stdout: captures.stdout_bytes,
        stderr: captures.stderr_bytes,
        reason,
        cleanup_verified: verified,
    })
}
