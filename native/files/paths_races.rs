use super::output_directory_observed;
use std::{
    fs,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

struct Workspace(std::path::PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("owned directory race cleanup");
    }
}

#[cfg(unix)]
fn link(target: &Path, source: &Path) {
    std::os::unix::fs::symlink(source, target).unwrap();
}

#[cfg(windows)]
fn link(target: &Path, source: &Path) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = fs::canonicalize(source.parent().expect("owned fixture root")).unwrap();
    let junction = Path::new("outputs").join(target.file_name().expect("owned junction basename"));
    runtime.block_on(async {
        let mut child = tokio::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .current_dir(directory)
            .arg(junction)
            .arg(source.file_name().expect("owned junction destination"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x08000000)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
            Ok(status) => {
                let status = status.unwrap();
                assert!(
                    status.success(),
                    "owned junction creation failed with {:?}",
                    status.code()
                );
            }
            Err(_) => {
                tokio::time::timeout(Duration::from_secs(2), child.kill())
                    .await
                    .expect("owned junction child must be reaped")
                    .unwrap();
                panic!("owned junction creation exceeded 2 seconds");
            }
        }
    });
    assert!(fs::symlink_metadata(target).unwrap().is_symlink());
}

fn scheduled(kind: &str) {
    let workspace = Workspace(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".ci-work/native-tests")
            .join(uuid::Uuid::new_v4().to_string()),
    );
    let root = workspace.0.join("outputs");
    let outside = workspace.0.join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    let target = fs::canonicalize(&root).unwrap().join("batch");
    let (observed, observations) = mpsc::channel();
    std::thread::scope(|scope| {
        let mut threads = Vec::new();
        let mut releases = Vec::new();
        for _ in 0..2 {
            let observed = observed.clone();
            let target = &target;
            let root = &root;
            let (release, resume) = mpsc::channel();
            releases.push(release);
            threads.push(scope.spawn(move || {
                let mut sent = false;
                output_directory_observed(root, Some("batch/nested"), |path| {
                    let result = fs::symlink_metadata(path);
                    if path == target
                        && !sent
                        && result
                            .as_ref()
                            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                    {
                        sent = true;
                        observed.send(()).unwrap();
                        resume
                            .recv_timeout(Duration::from_secs(6))
                            .expect("directory mutation must release both observations");
                    }
                    result
                })
            }));
        }
        drop(observed);
        let deadline = Instant::now() + Duration::from_secs(2);
        let first = observations.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        let second = observations.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        let changed = std::panic::catch_unwind(|| {
            first.expect("first real missing-path observation");
            second.expect("second real missing-path observation");
            assert!(!target.exists());
            match kind {
                "directory" => {}
                "file" => fs::write(&target, b"fixture").unwrap(),
                "link" => link(&target, &outside),
                _ => unreachable!(),
            }
        });
        for release in releases {
            let _ = release.send(());
        }
        let results: Vec<_> = threads.into_iter().map(|thread| thread.join()).collect();
        changed.unwrap();
        assert!(
            observations.try_recv().is_err(),
            "exactly two missing observations"
        );
        for result in results {
            let result = result.unwrap();
            if kind == "directory" {
                assert_eq!(
                    result.unwrap(),
                    fs::canonicalize(target.join("nested")).unwrap()
                );
            } else {
                assert_eq!(result.unwrap_err().code, "output_file_error");
            }
        }
    });
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    if kind == "file" {
        assert_eq!(fs::read(&target).unwrap(), b"fixture");
    }
}

#[test]
fn two_real_missing_observations_accept_concurrent_directory_creation() {
    scheduled("directory");
}

#[test]
fn two_real_missing_observations_reject_a_concurrent_regular_file() {
    scheduled("file");
}

#[test]
fn two_real_missing_observations_reject_a_concurrent_link_or_windows_junction() {
    scheduled("link");
}
