use crate::compat::value::{Value, json};
use std::{ffi::c_void, mem::size_of};

type Handle = *mut c_void;

#[repr(C)]
struct ThreadEntry {
    exit_status: u32,
    teb: *mut c_void,
    process_id: u32,
    thread_id: u32,
    affinity: usize,
    priority: i32,
    base_priority: i32,
    last_argument: *mut c_void,
    last_syscall: u16,
    created: [u32; 2],
    exited: [u32; 2],
    kernel: [u32; 2],
    user: [u32; 2],
    start_address: *mut c_void,
    captured: [u32; 2],
    flags: u32,
    suspend_count: u16,
    context_size: u16,
    context: *mut c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn PssCaptureSnapshot(process: Handle, flags: u32, context: u32, snapshot: *mut Handle) -> u32;
    fn PssWalkMarkerCreate(allocator: *const c_void, marker: *mut Handle) -> u32;
    fn PssWalkSnapshot(
        snapshot: Handle,
        class: u32,
        marker: Handle,
        buffer: *mut c_void,
        length: u32,
    ) -> u32;
    fn PssWalkMarkerFree(marker: Handle) -> u32;
    fn PssFreeSnapshot(process: Handle, snapshot: Handle) -> u32;
}

fn walk(snapshot: Handle, marker: Handle, pid: u32) -> Result<(u32, u32, u16), u32> {
    let (mut threads, mut suspended, mut maximum) = (0, 0, 0);
    loop {
        // SAFETY: the SDK structure contains only integers and nullable raw pointers. None of
        // its pointers are dereferenced or exposed; thread-only capture excludes memory/context.
        let mut entry: ThreadEntry = unsafe { std::mem::zeroed() };
        // SAFETY: live owned snapshot/marker, initialized correctly sized output structure.
        let code = unsafe {
            PssWalkSnapshot(
                snapshot,
                3,
                marker,
                (&mut entry as *mut ThreadEntry).cast(),
                size_of::<ThreadEntry>() as u32,
            )
        };
        if code == 259 {
            return if threads > 0 {
                Ok((threads, suspended, maximum))
            } else {
                Err(1168)
            };
        }
        if code != 0 {
            return Err(code);
        }
        if entry.process_id != pid || threads == 1024 {
            return Err(13);
        }
        threads += 1;
        suspended += u32::from(entry.suspend_count > 0);
        maximum = maximum.max(entry.suspend_count);
    }
}

pub(super) fn observe(process: Handle, pid: u32) -> Value {
    let mut snapshot = std::ptr::null_mut();
    // SAFETY: caller retains this exact owned process handle. Capture threads only (0x80),
    // without cloning memory, capturing contexts/handles/names or following another process.
    let captured = unsafe { PssCaptureSnapshot(process, 0x80, 0, &mut snapshot) };
    if captured != 0 {
        return report(Err(captured), true);
    }
    let mut marker = std::ptr::null_mut();
    // SAFETY: NULL selects the process heap; output owns one walk marker on success.
    let created = unsafe { PssWalkMarkerCreate(std::ptr::null(), &mut marker) };
    let result = if created == 0 {
        walk(snapshot, marker, pid)
    } else {
        Err(created)
    };
    // SAFETY: each successful allocation is freed exactly once. PSS handles are not Win32
    // CloseHandle handles; the snapshot lives in this caller, not in the inspected child.
    let marker_closed = created != 0 || unsafe { PssWalkMarkerFree(marker) } == 0;
    let snapshot_closed = unsafe { PssFreeSnapshot(GetCurrentProcess(), snapshot) } == 0;
    report(result, marker_closed && snapshot_closed)
}

fn report(result: Result<(u32, u32, u16), u32>, cleaned: bool) -> Value {
    let (counts, error) = match result {
        Ok(counts) => (Some(counts), None),
        Err(code) => (None, Some(code)),
    };
    json!({
        "observed_threads": counts.map(|value| value.0),
        "suspended_threads": counts.map(|value| value.1),
        "max_suspend_count": counts.map(|value| value.2),
        "snapshot_error": error,
        "snapshot_cleanup_verified": cleaned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_layout_and_numeric_projection_match_the_sdk_without_addresses() {
        assert_eq!(size_of::<ThreadEntry>(), 120);
        assert_eq!(std::mem::offset_of!(ThreadEntry, suspend_count), 108);
        let known = report(Ok((3, 2, 1)), true);
        assert_eq!(known["observed_threads"], 3);
        assert_eq!(known["suspended_threads"], 2);
        assert_eq!(known["max_suspend_count"], 1);
        assert!(known["snapshot_error"].is_null());
        let unknown = report(Err(5), false);
        assert!(unknown["observed_threads"].is_null());
        assert_eq!(unknown["snapshot_error"], 5);
        assert_eq!(unknown["snapshot_cleanup_verified"], false);
        assert_eq!(unknown.as_object().unwrap().len(), 5);
    }

    #[test]
    fn current_owned_process_thread_snapshot_is_available_and_released() {
        // SAFETY: query-only pseudo handle for this test's own process, never closed.
        let result = observe(unsafe { GetCurrentProcess() }, std::process::id());
        assert!(result["snapshot_error"].is_null());
        assert!(result["observed_threads"].as_u64().unwrap() > 0);
        assert_eq!(result["snapshot_cleanup_verified"], true);
    }
}
