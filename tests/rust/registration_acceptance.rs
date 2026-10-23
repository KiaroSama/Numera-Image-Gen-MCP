mod common;

use common::Workspace;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

// Only the direct native registration command runs here; it has no helper descendants.
struct Owned {
    child: Child,
}

fn exited(child: &mut Child, deadline: Instant) -> std::io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        std::thread::park_timeout(remaining.min(Duration::from_millis(10)));
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.child.kill();
        let reaped = matches!(
            exited(&mut self.child, Instant::now() + Duration::from_secs(2)),
            Ok(Some(_))
        );
        if !reaped {
            let detail = format!(
                "Native registration child cleanup incomplete: pid={}; outer CI cleanup required.",
                self.child.id(),
            );
            if std::thread::panicking() {
                eprintln!("{detail}");
            } else {
                panic!("{detail}");
            }
        }
    }
}

struct Pending {
    owned: Owned,
    stdout: PathBuf,
    stderr: PathBuf,
    deadline: Instant,
}

struct Captured {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

impl Pending {
    fn finish(mut self) -> Captured {
        let mut last_size = 0;
        let mut progress = Instant::now();
        let status = loop {
            let size = fs::metadata(&self.stdout).unwrap().len()
                + fs::metadata(&self.stderr).unwrap().len();
            assert!(size <= 1048576, "registration capture exceeds one MiB");
            if size != last_size {
                last_size = size;
                progress = Instant::now();
            }
            if let Some(status) = self.owned.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < self.deadline,
                "registration child wall deadline"
            );
            assert!(
                progress.elapsed() < Duration::from_secs(6),
                "registration child idle deadline"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        };
        assert!(fs::metadata(&self.stdout).unwrap().len() <= 1048576);
        assert!(fs::metadata(&self.stderr).unwrap().len() <= 1048576);
        Captured {
            status,
            stdout: fs::read_to_string(&self.stdout).unwrap(),
            stderr: fs::read_to_string(&self.stderr).unwrap(),
        }
    }
}

struct Fixture {
    workspace: Workspace,
    executable: PathBuf,
    deadline: Instant,
    serial: Cell<usize>,
}

impl Fixture {
    fn new() -> Self {
        let mut workspace = Workspace::new();
        workspace.0 = workspace.0.components().collect();
        for directory in ["bin", "caller", "capture", "temp"] {
            fs::create_dir(workspace.0.join(directory)).unwrap();
        }
        let executable = workspace.0.join("bin").join(if cfg!(windows) {
            "numera-image-gen-mcp.exe"
        } else {
            "numera-image-gen-mcp"
        });
        fs::copy(env!("CARGO_BIN_EXE_numera-image-gen-mcp"), &executable).unwrap();
        Self {
            workspace,
            executable,
            deadline: Instant::now() + Duration::from_secs(20),
            serial: Cell::new(0),
        }
    }

    fn root(&self) -> &Path {
        &self.workspace.0
    }

