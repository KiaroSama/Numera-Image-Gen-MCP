use numera_image_gen_mcp::{
    devtools::supervisor::{Bounds, run},
    logging::Logger,
};
use std::{
    io::{self, Write},
    process::{Command, ExitCode},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn execute() -> io::Result<i32> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--help") {
        println!(
            "Numera native developer tool\nUsage: numera-dev bounded WALL_MS IDLE_MS --exec PROGRAM [ARGS...]\n       numera-dev setup ACTION --root ABSOLUTE_ROOT [--dry-run] [OPTIONS]\nActions: Install/Update --artifact ABSOLUTE_PREPARED_DIRECTORY\n         Diagnose --config ABSOLUTE_CONFIG\n         Register/Remove --client ClaudeDesktop|ClaudeProject|Codex --host-config ABSOLUTE_HOST --config ABSOLUTE_CONFIG\nRuns owned commands with bounded process-tree cleanup.\nNo shell evaluation, local compilation, runtime installation or implicit host selection."
        );
        return Ok(0);
    }
    if args.first().is_some_and(|arg| arg == "--version") {
        println!(
            "{}",
            numera_image_gen_mcp::build_identity::version("numera-dev")
        );
        return Ok(0);
    }
    if args.first().is_some_and(|arg| arg == "setup") {
        let values = args[1..]
            .iter()
            .map(|value| {
                value.to_str().map(str::to_owned).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Setup options must be valid Unicode.",
                    )
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let root = values
            .windows(2)
            .find(|pair| pair[0] == "--root")
            .map(|pair| std::path::PathBuf::from(&pair[1]))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Explicit installation root is required.",
                )
            })?;
        let root = root.canonicalize()?;
        let executable = root.join("bin").join(if cfg!(windows) {
            "numera-image-gen-mcp.exe"
        } else {
            "numera-image-gen-mcp"
        });
        let report = numera_image_gen_mcp::devtools::setup::execute(
            &values,
            &executable,
            &std::env::vars().collect(),
        )
        .map_err(|_| io::Error::other("Native setup failed; inspect the redacted log."))?;
        println!("{}", report.value);
        return Ok(0);
    }
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected bounded WALL_MS IDLE_MS --exec PROGRAM [ARGS...].",
        )
    };
    if args.len() < 5 || args[0] != "bounded" || args[3] != "--exec" {
        return Err(invalid());
    }
    let parse = |value: &std::ffi::OsStr| -> io::Result<u64> {
        let value = value.to_str().ok_or_else(invalid)?;
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }
        value.parse().map_err(|_| invalid())
    };
    let bounds = Bounds::new(parse(&args[1])?, parse(&args[2])?)?;
    let root = std::env::current_dir()?.canonicalize()?;
    let logger = Logger::new(&root.join("logs"), "DEBUG");
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let listener = runtime.spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal as unix_signal};
            let mut terminate = unix_signal(SignalKind::terminate())?;
            tokio::select! { result = tokio::signal::ctrl_c() => result?, _ = terminate.recv() => {} }
        }
        #[cfg(windows)]
        tokio::signal::ctrl_c().await?;
        signal.store(true, Ordering::Release);
        Ok::<(), io::Error>(())
    });
    let outcome = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            run(
                Command::new(&args[4]).args(&args[5..]),
                &root,
                bounds,
                &stop,
                &logger,
            )
        });
        runtime.block_on(async {
            while !worker.is_finished() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        });
        worker
            .join()
            .map_err(|_| io::Error::other("Owned command supervisor failed."))?
    });
    listener.abort();
    let _ = runtime.block_on(listener);
    let outcome = outcome?;
    io::stdout().write_all(&outcome.stdout)?;
    io::stderr().write_all(&outcome.stderr)?;
    Ok(outcome.code)
}

fn main() -> ExitCode {
    match execute() {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            eprintln!(
                "Native developer operation failed: kind={:?}, os_code={:?}. No private arguments are logged.",
                error.kind(),
                error.raw_os_error()
            );
            ExitCode::FAILURE
        }
    }
}
