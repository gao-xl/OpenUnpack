//! Native clipboard operations. Never read or log clipboard text.
use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
            RegisterClipboardFormatW, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
    },
};
use zeroize::Zeroizing;

const CF_UNICODETEXT: u32 = 13;

struct OpenGuard;
impl Drop for OpenGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

fn open(owner: usize) -> Result<OpenGuard, String> {
    for _ in 0..5 {
        if unsafe { OpenClipboard(owner as HWND) } != 0 {
            return Ok(OpenGuard);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err("系统剪贴板正在使用，请稍后再复制。".into())
}

unsafe fn register_format(name: &str) -> Result<u32, String> {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let format = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    if format == 0 {
        return Err(format!("无法注册剪贴板安全格式: {name}"));
    }
    Ok(format)
}

unsafe fn set_clipboard_dword(format: u32, value: u32) -> Result<(), String> {
    unsafe {
        let buffer = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<u32>());
        if buffer.is_null() {
            return Err("无法分配剪贴板安全标记内存。".into());
        }
        let pointer = GlobalLock(buffer) as *mut u32;
        if pointer.is_null() {
            GlobalFree(buffer);
            return Err("无法锁定剪贴板安全标记内存。".into());
        }
        std::ptr::write(pointer, value);
        GlobalUnlock(buffer);
        if SetClipboardData(format, buffer).is_null() {
            GlobalFree(buffer);
            return Err("无法写入剪贴板安全标记。".into());
        }
        Ok(())
    }
}

unsafe fn zero_and_free_utf16(buffer: *mut core::ffi::c_void, len_u16: usize) {
    unsafe {
        let p = GlobalLock(buffer) as *mut u8;
        if !p.is_null() {
            std::ptr::write_bytes(p, 0, len_u16 * 2);
            GlobalUnlock(buffer);
        }
        GlobalFree(buffer);
    }
}

pub fn copy(owner: usize, text: &str) -> Result<u32, String> {
    if text.contains('\0') {
        return Err("密码包含不支持的字符。".into());
    }
    let data = Zeroizing::new(text.encode_utf16().chain(Some(0)).collect::<Vec<u16>>());
    let _open = open(owner)?;
    unsafe {
        let fmt_exclude = register_format("ExcludeClipboardContentFromMonitorProcessing")?;
        let fmt_no_history = register_format("CanIncludeInClipboardHistory")?;
        let fmt_no_cloud = register_format("CanUploadToCloudClipboard")?;

        let buffer = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2);
        if buffer.is_null() {
            return Err("无法分配剪贴板内存。".into());
        }
        let pointer = GlobalLock(buffer) as *mut u16;
        if pointer.is_null() {
            GlobalFree(buffer);
            return Err("无法写入剪贴板。".into());
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), pointer, data.len());
        GlobalUnlock(buffer);

        if EmptyClipboard() == 0 {
            zero_and_free_utf16(buffer, data.len());
            return Err("无法清空剪贴板。".into());
        }

        // Write privacy markers first; if any fail, abort before placing secret text.
        if let Err(err) = set_clipboard_dword(fmt_exclude, 1)
            .and_then(|()| set_clipboard_dword(fmt_no_history, 0))
            .and_then(|()| set_clipboard_dword(fmt_no_cloud, 0))
        {
            let _ = EmptyClipboard();
            zero_and_free_utf16(buffer, data.len());
            return Err(err);
        }

        if SetClipboardData(CF_UNICODETEXT, buffer).is_null() {
            let _ = EmptyClipboard();
            zero_and_free_utf16(buffer, data.len());
            return Err("无法写入剪贴板。".into());
        }
        // The OS owns buffer after SetClipboardData. Do not free it.
        Ok(GetClipboardSequenceNumber())
    }
}

pub fn clear_if_unchanged(sequence: u32) -> Result<bool, String> {
    let _open = open(0)?;
    if sequence == 0 || unsafe { GetClipboardSequenceNumber() } != sequence {
        return Ok(false);
    }
    if unsafe { EmptyClipboard() } == 0 {
        return Err("剪贴板清理失败。".into());
    }
    Ok(true)
}
