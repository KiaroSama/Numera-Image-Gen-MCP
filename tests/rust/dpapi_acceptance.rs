#[cfg(windows)]
mod common;
#[cfg(windows)]
#[path = "common/dpapi_worker.rs"]
mod dpapi_worker;

#[cfg(windows)]
mod windows {
    use super::common::Workspace;
    use numera_image_gen_mcp::{
        compat::value::{Value, json},
        config::{Config, credentials},
        runtime_paths::RuntimePaths,
    };
    use std::{
        collections::HashMap,
        ffi::c_void,
        fs::{self, File, OpenOptions},
        io,
        os::windows::{
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
            process::CommandExt,
        },
        path::{Path, PathBuf},
        process::{Child, Command, ExitStatus, Stdio},
        time::{Duration, Instant},
    };

    const PREFIX: &str = "non-secret-DPAPI-fixture";
    const FIXTURE: &str = "non-secret-DPAPI-fixture-日本語";
    const LIMIT: u64 = 1048576;
    type Handle = *mut c_void;

    fn stage_flags(output: &str) -> [bool; 13] {
        [
            "DPAPI_WORKER_ENROLLED",
            "Maintenance operation started.",
            "Bounded child started.",
            "User-scoped secret protection requested; value and path are not logged.",
            "DPAPI secret created for the current Windows user; value not logged.",
            "Secret protection failed. Verify destination, current-user DPAPI and access restrictions before use.",
            "ACL_VERIFIED",
            "Bounded child finished.",
            "\"cleanup_verified\":true",
            "\"cleanup_verified\":false",
            "DPAPI_WRAPPER_STARTED",
            "DPAPI_PARENT_SECURITY_READY",
            "DPAPI_PARENT_CREATED",
        ]
        .map(|marker| output.contains(marker))
    }

    #[test]
    fn timeout_stage_projection_retains_only_fixed_booleans() {
        let output = "private-path synthetic-plaintext ciphertext DPAPI_WORKER_ENROLLED\nBounded child started.\nACL_VERIFIED\n{\"cleanup_verified\":false}";
        assert_eq!(
            stage_flags(output)[..10],
            [
                true, false, true, false, false, false, true, false, false, true
            ]
        );
        let milestones = "DPAPI_WRAPPER_STARTED DPAPI_PARENT_SECURITY_READY DPAPI_PARENT_CREATED";
        assert_eq!(stage_flags(milestones)[10..], [true; 3]);
        assert_eq!(
            stage_flags("private-path plaintext ciphertext"),
            [false; 13]
        );
    }

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimits {
        process_time: i64,
        job_time: i64,
        flags: u32,
        minimum_working_set: usize,
        maximum_working_set: usize,
        active_limit: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: [u64; 6],
        memory: [usize; 4],
    }

    #[repr(C)]
    #[derive(Default)]
    struct Accounting {
        times: [i64; 4],
        page_faults: u32,
        total: u32,
        active: u32,
        terminated: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
        fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> Handle;
        fn GetCurrentProcess() -> Handle;
        fn GetLastError() -> u32;
        fn SetInformationJobObject(job: Handle, class: i32, data: *const c_void, size: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn QueryInformationJobObject(
            job: Handle,
            class: i32,
            data: *mut c_void,
            size: u32,
            returned: *mut u32,
        ) -> i32;
        fn TerminateJobObject(job: Handle, code: u32) -> i32;
    }

