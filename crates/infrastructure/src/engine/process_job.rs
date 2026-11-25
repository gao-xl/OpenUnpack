//! Kernel-level supervision for the 7-Zip child processes.
//!
//! `kill_on_drop` only covers one case: the Rust future is dropped while the
//! host process is still alive. If the host crashes, is killed from Task
//! Manager, or the machine loses power to the process tree, the child survives
//! and keeps writing into the extraction workspace. On Windows a Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` closes that hole: the kernel terminates
//! every assigned process when the last handle to the job goes away, which
//! happens on a clean exit *and* on host death, because handles are closed by
//! the kernel when a process object is destroyed.
//!
//! The guard is intentionally cheap to construct and best-effort: if the job
//! cannot be created or the child cannot be assigned (for example a restrictive
//! parent job on an old Windows build), extraction continues with the old
//! `kill_on_drop` behaviour instead of failing the job.

/// Keeps the child process inside a kill-on-close job for its whole lifetime.
pub(crate) struct ProcessJob {
    #[cfg(windows)]
    handle: JobHandle,
}

impl ProcessJob {
    /// Assigns `child` to a fresh job object.
    ///
    /// Returns `None` when supervision is unavailable; callers must keep their
    /// own fallback (we always keep `kill_on_drop`).
    pub(crate) fn attach(child: &tokio::process::Child) -> Option<Self> {
        #[cfg(windows)]
        {
            attach_windows(child.id()?).map(|handle| Self { handle })
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            None
        }
    }

    /// Terminates every process in the job.
    ///
    /// This is the whole-tree kill: unlike `Child::kill`, it also reaches
    /// grandchildren a future engine build might spawn.
    #[allow(dead_code)]
    pub(crate) fn terminate(&self) {
        #[cfg(windows)]
        {
            // SAFETY: the handle is owned by this guard and only closed on drop.
            unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(self.handle.0, 1);
            }
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        // Closing the last handle triggers KILL_ON_JOB_CLOSE for any survivor.
        // SAFETY: the handle is owned by this guard and closed exactly once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle.0);
        }
    }
}

#[cfg(windows)]
struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

// A kernel handle is an opaque value. The guard is moved between threads with
// the future that owns the child, and both Win32 calls used on it are
// thread-agnostic.
#[cfg(windows)]
unsafe impl Send for JobHandle {}
#[cfg(windows)]
unsafe impl Sync for JobHandle {}

#[cfg(windows)]
fn attach_windows(pid: u32) -> Option<JobHandle> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    // SAFETY: null attributes and name request an anonymous job owned by us.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return None;
    }
    let job = JobHandle(job);

    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `limits` is a correctly sized, initialised structure for this class.
    let configured = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return None;
    }

    // SAFETY: `pid` comes from a child we just spawned; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
    if process.is_null() {
        return None;
    }
    // SAFETY: both handles are valid for the duration of the call.
    let assigned = unsafe { AssignProcessToJobObject(job.0, process) };
    // SAFETY: the process handle is ours and no longer needed.
    unsafe {
        CloseHandle(process);
    }
    if assigned == 0 {
        return None;
    }
    Some(job)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// The kernel must report the child as dead once the job is terminated,
    /// which is what makes the crash case safe as well.
    #[tokio::test]
    async fn job_termination_kills_the_supervised_child() {
        let mut child = tokio::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 > NUL"])
            .kill_on_drop(true)
            .spawn()
            .expect("spawn ping");
        let job = ProcessJob::attach(&child).expect("job object");
        job.terminate();
        let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait())
            .await
            .expect("child must be reaped after job termination")
            .expect("wait");
        assert!(
            !status.success(),
            "terminated child must not report success"
        );
    }
}
