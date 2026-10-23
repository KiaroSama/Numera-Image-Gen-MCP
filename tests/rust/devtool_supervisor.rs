mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    devtools::supervisor::{Bounds, run},
    logging::Logger,
};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < until => {
                    std::thread::park_timeout(Duration::from_millis(10))
                }
                _ => break,
            }
        }
        if std::thread::panicking() {
            eprintln!("Native fixture child cleanup is unverified.");
        } else {
            panic!("Native fixture child cleanup is unverified.");
        }
    }
}

fn command(root: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "fixture_worker", "--nocapture"])
        .env("NUMERA_DEV_FIXTURE", mode)
        .env("NUMERA_DEV_FIXTURE_ROOT", root)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn record_leaf(root: &Path, child: &Child) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let created = creation_time(child.as_raw_handle()).unwrap();
        fs::write(root.join("leaf-created"), created.to_string()).unwrap();
    }
    fs::write(root.join("leaf-pid"), child.id().to_string()).unwrap();
}

#[cfg(windows)]
fn creation_time(handle: *mut std::ffi::c_void) -> io::Result<u64> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcessTimes(
            handle: *mut std::ffi::c_void,
            created: *mut u64,
            exited: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
    }
    let (mut created, mut exited, mut kernel, mut user) = (0, 0, 0, 0);
    // SAFETY: live query-capable process handle and four correctly sized FILETIME outputs.
    if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(created)
}

fn wait_ready(path: &Path) {
    let until = Instant::now() + Duration::from_secs(2);
    while !path.exists() {
        assert!(
            Instant::now() < until,
            "Native fixture readiness timed out."
        );
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

#[test]
fn fixture_worker() {
    let Ok(mode) = std::env::var("NUMERA_DEV_FIXTURE") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("NUMERA_DEV_FIXTURE_ROOT").unwrap());
    match mode.as_str() {
        "tree" => {
            let mut child = Guard(
                command(&root, "leaf")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            wait_ready(&root.join("leaf-ready"));
            record_leaf(&root, &child.0);
            println!("fixture-ready");
            io::stdout().flush().unwrap();
            let until = Instant::now() + Duration::from_secs(6);
            while Instant::now() < until && child.0.try_wait().unwrap().is_none() {
                std::thread::park_timeout(Duration::from_millis(10));
            }
        }
        "leaf" | "unrelated" => {
            fs::write(root.join(format!("{mode}-ready")), b"ready").unwrap();
            let until = Instant::now() + Duration::from_secs(6);
            while Instant::now() < until {
                std::thread::park_timeout(Duration::from_millis(10));
            }
        }
        "exit" => std::process::exit(37),
        "orphan" => {
            let child = command(&root, "leaf")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let child = Guard(child);
            wait_ready(&root.join("leaf-ready"));
            record_leaf(&root, &child.0);
            // Exit bypasses Drop; the outer supervisor still owns the live descendant.
            std::process::exit(37);
        }
        "output" => {
            io::stdout()
                .write_all(&vec![b'x'; 3 * 1024 * 1024])
                .unwrap();
        }
        _ => panic!("Unknown native fixture mode."),
    }
}

#[test]
fn idle_timeout_reaps_owned_tree_preserves_unrelated_and_records_cleanup() {
    let workspace = Workspace::new();
    let root = &workspace.0;
    let mut unrelated = Guard(
        command(root, "unrelated")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_ready(&root.join("unrelated-ready"));
    let logger = Logger::new(&root.join("logs"), "DEBUG");
    let result = run(
        &mut command(root, "tree"),
        root,
        Bounds::new(5000, 500).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(result.code, 124);
    assert_eq!(result.reason.as_deref(), Some("idle"));
    assert!(result.cleanup_verified);
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("fixture-ready")
    );
    assert!(unrelated.0.try_wait().unwrap().is_none());
    assert_descendant_stopped(root);
    let events = fs::read_to_string(root.join(".ci-work/processes.jsonl")).unwrap();
    let rows = events
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        rows.iter()
            .any(|row| row["event"] == "start" && row["wall_ms"] == 5000 && row["idle_ms"] == 500)
    );
    assert!(
        rows.iter()
            .any(|row| row["event"] == "terminate" && row["reason"] == "idle")
    );
    assert!(rows.iter().any(|row| row["event"] == "finish"
        && row["exit_code"] == 124
        && row["cleanup_verified"] == true));
    assert!(!events.contains("NUMERA_DEV_FIXTURE"));
    assert!(!events.contains(&root.to_string_lossy().into_owned()));
}

fn assert_descendant_stopped(root: &Path) {
    let pid: u32 = fs::read_to_string(root.join("leaf-pid"))
        .unwrap()
        .parse()
        .unwrap();
    #[cfg(unix)]
    {
        let status = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut status = Guard(status);
        let until = Instant::now() + Duration::from_secs(2);
        while status.0.try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < until,
                "Bounded process inspection timed out."
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
        let mut bytes = String::new();
        use std::io::Read;
        status
            .0
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut bytes)
            .unwrap();
        assert!(bytes.trim().is_empty() || bytes.trim().starts_with('Z'));
    }
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
            fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        // SAFETY: query-only handle; no PID-based termination, always closed.
        unsafe {
            let handle = OpenProcess(0x00101000, 0, pid);
            if !handle.is_null() {
                let created = creation_time(handle);
                let result = WaitForSingleObject(handle, 0);
                CloseHandle(handle);
                let expected: u64 = fs::read_to_string(root.join("leaf-created"))
                    .unwrap()
                    .parse()
                    .unwrap();
                if created.unwrap() == expected {
                    assert_eq!(result, 0, "The original owned descendant is still alive.");
                }
            } else {
                assert_eq!(io::Error::last_os_error().raw_os_error(), Some(87));
            }
        }
    }
}

