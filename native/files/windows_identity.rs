//! Stable Win32 handle identity; std Windows file-index accessors are still nightly-only.
use std::{ffi::c_void, fs::File, io, os::windows::io::AsRawHandle};

#[repr(C)]
#[derive(Default)]
struct FileId {
    volume: u64,
    id: [u8; 16],
}
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[repr(C)]
#[derive(Default)]
struct Information {
    attributes: u32,
    created: FileTime,
    accessed: FileTime,
    written: FileTime,
    volume: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandleEx(
        handle: *mut c_void,
        class: i32,
        buffer: *mut c_void,
        size: u32,
    ) -> i32;
    fn GetFileInformationByHandle(handle: *mut c_void, information: *mut Information) -> i32;
}

pub fn identity(file: &File) -> io::Result<(u64, [u8; 16])> {
    let mut id = FileId::default();
    // SAFETY: live File owns handle; FileId is correctly aligned repr(C), passed size is exact.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            18,
            (&mut id as *mut FileId).cast(),
            std::mem::size_of::<FileId>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((id.volume, id.id))
}

pub fn links(file: &File) -> io::Result<u32> {
    let mut information = Information::default();
    // SAFETY: live File owns handle; Information matches BY_HANDLE_FILE_INFORMATION layout.
    let success = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information.links)
}
