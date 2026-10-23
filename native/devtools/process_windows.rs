use std::{
    ffi::c_void,
    io,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command},
    time::{Duration, Instant},
};

type Handle = *mut c_void;
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
#[repr(C)]
#[derive(Default)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    id: u32,
    owner: u32,
    priority: i32,
    delta: i32,
    flags: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
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
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn IsProcessInJob(process: Handle, job: Handle, member: *mut i32) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
    fn OpenThread(access: u32, inherit: i32, id: u32) -> Handle;
    fn ResumeThread(thread: Handle) -> u32;
    fn GetLastError() -> u32;
    fn GetProcessIdOfThread(thread: Handle) -> u32;
    fn GetProcessTimes(
        process: Handle,
        created: *mut u64,
        exited: *mut u64,
        kernel: *mut u64,
        user: *mut u64,
    ) -> i32;
}

fn handle(raw: Handle) -> io::Result<OwnedHandle> {
    if raw.is_null() || raw as isize == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful APIs transfer one real noninheritable handle, never a pseudo handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

pub struct Owned {
    child: Child,
    job: OwnedHandle,
    stopped: bool,
    cleanup_deadline: Option<Instant>,
    startup_ms: u64,
    resumed_threads: usize,
    running_threads: usize,
}

impl Owned {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let begin = Instant::now();
        // A suspended command cannot create descendants before assignment to its owned job.
        let job = handle(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) })?;
        let mut limits = ExtendedLimits::default();
        limits.basic.flags = 0x2000;
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                9,
                (&limits as *const ExtendedLimits).cast(),
                std::mem::size_of::<ExtendedLimits>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        command.creation_flags(0x08000004);
        let child = command.spawn()?;
        let mut owned = Self {
            child,
            job,
            stopped: false,
            cleanup_deadline: None,
            startup_ms: 0,
            resumed_threads: 0,
            running_threads: 0,
        };
        if unsafe {
            AssignProcessToJobObject(owned.job.as_raw_handle(), owned.child.as_raw_handle())
        } == 0
        {
            let error = io::Error::last_os_error();
            owned.stop(Instant::now() + Duration::from_secs(2))?;
            return Err(error);
        }
        match owned.resume() {
            Ok((resumed, running)) => {
                owned.resumed_threads = resumed;
                owned.running_threads = running;
            }
            Err(error) => {
                owned.stop(Instant::now() + Duration::from_secs(2))?;
                return Err(error);
            }
        }
        owned.startup_ms = begin.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        Ok(owned)
    }

    fn resume(&self) -> io::Result<(usize, usize)> {
        let snapshot = handle(unsafe { CreateToolhelp32Snapshot(4, 0) })?;
        let mut entry = ThreadEntry {
            size: std::mem::size_of::<ThreadEntry>() as u32,
            ..Default::default()
        };
        if unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut threads = Vec::new();
        loop {
            if entry.size < 16 {
                return Err(io::Error::other("Incomplete owned thread identity."));
            }
            if entry.owner == self.child.id() {
                // Snapshot order does not identify the primary thread; retain all owned handles.
                let thread = handle(unsafe { OpenThread(0x802, 0, entry.id) })?;
                // SAFETY: live query-capable handle, checked before any thread state changes.
                let owner = unsafe { GetProcessIdOfThread(thread.as_raw_handle()) };
                if owner == 0 {
                    return Err(io::Error::last_os_error());
                }
                if owner != self.child.id() {
                    return Err(io::Error::other("Owned thread identity changed."));
                }
                threads.push(thread);
            }
            entry.size = std::mem::size_of::<ThreadEntry>() as u32;
            if unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } == 0 {
                let error = unsafe { GetLastError() };
                if error == 18 {
                    break;
                }
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        resume_all(threads.iter().map(|thread| {
            // SAFETY: retained handle belongs to the suspended, already job-assigned child.
            let previous = unsafe { ResumeThread(thread.as_raw_handle()) };
            if previous == u32::MAX {
                Err(io::Error::last_os_error())
            } else {
                Ok(previous)
            }
        }))
    }

    pub fn spawn_direct(command: &mut Command) -> io::Result<Self> {
        Self::spawn(command)
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
        Ok(self.child.try_wait()?.is_some())
    }

    pub(super) fn thread_snapshot(&self) -> crate::compat::value::Value {
        super::windows_snapshot::observe(self.child.as_raw_handle(), self.child.id())
    }

    pub(super) fn diagnostics(&mut self) -> crate::compat::value::Value {
        use crate::compat::value::json;
        let (mut created, mut exited, mut kernel, mut user) = (0, 0, 0, 0);
        // SAFETY: retained owned process handle and four correctly sized FILETIME outputs;
        // only CPU durations are reported, never process arguments, paths or environment.
        let times = unsafe {
            GetProcessTimes(
                self.child.as_raw_handle(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } != 0;
        json!({
            "pid": self.child.id(),
            "startup_ms": self.startup_ms,
            "resumed_threads": self.resumed_threads,
            "running_threads": self.running_threads,
            "child_running": self.child.try_wait().ok().map(|status| status.is_none()),
            "kernel_ms": times.then_some(kernel / 10000),
            "user_ms": times.then_some(user / 10000),
        })
    }

    fn active(&self) -> io::Result<u32> {
        let mut accounting = Accounting::default();
        // SAFETY: complete initialized accounting structure, owned job and query-only API.
        if unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
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

    fn members(&self, deadline: Instant) -> io::Result<Vec<OwnedHandle>> {
        // Two DWORD counts precede an aligned ULONG_PTR array on both supported 64-bit targets.
        let mut buffer = vec![0usize; 65537];
        // SAFETY: initialized, pointer-aligned buffer; class3 includes nested jobs, bounded to65536PIDs.
        if unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
                3,
                buffer.as_mut_ptr().cast(),
                std::mem::size_of_val(buffer.as_slice()) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let counts = buffer[0];
        let assigned = counts as u32;
        let returned = (counts >> 32) as u32;
        if assigned != returned || returned as usize > 65536 {
            return Err(io::Error::other("Owned job member list is incomplete."));
        }
        let mut members = Vec::new();
        for pid in &buffer[1..1 + returned as usize] {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Owned job member inspection exceeded its ceiling.",
                ));
            }
            let pid = u32::try_from(*pid)
                .map_err(|_| io::Error::other("Owned job member identity is invalid."))?;
            // SAFETY: query/synchronize only; membership is rechecked before retaining the handle.
            let raw = unsafe { OpenProcess(0x00101000, 0, pid) };
            if raw.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(87) {
                    continue;
                }
                return Err(error);
            }
            let process = handle(raw)?;
            let mut member = 0;
            // SAFETY: live query-capable handles and initialized BOOL output.
            if unsafe {
                IsProcessInJob(
                    process.as_raw_handle(),
                    self.job.as_raw_handle(),
                    &mut member,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if member != 0 {
                members.push(process);
            }
        }
        Ok(members)
    }

    pub fn stop(&mut self, deadline: Instant) -> io::Result<i32> {
        let deadline = *self.cleanup_deadline.get_or_insert(deadline);
        let original = self.child.try_wait()?;
        if original.is_none() {
            self.child.kill()?;
        }
        while self.child.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Owned child did not stop.",
                ));
            }
            pause(deadline);
        }
        let members = self.members(deadline);
        if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut members = members?;
        members.extend(self.members(deadline)?);
        loop {
            let mut pending = Vec::new();
            for process in members {
                // SAFETY: retained immutable process handle; zero-time state observation only.
                match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
                    0 => {}
                    258 => pending.push(process),
                    _ => return Err(io::Error::last_os_error()),
                }
            }
            members = pending;
            if self.active()? == 0 && members.is_empty() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Owned job cleanup exceeded its ceiling.",
                ));
            }
            pause(deadline);
        }
        self.stopped = true;
        Ok(original.and_then(|status| status.code()).unwrap_or(1))
    }
}

