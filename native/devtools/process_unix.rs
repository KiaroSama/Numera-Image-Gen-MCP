use std::os::unix::process::CommandExt;
use std::{
    io,
    process::{Child, Command},
    time::{Duration, Instant},
};

pub struct Owned {
    child: Child,
    group: Option<i32>,
    stopped: bool,
    cleanup_deadline: Option<Instant>,
}

impl Owned {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        command.process_group(0);
        let child = command.spawn()?;
        let group = i32::try_from(child.id())
            .map_err(|_| io::Error::other("Child identity exceeds process-group range."))?;
        Ok(Self {
            child,
            group: Some(group),
            stopped: false,
            cleanup_deadline: None,
        })
    }

    // A bridge creates no descendants. Keep its caller's process group so an outer owner
    // can contain readiness and bridge together even when the readiness parent is killed.
    pub fn spawn_direct(command: &mut Command) -> io::Result<Self> {
        Ok(Self {
            child: command.spawn()?,
            group: None,
            stopped: false,
            cleanup_deadline: None,
        })
    }

    pub fn pipes(&mut self) -> io::Result<super::Pipes> {
        Ok(super::Pipes {
            stdin: self.child.stdin.take(),
            stdout: self
                .child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("Owned stdout is missing."))?,
            stderr: self
                .child
                .stderr
                .take()
                .ok_or_else(|| io::Error::other("Owned stderr is missing."))?,
        })
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub(super) fn capture(&mut self) -> io::Result<super::capture::Capture> {
        let stdout = self
            .child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Owned stdout is missing."))?;
        let stderr = self
            .child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Owned stderr is missing."))?;
        super::capture::Capture::new(stdout, stderr)
    }

    pub fn exited(&mut self) -> io::Result<bool> {
        // SAFETY: exact owned unreaped PID, initialized result and non-consuming observation.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        #[cfg(target_os = "linux")]
        let pid = unsafe { info.si_pid() };
        #[cfg(target_os = "macos")]
        let pid = info.si_pid;
        Ok(pid != 0)
    }

    pub fn stop(&mut self, deadline: Instant) -> io::Result<i32> {
        if self.stopped {
            return Err(io::Error::other("Owned command already reaped."));
        }
        let deadline = *self.cleanup_deadline.get_or_insert(deadline);
        // The unreaped child anchors PID/PGID identity; direct bridges inherit outer containment.
        if let Some(group) = self.group {
            if (!self.exited()? || live_group(group)?)
                && unsafe { libc::kill(-group, libc::SIGKILL) } == -1
            {
                let error = io::Error::last_os_error();
                match error.raw_os_error() {
                    Some(libc::ESRCH) => {}
                    // macOS refuses zombie-only groups; recheck without ignoring live EPERM.
                    Some(libc::EPERM) if self.exited()? && !live_group(group)? => {}
                    _ => return Err(error),
                }
            }
        } else if !self.exited()? {
            self.child.kill()?;
        }
        while !self.exited()? || self.group.map(live_group).transpose()?.unwrap_or(false) {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Owned process group cleanup exceeded its ceiling.",
                ));
            }
            pause(deadline);
        }
        let status = self
            .child
            .try_wait()?
            .ok_or_else(|| io::Error::other("Owned child exit could not be reaped."))?;
        self.stopped = true;
        use std::os::unix::process::ExitStatusExt;
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.stopped && self.stop(Instant::now() + Duration::from_secs(2)).is_err() {
            eprintln!("ERROR: Owned process group cleanup remains unverified.");
        }
    }
}

fn pause(deadline: Instant) {
    std::thread::park_timeout(
        Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
    );
}

#[cfg(target_os = "linux")]
fn live_group(group: i32) -> io::Result<bool> {
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let text = match std::fs::read_to_string(entry.path().join("stat")) {
            Ok(text) => text,
            Err(error) if matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)) => {
                continue;
            }
            Err(error) => return Err(error),
        };
        let Some((_, fields)) = text.rsplit_once(')') else {
            return Err(io::Error::other("Invalid process accounting record."));
        };
        let fields = fields.split_whitespace().take(3).collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err(io::Error::other("Incomplete process accounting record."));
        }
        if fields[2].parse::<i32>().ok() == Some(group) && fields[0] != "Z" {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
fn live_group(group: i32) -> io::Result<bool> {
    // Apple sys/proc_info.h selector; libc exposes the function but not this constant.
    const PROC_PGRP_ONLY: u32 = 2;
    let mut pids = vec![0i32; 65536];
    let size = std::mem::size_of_val(pids.as_slice());
    // SAFETY: bounded initialized buffer; enumerate only the anchored owned group.
    let bytes = unsafe {
        libc::proc_listpids(
            PROC_PGRP_ONLY,
            group as u32,
            pids.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if bytes <= 0 || bytes as usize >= size {
        return Err(io::Error::other(
            "Cannot bound owned process-group accounting.",
        ));
    }
    for pid in pids
        .into_iter()
        .take(bytes as usize / std::mem::size_of::<i32>())
        .filter(|pid| *pid > 0)
    {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>();
        // SAFETY: sized query-only structure; arg1 includes the anchored unreaped zombie.
        let bytes = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                1,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size as i32,
            )
        };
        if bytes == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(error);
        }
        if bytes as usize != size {
            return Err(io::Error::other("Incomplete owned process accounting."));
        }
        if info.pbi_pgid == group as u32 && info.pbi_status != 5 {
            return Ok(true);
        }
    }
    Ok(false)
}
