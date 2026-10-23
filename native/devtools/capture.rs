use std::{
    io::{self, Read},
    process::{ChildStderr, ChildStdout},
};

pub(super) const LIMIT: usize = 2 * 1024 * 1024;
pub(super) struct Capture {
    stdout: ChildStdout,
    stderr: ChildStderr,
    pub stdout_bytes: Vec<u8>,
    pub stderr_bytes: Vec<u8>,
    pub exceeded: bool,
}
impl Capture {
    pub fn new(stdout: ChildStdout, stderr: ChildStderr) -> io::Result<Self> {
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        Ok(Self {
            stdout,
            stderr,
            stdout_bytes: Vec::new(),
            stderr_bytes: Vec::new(),
            exceeded: false,
        })
    }
    pub fn poll(&mut self) -> io::Result<bool> {
        let mut progress = false;
        for _ in 0..32 {
            let mut round_progress = false;
            for stderr in [false, true] {
                let total = self.stdout_bytes.len() + self.stderr_bytes.len();
                let mut buffer = [0u8; 8192];
                let capacity = buffer.len().min(LIMIT.saturating_sub(total) + 1);
                let read = if stderr {
                    read_ready(&mut self.stderr, &mut buffer[..capacity])
                } else {
                    read_ready(&mut self.stdout, &mut buffer[..capacity])
                }?;
                round_progress |= read != 0;
                let retained = read.min(LIMIT.saturating_sub(total));
                if stderr {
                    self.stderr_bytes.extend_from_slice(&buffer[..retained]);
                } else {
                    self.stdout_bytes.extend_from_slice(&buffer[..retained]);
                }
                if read > retained {
                    self.exceeded = true;
                }
            }
            progress |= round_progress;
            if !round_progress || self.exceeded {
                break;
            }
        }
        Ok(progress)
    }
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: live owned pipe, F_GETFL takes no third argument; preserve existing flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(unix)]
fn read_ready(
    pipe: &mut (impl Read + std::os::fd::AsRawFd),
    bytes: &mut [u8],
) -> io::Result<usize> {
    match pipe.read(bytes) {
        Ok(read) => Ok(read),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(0)
        }
        Err(error) => Err(error),
    }
}
#[cfg(windows)]
fn nonblocking(_pipe: &impl std::os::windows::io::AsRawHandle) -> io::Result<()> {
    Ok(())
}
#[cfg(windows)]
fn read_ready(
    pipe: &mut (impl Read + std::os::windows::io::AsRawHandle),
    bytes: &mut [u8],
) -> io::Result<usize> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            handle: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }
    let mut available = 0;
    // SAFETY: sole reader, live pipe handle, query-only availability before a bounded read.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(109) {
            Ok(0)
        } else {
            Err(error)
        };
    }
    if available == 0 {
        return Ok(0);
    }
    let length = bytes.len().min(available as usize);
    pipe.read(&mut bytes[..length])
}
