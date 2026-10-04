//! Windows identifies a browser executable from its version resource.
//!
//! `chrome.exe` and `msedge.exe` are GUI-subsystem programs that print nothing
//! for `--version`, so the executable is never run: ProductName and
//! ProductVersion are read from the file itself.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// One string value of the version block, read as UTF-16 up to its terminator.
unsafe fn query_string(block: &[u8], sub_block: &str) -> Option<String> {
    let name = wide(sub_block);
    let mut data: *mut c_void = std::ptr::null_mut();
    let mut chars: u32 = 0;
    // SAFETY: `block` is a complete version block and `name` is NUL terminated.
    let found =
        unsafe { VerQueryValueW(block.as_ptr().cast(), name.as_ptr(), &mut data, &mut chars) };
    if found == 0 || data.is_null() || chars == 0 {
        return None;
    }
    // SAFETY: on success `data` points inside `block` at `chars` UTF-16 units.
    let units = unsafe { std::slice::from_raw_parts(data.cast::<u16>(), chars as usize) };
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

/// `<ProductName> <ProductVersion>` for the executable, in the shape the
/// supported-variant check reads (for example `Microsoft Edge 154.0.4258.53`).
pub fn product_identity(path: &Path) -> Result<String, String> {
    let file: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut handle = 0u32;
    // SAFETY: `file` is NUL terminated.
    let size = unsafe { GetFileVersionInfoSizeW(file.as_ptr(), &mut handle) };
    if size == 0 {
        return Err("the executable has no version resource".to_string());
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` is `size` bytes as the size query requested.
    let read = unsafe { GetFileVersionInfoW(file.as_ptr(), 0, size, block.as_mut_ptr().cast()) };
    if read == 0 {
        return Err("the executable's version resource could not be read".to_string());
    }
    // SAFETY: `block` holds the version block read above.
    let translations = unsafe { query_string_raw_translations(&block) };
    for (language, codepage) in translations {
        let prefix = format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\");
        // SAFETY: as above.
        let name = unsafe { query_string(&block, &format!("{prefix}ProductName")) };
        let version = unsafe { query_string(&block, &format!("{prefix}ProductVersion")) };
        if let (Some(name), Some(version)) = (name, version) {
            return crate::detect::compose_product_identity(&name, &version);
        }
    }
    Err("the executable's version resource has no ProductName and ProductVersion".to_string())
}

/// `(language, codepage)` pairs of `\VarFileInfo\Translation`.
unsafe fn query_string_raw_translations(block: &[u8]) -> Vec<(u16, u16)> {
    let name = wide("\\VarFileInfo\\Translation");
    let mut data: *mut c_void = std::ptr::null_mut();
    let mut bytes: u32 = 0;
    // SAFETY: `block` is a complete version block and `name` is NUL terminated.
    let found =
        unsafe { VerQueryValueW(block.as_ptr().cast(), name.as_ptr(), &mut data, &mut bytes) };
    if found == 0 || data.is_null() {
        return Vec::new();
    }
    // SAFETY: on success `data` points inside `block` at `bytes` bytes of u16 pairs.
    let units = unsafe { std::slice::from_raw_parts(data.cast::<u16>(), bytes as usize / 2) };
    units
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect()
}
