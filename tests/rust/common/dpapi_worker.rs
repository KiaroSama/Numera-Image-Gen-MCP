use std::{
    io::{self, Write},
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub fn diagnostic_summary(output: &str) -> serde_json::Value {
    let mut events = Vec::new();
    for line in output.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value["component"] != "bounded" {
            continue;
        }
        let mut fields = serde_json::Map::new();
        for key in [
            "pid",
            "startup_ms",
            "resumed_threads",
            "running_threads",
            "kernel_ms",
            "user_ms",
            "elapsed_ms",
            "idle_ms",
            "stdout_bytes",
            "stderr_bytes",
            "observed_threads",
            "suspended_threads",
            "max_suspend_count",
            "snapshot_error",
        ] {
            if value[key].is_u64() {
                fields.insert(key.into(), value[key].clone());
            }
        }
        for key in ["child_running", "snapshot_cleanup_verified"] {
            if value[key].is_boolean() {
                fields.insert(key.into(), value[key].clone());
            }
        }
        if matches!(
            value["reason"].as_str(),
            Some("idle" | "wall" | "cancelled" | "output_limit")
        ) {
            fields.insert("reason".into(), value["reason"].clone());
        }
        if !fields.is_empty() {
            events.push(serde_json::Value::Object(fields));
        }
        if events.len() == 8 {
            break;
        }
    }
    serde_json::Value::Array(events)
}

#[test]
fn bounded_diagnostic_projection_never_forwards_private_fields_or_invalid_types() {
    let input = r#"{"component":"bounded","pid":7,"resumed_threads":2,"running_threads":1,"child_running":true,"kernel_ms":3,"reason":"idle","path":"private","user_ms":"private","message":"private","stderr_bytes":0}
{"component":"bounded","pid":"private","reason":"private","child_running":"private"}
{"component":"other","pid":9}"#;
    let actual = diagnostic_summary(input);
    let expected = serde_json::json!([{
        "pid":7,"resumed_threads":2,"running_threads":1,"child_running":true,
        "kernel_ms":3,"reason":"idle","stderr_bytes":0
    }]);
    assert_eq!(actual, expected);
    assert!(!actual.to_string().contains("private"));
    let snapshot = r#"{"component":"bounded","observed_threads":2,"suspended_threads":1,"max_suspend_count":1,"snapshot_error":5,"snapshot_cleanup_verified":false,"teb":"private"}"#;
    let expected = serde_json::json!([{
        "observed_threads":2,"suspended_threads":1,"max_suspend_count":1,
        "snapshot_error":5,"snapshot_cleanup_verified":false
    }]);
    assert_eq!(diagnostic_summary(snapshot), expected);
    assert_eq!(
        diagnostic_summary(&format!("{input}\n").repeat(20))
            .as_array()
            .unwrap()
            .len(),
        8
    );
}

struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let until = Instant::now() + Duration::from_secs(2);
        while !matches!(self.0.try_wait(), Ok(Some(_))) {
            if Instant::now() >= until {
                if std::thread::panicking() {
                    eprintln!("Native DPAPI descendant cleanup failed.");
                    return;
                }
                panic!("Native DPAPI descendant cleanup failed.");
            }
            pause(until);
        }
    }
}

fn pause(until: Instant) {
    std::thread::park_timeout(
        Duration::from_millis(10).min(until.saturating_duration_since(Instant::now())),
    );
}

fn descendant(mode: &str) -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", "windows::dpapi_script_worker", "--nocapture"])
        .env("NUMERA_FIXTURE_DPAPI_MODE", mode)
        .stdin(Stdio::null())
        .creation_flags(0x08000000);
    Ok(command)
}

pub fn run(mode: &str) -> io::Result<i32> {
    if mode == "leaf" {
        std::fs::write("native-descendant-ready", b"ready")?;
        let until = Instant::now() + Duration::from_secs(6);
        while Instant::now() < until {
            pause(until);
        }
        return Ok(0);
    }
    if mode == "branch" {
        let mut child = Guard(
            descendant("leaf")?
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let until = Instant::now() + Duration::from_secs(2);
        while !Path::new("native-descendant-ready").exists() {
            if child.0.try_wait()?.is_some() || Instant::now() >= until {
                return Err(io::Error::other("Native descendant readiness failed."));
            }
            pause(until);
        }
        println!("fixture-descendant-ready");
        io::stdout().flush()?;
        let until = Instant::now() + Duration::from_secs(6);
        while child.0.try_wait()?.is_none() && Instant::now() < until {
            pause(until);
        }
        return Ok(0);
    }
    let mut command = if mode == "hang" {
        descendant("branch")?
    } else {
        assert_eq!(mode, "script");
        let mut command = Command::new(env!("CARGO_BIN_EXE_numera-dev"));
        command
            .args([
                "bounded",
                "15000",
                "8000",
                "--exec",
                "pwsh",
                "-NoProfile",
                "-NonInteractive",
                "-File",
            ])
            .arg(std::env::var_os("NUMERA_FIXTURE_DPAPI_SCRIPT").unwrap());
        if std::env::var("NUMERA_FIXTURE_DPAPI_WRAPPER").unwrap() == "0" {
            command
                .arg("-Path")
                .arg(std::env::var_os("NUMERA_FIXTURE_DPAPI").unwrap());
        }
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .creation_flags(0x08000000);
    let mut child = Guard(command.spawn()?);
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.0.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if Instant::now() >= until {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Native DPAPI worker exceeded its bound.",
            ));
        }
        pause(until);
    }
}
