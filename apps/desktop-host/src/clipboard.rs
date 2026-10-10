//! Native clipboard operations. Never read or log clipboard text.
use windows_sys::Win32::{Foundation::{GlobalFree, HWND},System::{DataExchange::{OpenClipboard,CloseClipboard,EmptyClipboard,SetClipboardData,GetClipboardSequenceNumber},Memory::{GlobalAlloc,GlobalLock,GlobalUnlock,GMEM_MOVEABLE}}};
use zeroize::Zeroizing;

struct OpenGuard;
impl Drop for OpenGuard {fn drop(&mut self){unsafe{CloseClipboard();}}}
fn open(owner:usize)->Result<OpenGuard,String>{
    for _ in 0..5 {if unsafe{OpenClipboard(owner as HWND)}!=0{return Ok(OpenGuard)}std::thread::sleep(std::time::Duration::from_millis(20));}
    Err("系统剪贴板正在使用，请稍后再复制。".into())
}
pub fn copy(owner:usize,text:&str)->Result<u32,String>{
    if text.contains('\0'){return Err("密码包含不支持的字符。".into());}
    let data=Zeroizing::new(text.encode_utf16().chain(Some(0)).collect::<Vec<u16>>());
    let _open=open(owner)?;
    unsafe {
        let buffer=GlobalAlloc(GMEM_MOVEABLE,data.len()*2);
        if buffer.is_null(){return Err("无法分配剪贴板内存。".into());}
        let pointer=GlobalLock(buffer) as *mut u16;
        if pointer.is_null(){GlobalFree(buffer);return Err("无法写入剪贴板。".into());}
        std::ptr::copy_nonoverlapping(data.as_ptr(),pointer,data.len());GlobalUnlock(buffer);
        if EmptyClipboard()==0||SetClipboardData(13,buffer).is_null(){
            let p=GlobalLock(buffer) as *mut u8;
            if !p.is_null(){std::ptr::write_bytes(p,0,data.len()*2);GlobalUnlock(buffer);}
            GlobalFree(buffer);return Err("无法写入剪贴板。".into());
        }
        // The OS owns buffer after SetClipboardData. Do not free it.
        Ok(GetClipboardSequenceNumber())
    }
}
pub fn clear_if_unchanged(sequence:u32)->Result<bool,String>{
    let _open=open(0)?;
    if sequence==0||unsafe{GetClipboardSequenceNumber()}!=sequence{return Ok(false);}
    if unsafe{EmptyClipboard()}==0{return Err("剪贴板清理失败。".into());}Ok(true)
}
