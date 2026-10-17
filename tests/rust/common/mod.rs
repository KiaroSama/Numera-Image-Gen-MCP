use std::path::{Path, PathBuf};

pub struct Workspace(pub PathBuf);
impl Workspace {
    pub fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".ci-work/native-tests")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).expect("isolated workspace");
        Self(root)
    }
}
pub fn cleanup_retryable(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 145 | 303))
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let error = match std::fs::remove_dir_all(&self.0) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => error,
            };
            if cfg!(windows) && cleanup_retryable(&error) && std::time::Instant::now() < deadline {
                std::thread::yield_now();
                continue;
            }
            let detail = format!(
                "kind={:?}, os_code={:?}",
                error.kind(),
                error.raw_os_error()
            );
            if std::thread::panicking() {
                eprintln!("Fixture cleanup failed: {detail}.");
            } else {
                panic!("Fixture cleanup failed: {detail}.");
            }
            break;
        }
    }
}