    fn handle(raw: Handle) -> io::Result<OwnedHandle> {
        if raw.is_null() {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: successful job APIs return one owned, non-pseudo handle.
            Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        assert!(!text.contains('\0'));
        text.encode_utf16().chain([0]).collect()
    }

    struct Job {
        handle: OwnedHandle,
        name: String,
    }

    impl Job {
        fn new() -> io::Result<Self> {
            let name = format!("Local\\Numera-DPAPI-fixture-{}", uuid::Uuid::new_v4());
            // SAFETY: UTF-16 name lives for the call; NULL attributes prohibit inheritance.
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), wide(&name).as_ptr()) };
            // SAFETY: capture the thread-local error before another API can replace it.
            let collision = unsafe { GetLastError() } == 183;
            let handle = handle(raw)?;
            if collision {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Fixture job already exists.",
                ));
            }
            let mut limits = ExtendedLimits::default();
            limits.basic.flags = 0x2000;
            // SAFETY: class9 receives the complete initialized extended-limit structure.
            if unsafe {
                SetInformationJobObject(
                    handle.as_raw_handle(),
                    9,
                    (&limits as *const ExtendedLimits).cast(),
                    std::mem::size_of::<ExtendedLimits>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { handle, name })
        }

        fn active(&self) -> io::Result<u32> {
            let mut accounting = Accounting::default();
            // SAFETY: class1 writes only into this correctly sized accounting structure.
            if unsafe {
                QueryInformationJobObject(
                    self.handle.as_raw_handle(),
                    1,
                    (&mut accounting as *mut Accounting).cast(),
                    std::mem::size_of::<Accounting>() as u32,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(accounting.active)
        }

        fn terminate(&self) -> io::Result<()> {
            // SAFETY: this handle belongs only to this fixture's non-breakaway job.
            if unsafe { TerminateJobObject(self.handle.as_raw_handle(), 1) } == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    fn join_job(name: &str) -> io::Result<()> {
        // SAFETY: exact parent-owned name; assign-process access, no handle inheritance.
        let job = handle(unsafe { OpenJobObjectW(1, 0, wide(name).as_ptr()) })?;
        // SAFETY: GetCurrentProcess is a borrowed pseudo handle, never closed or exposed.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), GetCurrentProcess()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn pause(deadline: Instant) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            std::thread::park_timeout(remaining.min(Duration::from_millis(10)));
        }
    }

    struct Owned {
        child: Child,
        job: Job,
        stdout: PathBuf,
        stderr: PathBuf,
        cleaned: bool,
    }

    impl Owned {
        fn stop(&mut self) -> io::Result<()> {
            // Stop the direct worker first: it must not join after the empty-job observation.
            let deadline = Instant::now() + Duration::from_secs(2);
            if self.child.try_wait()?.is_none() {
                self.child.kill()?;
            }
            while self.child.try_wait()?.is_none() {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Fixture worker did not stop before job cleanup.",
                    ));
                }
                pause(deadline);
            }
            self.job.terminate()?;
            loop {
                if self.job.active()? == 0 {
                    self.cleaned = true;
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Fixture job cleanup exceeded two seconds.",
                    ));
                }
                pause(deadline);
            }
        }

        fn capture_size(&self) -> io::Result<u64> {
            let size = fs::metadata(&self.stdout)?
                .len()
                .checked_add(fs::metadata(&self.stderr)?.len())
                .ok_or_else(|| io::Error::other("Fixture capture length overflow."))?;
            if size > LIMIT {
                return Err(io::Error::other("Fixture capture exceeded one MiB."));
            }
            Ok(size)
        }

        fn text(&self) -> io::Result<String> {
            self.capture_size()?;
            Ok(format!(
                "{}{}",
                fs::read_to_string(&self.stdout)?,
                fs::read_to_string(&self.stderr)?,
            ))
        }

        fn finish(mut self, deadline: Instant) -> io::Result<(ExitStatus, String)> {
            let mut last_size = self.capture_size()?;
            let mut progress = Instant::now();
            let status = loop {
                let size = self.capture_size()?;
                if size != last_size {
                    last_size = size;
                    progress = Instant::now();
                }
                if let Some(status) = self.child.try_wait()? {
                    break status;
                }
                let wall = Instant::now() >= deadline;
                let idle = progress.elapsed() >= Duration::from_secs(8);
                if wall || idle {
                    self.stop()?;
                    let output = self.text()?;
                    let stages = stage_flags(&output);
                    let diagnostic = super::dpapi_worker::diagnostic_summary(&output);
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "DPAPI timeout: wall={wall}, idle={idle}, stages={stages:?}, bounded_numeric_diagnostic={diagnostic}."
                        ),
                    ));
                }
                pause(deadline);
            };
            self.stop()?;
            Ok((status, self.text()?))
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.cleaned
                && let Err(error) = self.stop()
            {
                let detail = format!(
                    "DPAPI fixture cleanup unverified: kind={:?}, os_code={:?}.",
                    error.kind(),
                    error.raw_os_error(),
                );
                // Closing the last Job handle still terminates contained descendants on failure.
                if std::thread::panicking() {
                    eprintln!("{detail}");
                } else {
                    panic!("{detail}");
                }
            }
        }
    }

    fn start(
        root: &Path,
        mode: &str,
        script: Option<&Path>,
        destination: Option<&Path>,
        wrapper: bool,
    ) -> io::Result<Owned> {
        let job = Job::new()?;
        let nonce = uuid::Uuid::new_v4();
        let stdout = root.join(format!("{nonce}.stdout"));
        let stderr = root.join(format!("{nonce}.stderr"));
        let capture = |path: &Path| -> io::Result<File> {
            OpenOptions::new().write(true).create_new(true).open(path)
        };
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--exact", "windows::dpapi_script_worker", "--nocapture"])
            .current_dir(root)
            .env_clear()
            .env("NUMERA_FIXTURE_DPAPI_JOB", &job.name)
            .env("NUMERA_FIXTURE_DPAPI_MODE", mode)
            .env("NUMERA_LOG_DIR", root.join("logs"))
            .env("TEMP", root)
            .env("TMP", root)
            .env("TMPDIR", root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(capture(&stdout)?))
            .stderr(Stdio::from(capture(&stderr)?))
            .creation_flags(0x08000000);
        for key in [
            "PATH",
            "PATHEXT",
            "SystemRoot",
            "WINDIR",
            "USERPROFILE",
            "HOMEDRIVE",
            "HOMEPATH",
            "APPDATA",
            "LOCALAPPDATA",
            "PSModulePath",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        if let Some(script) = script {
            command.env("NUMERA_FIXTURE_DPAPI_SCRIPT", script);
        }
        if let Some(destination) = destination {
            command.env("NUMERA_FIXTURE_DPAPI", destination);
        }
        command.env(
            "NUMERA_FIXTURE_DPAPI_WRAPPER",
            if wrapper { "1" } else { "0" },
        );
        Ok(Owned {
            child: command.spawn()?,
            job,
            stdout,
            stderr,
            cleaned: false,
        })
    }

    #[test]
    fn dpapi_script_worker() {
        let Some(name) = std::env::var_os("NUMERA_FIXTURE_DPAPI_JOB") else {
            return;
        };
        let mode = std::env::var("NUMERA_FIXTURE_DPAPI_MODE").unwrap();
        if matches!(mode.as_str(), "script" | "hang") {
            // Descendants inherit this non-breakaway job; only the root enrolls itself.
            join_job(name.to_str().unwrap()).unwrap();
            eprintln!("DPAPI_WORKER_ENROLLED");
        }
        std::process::exit(super::dpapi_worker::run(&mode).unwrap());
    }

    fn wrapper_cleanup(root: &Path, offset: usize, status: &ExitStatus) {
        let path = root.join(".ci-work/processes.jsonl");
        assert!(fs::metadata(&path).unwrap().len() <= LIMIT);
        let text = fs::read_to_string(path).unwrap();
        let text = text.get(offset..).unwrap();
        assert!(text.ends_with('\n'));
        let rows = text
            .lines()
            .map(|line| Value::parse_with_limit(line, LIMIT as usize).unwrap())
            .collect::<Vec<_>>();
        let starts = rows
            .iter()
            .filter(|row| row["event"] == "start")
            .collect::<Vec<_>>();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0]["wall_ms"], 15000);
        assert_eq!(starts[0]["idle_ms"], 8000);
        let finishes = rows
            .iter()
            .filter(|row| {
                row["event"] == "finish"
                    && row["pid"] == starts[0]["pid"]
                    && row["started"] == starts[0]["started"]
            })
            .collect::<Vec<_>>();
        assert_eq!(finishes.len(), 1);
        assert_eq!(finishes[0]["cleanup_verified"], true);
        assert_eq!(
            finishes[0]["exit_code"].as_i64(),
            status.code().map(i64::from)
        );
    }

    fn run(
        root: &Path,
        script: &Path,
        destination: &Path,
        wrapper: bool,
        deadline: Instant,
    ) -> (ExitStatus, String) {
        let record = root.join(".ci-work/processes.jsonl");
        let offset = match fs::metadata(record) {
            Ok(metadata) => {
                assert!(metadata.is_file() && metadata.len() <= LIMIT);
                metadata.len() as usize
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => panic!("Cannot inspect fixture record: {:?}.", error.kind()),
        };
        let result = start(root, "script", Some(script), Some(destination), wrapper)
            .unwrap()
            .finish(deadline)
            .unwrap();
        wrapper_cleanup(root, offset, &result.0);
        result
    }

    fn connection(root: &Path, file: &Path) -> Value {
        let config = Config::parse(
            &json!({
                "schemaVersion":1,"defaultConnection":"local",
                "outputDir":root.join("outputs").to_str().unwrap(),
                "stateDir":root.join("state").to_str().unwrap(),
                "logging":{"directory":root.join("logs").to_str().unwrap()},
                "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
                "connections":{"local":{
                    "adapter":"openai-images","baseUrl":"https://api.example/v1",
                    "auth":{"type":"bearer","secretDpapiFile":file.to_str().unwrap()}
                }}
            })
            .to_string(),
            &root.join("config.json"),
            &[],
            &HashMap::new(),
        )
        .unwrap();
        config.select(None).unwrap().1
    }

    #[test]
    fn parent_abort_reaps_enrolled_worker_and_waiting_grandchild() {
        let workspace = Workspace::new();
        let mut owned = start(&workspace.0, "hang", None, None, false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if owned.text().unwrap().contains("fixture-descendant-ready") {
                break;
            }
            assert!(
                owned.child.try_wait().unwrap().is_none(),
                "Waiting worker exited early."
            );
            assert!(
                Instant::now() < deadline,
                "Waiting descendant readiness exceeded five seconds."
            );
            pause(deadline);
        }
        assert!(
            owned.job.active().unwrap() >= 3,
            "All three native worker generations must be contained."
        );
        owned.stop().unwrap();
        assert!(owned.child.try_wait().unwrap().is_some());
        assert_eq!(owned.job.active().unwrap(), 0);
    }

    #[test]
    fn exact_source_windows_dpapi_script_acl_native_roundtrip_corrupt_and_no_overwrite() {
        let workspace = Workspace::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let root = &workspace.0;
        let scripts = root.join("scripts");
        fs::create_dir(&scripts).unwrap();
        for name in ["common.ps1", "protect-secret.ps1"] {
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("scripts")
                    .join(name),
                scripts.join(name),
            )
            .unwrap();
        }
        let wrapper = root.join("fixture.ps1");
        fs::write(&wrapper, r#"
$ErrorActionPreference='Stop'
Write-Output 'DPAPI_WRAPPER_STARTED'
$directory=[Security.AccessControl.DirectorySecurity]::new()
$directory.SetAccessRuleProtection($true,$false)
$directory.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.WindowsIdentity]::GetCurrent().User,[Security.AccessControl.FileSystemRights]::FullControl,[Security.AccessControl.AccessControlType]::Allow))
Write-Output 'DPAPI_PARENT_SECURITY_READY'
[IO.FileSystemAclExtensions]::Create([IO.DirectoryInfo]::new((Join-Path $PSScriptRoot 'private')),$directory)
Write-Output 'DPAPI_PARENT_CREATED'
function Read-Host { param([string]$Prompt,[switch]$AsSecureString); ConvertTo-SecureString 'non-secret-DPAPI-fixture-日本語' -AsPlainText -Force }
& (Join-Path $PSScriptRoot 'scripts/protect-secret.ps1') -Path $env:NUMERA_FIXTURE_DPAPI
$stage=0
try {
    $acl=Get-Acl -LiteralPath $env:NUMERA_FIXTURE_DPAPI
    $stage=1
    $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $stage=2
    $rules=@($acl.Access | Where-Object {$_.AccessControlType -eq 'Allow'})
    $stage=3
    $foreign=@($rules | Where-Object {$_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -ne $sid})
    $stage=4
    if (-not $acl.AreAccessRulesProtected -or $rules.Count -lt 1 -or $foreign.Count -ne 0) { throw 'Fixture ACL is not current-user only.' }
    Write-Output 'ACL_VERIFIED'
} catch {
    $missing=$false; $unmapped=$false; $denied=$false
    $exception=$_.Exception
    for ($depth=0; $null -ne $exception -and $depth -lt 8; $depth++) {
        $missing=$missing -or ($exception -is [Management.Automation.CommandNotFoundException])
        $unmapped=$unmapped -or ($exception -is [Security.Principal.IdentityNotMappedException])
        $denied=$denied -or ($exception -is [UnauthorizedAccessException])
        $exception=$exception.InnerException
    }
    [Console]::Error.WriteLine('ACL_STAGE_'+$stage)
    [Console]::Error.WriteLine('ACL_COMMAND_MISSING_'+$missing)
    [Console]::Error.WriteLine('ACL_IDENTITY_UNMAPPED_'+$unmapped)
    [Console]::Error.WriteLine('ACL_PERMISSION_DENIED_'+$denied)
    if ($stage -eq 4) {
        [Console]::Error.WriteLine('ACL_PROTECTED_'+$acl.AreAccessRulesProtected)
        [Console]::Error.WriteLine('ACL_HAS_ALLOW_'+($rules.Count -ge 1))
        [Console]::Error.WriteLine('ACL_HAS_FOREIGN_'+($foreign.Count -ne 0))
    }
    exit 1
}
"#.as_bytes()).unwrap();
        let file = root.join("private").join("protected 日本語.txt");
        let (status, output) = run(root, &wrapper, &file, true, deadline);
        let acl = [
            "ACL_STAGE_0",
            "ACL_STAGE_1",
            "ACL_STAGE_2",
            "ACL_STAGE_3",
            "ACL_STAGE_4",
            "ACL_COMMAND_MISSING_True",
            "ACL_IDENTITY_UNMAPPED_True",
            "ACL_PERMISSION_DENIED_True",
            "ACL_PROTECTED_True",
            "ACL_HAS_ALLOW_True",
            "ACL_HAS_FOREIGN_True",
        ]
        .map(|marker| output.lines().any(|line| line.trim() == marker));
        let stages = stage_flags(&output);
        let diagnostic = super::dpapi_worker::diagnostic_summary(&output);
        assert_eq!(
            status.code(),
            Some(0),
            "DPAPI fixture stages: worker_bound_protect_cleanup_wrapper={stages:?}, protect_started={}, protect_succeeded={}, protect_failed={}, acl_verified={}, destination_exists={}, acl_stage0_4_command_identity_permission_protected_allow_foreign={acl:?}, bounded_numeric_diagnostic={diagnostic}",
            output.contains("User-scoped secret protection requested; value and path are not logged."),
            output.contains("DPAPI secret created for the current Windows user; value not logged."),
            output.contains("Secret protection failed. Verify destination, current-user DPAPI and access restrictions before use."),
            output.lines().any(|line| line.trim() == "ACL_VERIFIED"),
            file.is_file(),
        );
        assert!(output.contains("ACL_VERIFIED"));
        assert!(
            !output.contains(PREFIX),
            "Synthetic plaintext must not enter child output"
        );
        let ciphertext = fs::read_to_string(&file).unwrap();
        assert!(
            !ciphertext.contains(PREFIX),
            "Ciphertext must not contain synthetic plaintext"
        );
        let paths = RuntimePaths::new(root).unwrap();
        let env = HashMap::new();
        let actual = runtime
            .block_on(credentials(&connection(root, &file), &env, &paths))
            .unwrap();
        assert_eq!(
            actual,
            HashMap::from([("Authorization".to_owned(), format!("Bearer {FIXTURE}"))])
        );
        let corrupt = root.join("corrupt.txt");
        fs::write(&corrupt, b"not-dpapi").unwrap();
        let error = runtime
            .block_on(credentials(&connection(root, &corrupt), &env, &paths))
            .unwrap_err();
        assert_eq!(error.code, "missing_credentials");
        let (status, _) = run(
            root,
            &scripts.join("protect-secret.ps1"),
            &file,
            false,
            deadline,
        );
        assert_eq!(status.code(), Some(1));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            ciphertext,
            "Repeat must not overwrite ciphertext"
        );
        for entry in fs::read_dir(root.join("logs")).unwrap() {
            let path = entry.unwrap().path();
            assert!(
                fs::metadata(&path).unwrap().len() <= LIMIT,
                "Bounded verification log"
            );
            let log = fs::read_to_string(path).unwrap();
            assert!(
                !log.contains(PREFIX),
                "Synthetic plaintext must not enter logs"
            );
            assert!(!log.contains(&ciphertext), "Ciphertext must not enter logs");
        }
        assert!(
            Instant::now() < deadline,
            "DPAPI case exceeded source thirty-second ceiling"
        );
    }
}