    fn start(&self, client: &str, file: &Path, remove: bool, dry: bool) -> Pending {
        assert!(
            Instant::now() < self.deadline,
            "source twenty-second case ceiling"
        );
        let index = self.serial.get();
        self.serial.set(index + 1);
        let stdout = self.root().join(format!("capture/{index}.stdout"));
        let stderr = self.root().join(format!("capture/{index}.stderr"));
        let capture = |path: &Path| -> File {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap()
        };
        let mut command = Command::new(&self.executable);
        command
            .args(["register", "--client", client, "--host-config"])
            .arg(file)
            .arg("--config")
            .arg(self.root().join("config.json"))
            .current_dir(self.root().join("caller"))
            .env_clear()
            .env("LOCALAPPDATA", self.root())
            .env("XDG_CONFIG_HOME", self.root())
            .env("HOME", self.root())
            .env("USERPROFILE", self.root())
            .env("TEMP", self.root().join("temp"))
            .env("TMP", self.root().join("temp"))
            .env("TMPDIR", self.root().join("temp"))
            .stdin(Stdio::null())
            .stdout(Stdio::from(capture(&stdout)))
            .stderr(Stdio::from(capture(&stderr)));
        for key in ["SystemRoot", "WINDIR", "LLVM_PROFILE_FILE"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        if remove {
            command.arg("--remove");
        }
        if dry {
            command.arg("--dry-run");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let deadline =
            (Instant::now() + Duration::from_secs(12)).min(self.deadline - Duration::from_secs(2));
        let child = command.spawn().unwrap_or_else(|error| {
            panic!(
                "Cannot start registration child: kind={:?}, os_code={:?}",
                error.kind(),
                error.raw_os_error()
            )
        });
        Pending {
            owned: Owned { child },
            stdout,
            stderr,
            deadline,
        }
    }

    fn run(&self, client: &str, file: &Path, remove: bool, dry: bool) -> Captured {
        let result = self.start(client, file, remove, dry).finish();
        assert!(
            Instant::now() < self.deadline,
            "source twenty-second case ceiling"
        );
        assert!(!result.stderr.contains(self.root().to_str().unwrap()));
        result
    }

    fn names(&self) -> Vec<String> {
        fs::read_dir(self.root())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect()
    }
}

#[test]
fn exact_source_native_registration_dry_run_backup_rollback_remove_and_private_logs() {
    let fixture = Fixture::new();
    let file = fixture.root().join("host.json");
    let before =
        "{\"model\":\"unchanged 日本語\",\"mcpServers\":{\"other\":{\"command\":\"stay\"}}}\n";
    fs::write(&file, before.as_bytes()).unwrap();
    assert!(
        fixture
            .run("ClaudeDesktop", &file, false, true)
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    assert!(!fixture.names().iter().any(|name| name.contains("backup")));
    assert!(
        fixture
            .run("ClaudeDesktop", &file, false, false)
            .status
            .success()
    );
    let after = fs::read_to_string(&file).unwrap();
    let value: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(value["model"], "unchanged 日本語");
    assert_eq!(value["mcpServers"]["other"]["command"], "stay");
    assert_eq!(
        value["mcpServers"]["numera-image-gen"]["command"],
        json!(fixture.executable)
    );
    assert_eq!(
        value["mcpServers"]["numera-image-gen"]["args"],
        json!(["bridge"])
    );
    assert!(
        fixture
            .run("ClaudeDesktop", &file, false, false)
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), after);
    let backups = fixture
        .names()
        .into_iter()
        .filter(|name| name.contains("backup"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read_to_string(fixture.root().join(&backups[0])).unwrap(),
        before
    );
    fs::copy(fixture.root().join(&backups[0]), &file).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    assert!(
        fixture
            .run("ClaudeDesktop", &file, false, false)
            .status
            .success()
    );
    assert!(
        fixture
            .run("ClaudeDesktop", &file, true, false)
            .status
            .success()
    );
    assert_eq!(
        serde_json::from_str::<Value>(&fs::read_to_string(&file).unwrap()).unwrap(),
        serde_json::from_str::<Value>(before).unwrap()
    );
    assert!(
        !fixture
            .names()
            .iter()
            .any(|name| name.ends_with(".tmp") || name.ends_with(".numera-lock"))
    );
    let logs = fs::read_dir(fixture.root().join("logs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    let unique = logs.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique.len(), logs.len());
    assert!(!logs.is_empty());
    let pattern =
        regex::Regex::new(r"_\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_UTC-\d+-[\da-f-]+\.log$").unwrap();
    for log in logs {
        assert!(pattern.is_match(log.file_name().unwrap().to_str().unwrap()));
        let text = fs::read_to_string(log).unwrap();
        assert!(!text.contains(fixture.root().to_str().unwrap()));
        for line in text.trim().lines() {
            let event: Value = serde_json::from_str(line).unwrap();
            assert!(event["timestamp"].as_str().unwrap().ends_with('Z'));
            assert!(
                ["INFO", "WARNING", "ERROR", "DEBUG"].contains(&event["level"].as_str().unwrap())
            );
            assert!(!event["run_id"].as_str().unwrap().is_empty());
        }
    }
    assert!(Instant::now() < fixture.deadline);
}

#[test]
fn exact_source_native_toml_registration_preserves_crlf_and_refuses_five_ambiguities() {
    let fixture = Fixture::new();
    let file = fixture.root().join("host.toml");
    let before = "# retained\r\nmodel = \"unchanged\"\r\n[mcp_servers.other]\r\ncommand = \"existing\" # retained\r\n";
    fs::write(&file, before.as_bytes()).unwrap();
    assert!(fixture.run("Codex", &file, false, false).status.success());
    let after = fs::read_to_string(&file).unwrap();
    assert!(after.starts_with(before));
    assert!(fixture.run("Codex", &file, false, false).status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), after);
    assert!(fixture.run("Codex", &file, true, false).status.success());
    assert!(fs::read_to_string(&file).unwrap().starts_with(before));
    for ambiguous in [
        "[mcp_servers.\"numera-image-gen\"]\ncommand=\"other\"\n",
        "[mcp_servers.numera-image-gen]\ncommand=\"a\"\n[mcp_servers.numera-image-gen]\ncommand=\"b\"\n",
        "description = \"\"\"\n[mcp_servers.numera-image-gen]\n\"\"\"\n",
        "mcp_servers = { \"numera-image-gen\" = { command = \"other\" } }\n",
        "[mcp_servers]\nnumera-image-gen.command = \"other\"\n",
    ] {
        fs::write(&file, ambiguous.as_bytes()).unwrap();
        let result = fixture.run("Codex", &file, false, false);
        assert!(!result.status.success());
        assert_eq!(fs::read_to_string(&file).unwrap(), ambiguous);
        assert!(!result.stderr.contains(fixture.root().to_str().unwrap()));
    }
}

#[test]
fn exact_source_native_registration_concurrency_active_lock_and_invalid_json_never_partially_write()
{
    let fixture = Fixture::new();
    let file = fixture.root().join("host.json");
    fs::write(
        &file,
        b"{\"mcpServers\":{\"other\":{\"command\":\"stay\"}}}",
    )
    .unwrap();
    let first = fixture.start("ClaudeDesktop", &file, false, false);
    let second = fixture.start("ClaudeDesktop", &file, false, false);
    let first = first.finish();
    let second = second.finish();
    assert!(first.status.success() || second.status.success());
    let value: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["other"]["command"], "stay");
    let lock = fixture.root().join("host.json.numera-lock");
    fs::write(&lock, b"owned by another request").unwrap();
    let before = fs::read_to_string(&file).unwrap();
    assert!(
        !fixture
            .run("ClaudeDesktop", &file, false, false)
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    assert_eq!(
        fs::read_to_string(&lock).unwrap(),
        "owned by another request"
    );
    for invalid in ["{\"mcpServers\":null}", "{\"mcpServers\":[]}", "{malformed"] {
        let file = fixture.root().join("invalid.json");
        fs::write(&file, invalid.as_bytes()).unwrap();
        assert!(
            !fixture
                .run("ClaudeDesktop", &file, false, false)
                .status
                .success()
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), invalid);
        assert!(
            !fixture
                .names()
                .iter()
                .any(|name| name.starts_with("invalid.json.numera"))
        );
    }
    assert!(first.stdout.is_empty() && second.stdout.is_empty());
    assert!(Instant::now() < fixture.deadline);
}
