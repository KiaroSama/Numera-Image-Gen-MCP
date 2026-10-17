//! PowerShell ConvertFrom-SecureString without a key stores DPAPI bytes as hexadecimal.
//! The decrypted bytes are UTF-16LE; no shell, secret argv, temporary file or runtime install.
use super::js_trim;
use std::{ffi::c_void, fs::File, io::Read, path::Path, ptr};

#[repr(C)]
struct Blob {
    length: u32,
    data: *mut u8,
}

#[link(name = "crypt32")]
unsafe extern "system" {
    fn CryptUnprotectData(
        input: *const Blob,
        description: *mut *mut u16,
        entropy: *const Blob,
        reserved: *mut c_void,
        prompt: *const c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

struct Plaintext(Blob);
impl Drop for Plaintext {
    fn drop(&mut self) {
        if !self.0.data.is_null() {
            // SAFETY: CryptUnprotectData owns this length-sized allocation until LocalFree.
            // Volatile writes cannot be optimized away before releasing the Windows allocation.
            for n in 0..self.0.length as usize {
                unsafe {
                    ptr::write_volatile(self.0.data.add(n), 0);
                }
            }
            unsafe {
                LocalFree(self.0.data.cast());
            }
        }
    }
}

pub(super) fn decrypt(path: &Path) -> std::io::Result<String> {
    let fail = || std::io::Error::other("DPAPI credential cannot be decrypted");
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(fail());
    }
    let mut encrypted = Vec::new();
    // Bounded UTF-8 serialized input plus an optional BOM; no reads of arbitrary user state.
    file.take(1024 * 1024 + 1).read_to_end(&mut encrypted)?;
    if encrypted.len() > 1024 * 1024 {
        return Err(fail());
    }
    let text = String::from_utf8(encrypted).map_err(|_| fail())?;
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return Err(fail());
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().as_chunks::<2>().0 {
        let digit = |b: u8| (b as char).to_digit(16).map(|n| n as u8);
        bytes.push(digit(pair[0]).ok_or_else(fail)? * 16 + digit(pair[1]).ok_or_else(fail)?);
    }
    let input = Blob {
        length: u32::try_from(bytes.len()).map_err(|_| fail())?,
        data: bytes.as_mut_ptr(),
    };
    let mut output = Plaintext(Blob {
        length: 0,
        data: ptr::null_mut(),
    });
    // SAFETY: input references live byte storage; output is initialized and released by Plaintext.
    // UI forbidden, no optional entropy, CurrentUser DPAPI exactly like PowerShell Protect.
    let success = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            1,
            &mut output.0,
        )
    };
    if success == 0 || output.0.data.is_null() || !output.0.length.is_multiple_of(2) {
        return Err(fail());
    }
    if output.0.length > 2 * 65536 {
        return Err(fail());
    }
    // SAFETY: successful DPAPI returned non-null output; length is checked and allocation remains live.
    let decrypted = unsafe { std::slice::from_raw_parts(output.0.data, output.0.length as usize) };
    let mut units: Vec<u16> = decrypted
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let key = String::from_utf16(&units).map_err(|_| fail());
    // SAFETY: each unit is an exclusive valid mutable element in the owned vector.
    for unit in &mut units {
        unsafe {
            ptr::write_volatile(unit, 0);
        }
    }
    let key = key?;
    // Source captures UTF-8 stdout with maxBuffer 65536, then JS trim.
    if key.len() > 65536 {
        return Err(fail());
    }
    Ok(js_trim(&key).to_owned())
}