fn resume_all(counts: impl IntoIterator<Item = io::Result<u32>>) -> io::Result<(usize, usize)> {
    let (mut resumed, mut running) = (0, 0);
    for previous in counts {
        match previous? {
            0 => running += 1,
            1 => resumed += 1,
            _ => {
                return Err(io::Error::other(
                    "Owned thread remains suspended after one resume.",
                ));
            }
        }
    }
    if resumed > 0 {
        Ok((resumed, running))
    } else {
        Err(io::Error::other(
            "Owned suspended primary thread is missing.",
        ))
    }
}

fn pause(deadline: Instant) {
    std::thread::park_timeout(
        Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
    );
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.stopped && self.stop(Instant::now() + Duration::from_secs(2)).is_err() {
            eprintln!("ERROR: Owned Windows job cleanup remains unverified.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resume_all;
    use std::cell::Cell;

    #[test]
    fn resume_summary_counts_every_owned_thread_without_claiming_primary_identity() {
        assert_eq!(resume_all([Ok(0), Ok(1), Ok(1), Ok(0)]).unwrap(), (2, 2));
        assert_eq!(resume_all([Ok(1)]).unwrap(), (1, 0));
        assert!(resume_all([Ok(0), Ok(0)]).is_err());
        assert!(resume_all([Ok(1), Ok(2)]).is_err());
    }

    #[test]
    fn resumes_every_owned_snapshot_thread_instead_of_stopping_at_the_first() {
        let calls = Cell::new(0);
        let counts = [1, 1].into_iter().map(|count| {
            calls.set(calls.get() + 1);
            Ok(count)
        });
        resume_all(counts).unwrap();
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn running_auxiliary_does_not_hide_the_suspended_primary() {
        resume_all([Ok(0), Ok(1), Ok(0)]).unwrap();
        assert!(resume_all([Ok(0)]).is_err());
        assert!(resume_all([]).is_err());
        assert!(resume_all([Ok(1), Ok(2)]).is_err());
        let failure = std::io::Error::other("Controlled resume failure");
        assert!(resume_all([Ok(1), Err(failure)]).is_err());
    }
}
