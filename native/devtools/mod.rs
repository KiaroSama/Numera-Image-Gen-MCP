pub mod artifact;
mod capture;
mod installation;
#[cfg(unix)]
mod process_unix;
#[cfg(windows)]
mod process_windows;
pub mod setup;
pub mod supervisor;
#[cfg(windows)]
mod windows_snapshot;

#[cfg(unix)]
pub use process_unix::Owned;
#[cfg(windows)]
pub use process_windows::Owned;

pub struct Pipes {
    pub stdin: Option<std::process::ChildStdin>,
    pub stdout: std::process::ChildStdout,
    pub stderr: std::process::ChildStderr,
}