#[test]
fn actual_native_bounded_cli_propagates_idle_timeout_and_verified_descendant_cleanup() {
    let workspace = Workspace::new();
    let logger = Logger::new(&workspace.0.join("logs"), "INFO");
    let executable = std::env::current_exe().unwrap();
    let mut cli = Command::new(env!("CARGO_BIN_EXE_numera-dev"));
    cli.args(["bounded", "5000", "500", "--exec"])
        .arg(executable)
        .args(["--exact", "fixture_worker", "--nocapture"])
        .env("NUMERA_DEV_FIXTURE", "tree")
        .env("NUMERA_DEV_FIXTURE_ROOT", &workspace.0);
    let result = run(
        &mut cli,
        &workspace.0,
        Bounds::new(12000, 5000).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(result.code, 124);
    assert!(result.reason.is_none());
    assert!(result.cleanup_verified);
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("fixture-ready")
    );
    assert_descendant_stopped(&workspace.0);
    #[cfg(unix)]
    {
        let pid: i32 = fs::read_to_string(workspace.0.join("leaf-pid"))
            .unwrap()
            .parse()
            .unwrap();
        // Frozen setup.test.ts requires PID absence, not merely a stopped or zombie child.
        assert!(pid > 0);
        // SAFETY: signal zero only observes this fixture's positive PID; it sends no signal.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }
    let rows = fs::read_to_string(workspace.0.join(".ci-work/processes.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let inner = rows
        .iter()
        .find(|row| row["event"] == "start" && row["idle_ms"] == 500)
        .unwrap();
    assert_eq!(inner["wall_ms"], 5000);
    assert!(rows.iter().any(|row| row["event"] == "terminate"
        && row["pid"] == inner["pid"]
        && row["reason"] == "idle"));
    assert!(rows.iter().any(|row| row["event"] == "finish"
        && row["pid"] == inner["pid"]
        && row["exit_code"] == 124
        && row["cleanup_verified"] == true));
}

#[test]
fn exited_leader_retains_status_and_cleans_its_live_descendant() {
    let workspace = Workspace::new();
    let logger = Logger::new(&workspace.0.join("logs"), "INFO");
    let outcome = run(
        &mut command(&workspace.0, "orphan"),
        &workspace.0,
        Bounds::new(5000, 2000).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(outcome.code, 37);
    assert!(outcome.reason.is_none());
    assert!(outcome.cleanup_verified);
    assert_descendant_stopped(&workspace.0);
}

#[test]
fn bounds_exit_wall_cancellation_and_output_limit_are_observable() {
    for (wall, idle) in [(0, 1), (1, 0), (3600001, 1), (1, 3600001)] {
        assert!(Bounds::new(wall, idle).is_err());
    }
    let workspace = Workspace::new();
    let logger = Logger::new(&workspace.0.join("logs"), "INFO");
    let normal = run(
        &mut command(&workspace.0, "exit"),
        &workspace.0,
        Bounds::new(5000, 2000).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(normal.code, 37);
    assert!(normal.cleanup_verified);
    let wall = run(
        &mut command(&workspace.0, "leaf"),
        &workspace.0,
        Bounds::new(500, 5000).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(wall.reason.as_deref(), Some("wall"));
    assert_eq!(wall.code, 124);
    assert!(wall.cleanup_verified);
    let output = run(
        &mut command(&workspace.0, "output"),
        &workspace.0,
        Bounds::new(5000, 2000).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(output.reason.as_deref(), Some("output_limit"));
    assert_eq!(output.code, 124);
    assert!(output.cleanup_verified);
    let cancel = run(
        &mut command(&workspace.0, "leaf"),
        &workspace.0,
        Bounds::new(5000, 2000).unwrap(),
        &AtomicBool::new(true),
        &logger,
    )
    .unwrap();
    assert_eq!(cancel.reason.as_deref(), Some("cancelled"));
    assert_eq!(cancel.code, 124);
    assert!(cancel.cleanup_verified);
}
