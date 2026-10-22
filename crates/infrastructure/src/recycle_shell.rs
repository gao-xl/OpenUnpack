use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{E_ABORT, E_INVALIDARG},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::Shell::*,
    },
    core::{HRESULT, PCWSTR, Ref, Result, implement},
};

#[implement(IFileOperationProgressSink)]
struct RecycleReceipt(Arc<AtomicBool>);
#[allow(non_snake_case, clippy::too_many_arguments)]
impl IFileOperationProgressSink_Impl for RecycleReceipt_Impl {
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, result: HRESULT) -> Result<()> {
        result.ok()
    }
    fn PreDeleteItem(&self, flags: u32, _item: Ref<IShellItem>) -> Result<()> {
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            return Err(E_ABORT.into());
        }
        Ok(())
    }
    fn PostDeleteItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        result: HRESULT,
        created: Ref<IShellItem>,
    ) -> Result<()> {
        result.ok()?;
        if created.is_null() {
            return Err(E_ABORT.into());
        }
        self.0.store(true, Ordering::Release);
        Ok(())
    }
    fn PreRenameItem(&self, _flags: u32, _item: Ref<IShellItem>, _name: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _name: &PCWSTR,
        _result: HRESULT,
        _created: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _destination: Ref<IShellItem>,
        _name: &PCWSTR,
    ) -> Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _destination: Ref<IShellItem>,
        _name: &PCWSTR,
        _result: HRESULT,
        _created: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _destination: Ref<IShellItem>,
        _name: &PCWSTR,
    ) -> Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _flags: u32,
        _item: Ref<IShellItem>,
        _destination: Ref<IShellItem>,
        _name: &PCWSTR,
        _result: HRESULT,
        _created: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreNewItem(&self, _flags: u32, _destination: Ref<IShellItem>, _name: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _flags: u32,
        _destination: Ref<IShellItem>,
        _name: &PCWSTR,
        _template: &PCWSTR,
        _attributes: u32,
        _result: HRESULT,
        _created: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _total: u32, _completed: u32) -> Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

struct ShellFailure {
    stage: &'static str,
    error: windows::core::Error,
}

fn shell_step<T>(stage: &'static str, result: Result<T>) -> std::result::Result<T, ShellFailure> {
    result.map_err(|error| ShellFailure { stage, error })
}

fn shell_parsing_path(path: &Path) -> Result<Vec<u16>> {
    crate::saferecycle::validate_shell_path(path).map_err(|_| E_INVALIDARG)?;
    let display = path.to_str().ok_or(E_INVALIDARG)?;
    let clean = if let Some(tail) = display.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{tail}")
    } else if let Some(tail) = display.strip_prefix(r"\\?\") {
        tail.to_owned()
    } else {
        display.to_owned()
    };
    // Win32 accepts slash separators in ordinary paths, but the Shell parser
    // rejects them with E_INVALIDARG. Verbatim literal slashes are rejected by
    // validate_shell_path rather than mapped onto another source object.
    Ok(std::ffi::OsStr::new(&clean.replace('/', r"\"))
        .encode_wide()
        .chain(Some(0))
        .collect())
}

pub(crate) fn recycle(path: &Path) -> std::result::Result<(), String> {
    // COM objects and their apartment are created, used and released on one
    // dedicated STA; the caller retains the source/ancestor guards until join.
    let path = path.to_owned();
    std::thread::spawn(move || -> std::result::Result<(), ShellFailure> {
        unsafe {
            let wide = shell_step("shellPath", shell_parsing_path(&path))?;
            shell_step(
                "CoInitializeEx",
                CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok(),
            )?;
            let _apartment = Apartment;
            let operation: IFileOperation = shell_step(
                "CoCreateInstance",
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER),
            )?;
            shell_step(
                "SetOperationFlags",
                operation.SetOperationFlags(
                    FOFX_RECYCLEONDELETE
                        | FOFX_EARLYFAILURE
                        | FOFX_ADDUNDORECORD
                        | FOF_NOERRORUI
                        | FOF_SILENT
                        | FOF_NOCONFIRMATION
                        | FOF_NO_CONNECTED_ELEMENTS,
                ),
            )?;
            let item: IShellItem = shell_step(
                "SHCreateItemFromParsingName",
                SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None),
            )?;
            let confirmed = Arc::new(AtomicBool::new(false));
            let sink: IFileOperationProgressSink = RecycleReceipt(confirmed.clone()).into();
            shell_step("DeleteItem", operation.DeleteItem(&item, &sink))?;
            shell_step("PerformOperations", operation.PerformOperations())?;
            if shell_step(
                "GetAnyOperationsAborted",
                operation.GetAnyOperationsAborted(),
            )?
            .as_bool()
                || !confirmed.load(Ordering::Acquire)
            {
                return Err(ShellFailure {
                    stage: "recycleReceipt",
                    error: E_ABORT.into(),
                });
            }
            Ok(())
        }
    })
    .join()
    .map_err(|_| "回收站操作异常；请核对源文件。".to_owned())?
    .map_err(|failure| {
        format!(
            "源文件回收未完成；请核对源文件（{}；{}）。",
            failure.stage,
            failure.error.code()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_path_normalizes_win32_separators_and_extended_prefixes() {
        for (input, expected) in [
            (r"C:/private/归档.zip", r"C:\private\归档.zip"),
            (r"C:\private/archive.zip", r"C:\private\archive.zip"),
            (r"\\?\C:\private\archive.zip", r"C:\private\archive.zip"),
            (
                r"\\?\UNC\server\share\archive.zip",
                r"\\server\share\archive.zip",
            ),
        ] {
            let wide = shell_parsing_path(Path::new(input)).unwrap();
            assert_eq!(wide.last(), Some(&0));
            assert_eq!(
                String::from_utf16(&wide[..wide.len() - 1]).unwrap(),
                expected
            );
        }
        assert!(shell_parsing_path(Path::new(r"\\?\C:\private\nested/archive.zip")).is_err());
    }

    #[test]
    fn shell_errors_retain_stage_and_hresult() {
        let failure = shell_step::<()>("SHCreateItemFromParsingName", Err(E_INVALIDARG.into()))
            .err()
            .unwrap();
        assert_eq!(failure.stage, "SHCreateItemFromParsingName");
        assert_eq!(failure.error.code(), E_INVALIDARG);
    }

    #[test]
    fn missing_recycle_flag_or_receipt_never_confirms_deletion() {
        let confirmed = Arc::new(AtomicBool::new(false));
        let sink: IFileOperationProgressSink = RecycleReceipt(confirmed.clone()).into();
        unsafe {
            assert_eq!(sink.PreDeleteItem(0x202, None).unwrap_err().code(), E_ABORT);
            sink.PreDeleteItem(TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32, None)
                .unwrap();
            assert_eq!(
                sink.PostDeleteItem(0, None, HRESULT(0), None)
                    .unwrap_err()
                    .code(),
                E_ABORT
            );
            assert_eq!(
                sink.PostDeleteItem(0, None, E_INVALIDARG, None)
                    .unwrap_err()
                    .code(),
                E_INVALIDARG
            );
        }
        assert!(!confirmed.load(Ordering::Acquire));
    }
}
