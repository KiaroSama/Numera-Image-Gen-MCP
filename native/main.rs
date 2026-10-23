use numera_image_gen_mcp::{
    compat::value::json,
    config::{self, ConfigReload},
    error::{NumeraError, Result},
    logging::Logger,
    mcp::{application::Application, handler::Handler, transport::Stdio},
    runtime_paths::RuntimePaths,
};
use rmcp::ServiceExt;
use std::{
    collections::HashMap,
    io::IsTerminal,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn installation_root(executable: &Path) -> Result<PathBuf> {
    let directory = executable.parent().ok_or_else(NumeraError::runtime)?;
    // Ready releases can live at the installation root or its explicit bin subdirectory.
    let root = if directory.file_name().is_some_and(|n| n == "bin") {
        directory.parent().ok_or_else(NumeraError::runtime)?
    } else {
        directory
    };
    std::fs::canonicalize(root).map_err(|_| {
        NumeraError::new(
            "invalid_configuration",
            "Cannot resolve the installed application root.",
        )
    })
}

fn application(
    args: Vec<String>,
    root: &Path,
    env: HashMap<String, String>,
    runtime: RuntimePaths,
) -> Result<Arc<Application>> {
    let file = config::config_file(&args, &env, root)?;
    let mut pinned = args;
    if let Some(file) = &file {
        pinned.extend(["--config".into(), file.to_string_lossy().into_owned()]);
    }
    let config = config::load_config(&pinned, &env, root)?;
    let logger = Arc::new(Logger::with_retention(
        Path::new(
            config.value["logging"]["directory"]
                .as_str()
                .ok_or_else(NumeraError::runtime)?,
        ),
        config.value["logging"]["level"].as_str().unwrap_or("INFO"),
        config.value["logging"]["retentionDays"]
            .as_u64()
            .unwrap_or(14),
    ));
    let reload = file.map(|file| ConfigReload::new(config.clone(), file, pinned, env.clone()));
    Ok(Arc::new(Application::new(
        config,
        reload,
        logger,
        runtime,
        env,
        CancellationToken::new(),
    )?))
}

async fn http(
    args: Vec<String>,
    root: &Path,
    env: HashMap<String, String>,
    runtime: RuntimePaths,
) -> Result<()> {
    use numera_image_gen_mcp::{cli::http_settings, mcp::http_server};
    let port = http_settings::port(root, &args, &env)?;
    let (_, args) = http_settings::split_port_args(&args)?;
    let token = http_settings::token(root, &env)?;
    let application = application(args, root, env, runtime)?;
    let stop = application.stop.clone();
    let listener = http_server::bind(port).await?;
    eprintln!("Local MCP HTTP: http://127.0.0.1:{port}/mcp (authentication required).");
    let service = http_server::serve(
        listener,
        application,
        http_server::Options::new(port, token),
        stop.clone(),
    );
    tokio::pin!(service);
    tokio::select! {
        result = service.as_mut() => result,
        _ = http_server::shutdown_signal() => {stop.cancel(); service.await}
    }
}

async fn menu(root: &Path, env: &HashMap<String, String>, runtime: &RuntimePaths) -> Result<()> {
    use numera_image_gen_mcp::cli::menu::{MenuAction, step};
    let logger = Logger::new(&root.join("logs"), "INFO");
    loop {
        let action = step(
            root,
            env,
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
        )?;
        logger.log(
            "INFO",
            "menu",
            "Native menu action selected.",
            json!({"action":format!("{action:?}")}),
        );
        match action {
            MenuAction::Exit => return Ok(()),
            MenuAction::Continue => {}
            MenuAction::StartHttp { port } => {
                http(
                    vec!["--port".into(), port.to_string()],
                    root,
                    env.clone(),
                    runtime.clone(),
                )
                .await?;
            }
        }
    }
}

async fn bridge(
    args: Vec<String>,
    root: PathBuf,
    env: HashMap<String, String>,
    runtime: RuntimePaths,
) -> Result<()> {
    let application = application(args, &root, env, runtime)?;
    let logger = application.logger.clone();
    let stop = application.stop.clone();
    let handler = Handler::new(application.clone())?;
    let transport = Stdio::new(128 * 1024 * 1024, stop.clone());
    let service = handler
        .serve_with_ct(transport, stop.clone())
        .await
        .map_err(|_| {
            NumeraError::new("invalid_input", "MCP initialization failed.").stage("protocol")
        })?;
    logger.log(
        "INFO",
        "MCP",
        "Stdio server ready.",
        json!({"runtime":"Rust","version":env!("CARGO_PKG_VERSION")}),
    );
    let cancellation = service.cancellation_token();
    let mut waiting = Box::pin(service.waiting());
    tokio::select! {
        result=&mut waiting=>{result.map_err(|_|NumeraError::runtime())?;},
        _=tokio::signal::ctrl_c()=>{
            application.shutdown();cancellation.cancel();
            tokio::time::timeout(Duration::from_secs(8),&mut waiting).await.map_err(|_|NumeraError::new("request_timeout","MCP shutdown deadline exceeded."))?.map_err(|_|NumeraError::runtime())?;
        }
    }
    application.shutdown();
    logger.log("INFO", "MCP", "Server stopped.", json!({}));
    Ok(())
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if matches!(
        args.first().map(String::as_str),
        Some("--version" | "version")
    ) {
        println!(
            "{}",
            numera_image_gen_mcp::build_identity::version("numera-image-gen-mcp")
        );
        return;
    }
    if matches!(args.first().map(String::as_str), Some("--help" | "help")) {
        println!(
            "Numera Image-Gen MCP\nUsage: numera-image-gen-mcp bridge [--config ABSOLUTE_PATH]\n       numera-image-gen-mcp http [--port 1969] [--config ABSOLUTE_PATH]\n       numera-image-gen-mcp menu\nHTTP listens only on 127.0.0.1 at /mcp; a separate private local token is required.\nClaude launches bridge directly; no launcher or background service is required.\nConfiguration: adjacent .env/config.local.json, explicit --config or NUMERA_CONFIG.\nManaged temporary files and cache stay inside the installation root."
        );
        return;
    }
    let outcome = (|| {
        let executable = std::env::current_exe().map_err(|_| NumeraError::runtime())?;
        let root = installation_root(&executable)?;
        let runtime = RuntimePaths::new(&root)?;
        let mut env = std::env::vars().collect::<HashMap<_, _>>();
        for (key, value) in runtime.child_environment() {
            env.insert(key, value);
        }
        if args.first().is_some_and(|s| s == "register") {
            return numera_image_gen_mcp::cli::maintenance::register(
                &args[1..],
                &root,
                &executable,
            );
        }
        // No target compiler/build/install or helper subprocess is required in production.
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| NumeraError::runtime())?;
        if args
            .first()
            .is_some_and(|s| s == "readiness" || s == "diagnose")
        {
            let (report, ready) = executor.block_on(numera_image_gen_mcp::cli::readiness::run(
                &args[1..],
                &root,
                &env,
                &runtime,
                &executable,
            ))?;
            println!("{report}");
            return if ready {
                Ok(())
            } else {
                Err(NumeraError::new(
                    "readiness_blocked",
                    "Read-only readiness has unresolved blockers.",
                ))
            };
        }
        if args.first().is_some_and(|s| s == "http") {
            return executor.block_on(http(args[1..].to_vec(), &root, env, runtime));
        }
        if args.first().is_some_and(|s| s == "menu")
            || (args.is_empty() && std::io::stdin().is_terminal())
        {
            return executor.block_on(menu(&root, &env, &runtime));
        }
        let args = if args.first().is_some_and(|s| s == "bridge") {
            args[1..].to_vec()
        } else {
            args
        };
        executor.block_on(bridge(args, root, env, runtime))
    })();
    if let Err(error) = outcome {
        eprintln!(
            "{}",
            json!({"level":"ERROR","component":"startup","message":"Server cannot start.","error":error})
        );
        std::process::exit(if error.code == "readiness_blocked" {
            2
        } else {
            1
        });
    }
}
