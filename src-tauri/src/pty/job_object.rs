use crate::domain::error::VeloxResult;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

/// RAII wrapper for Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
///
/// On non-Windows platforms, this compiles as a no-op stub.
/// Implements `Send + Sync` for thread-safe sharing across the PTY manager.
pub struct ProcessJobObject {
    #[cfg(windows)]
    handle: Mutex<Option<HANDLE>>,
    #[cfg(windows)]
    terminated: AtomicBool,
    #[cfg(not(windows))]
    _phantom: (),
}

impl ProcessJobObject {
    /// Creates a new Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
    /// and `JOB_OBJECT_LIMIT_BREAKAWAY_OK`.
    pub fn new() -> VeloxResult<Self> {
        #[cfg(windows)]
        {
            // SAFETY: CreateJobObjectW with null security attributes and null name is valid.
            // Returns a valid job handle or NULL on failure.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(crate::domain::error::VeloxError::Pty(
                    "CreateJobObjectW failed".to_string(),
                ));
            }

            // Configure the job object to kill all processes on close and allow breakaway
            // (required for child processes to be assigned when parent is already in a job).
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;

            // SAFETY: SetInformationJobObject with valid handle and correct structure size.
            let result = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if result == 0 {
                // SAFETY: Clean up handle on configuration failure.
                unsafe { CloseHandle(handle) };
                return Err(crate::domain::error::VeloxError::Pty(
                    "SetInformationJobObject failed".to_string(),
                ));
            }

            Ok(Self {
                handle: Mutex::new(Some(handle)),
                terminated: AtomicBool::new(false),
            })
        }

        #[cfg(not(windows))]
        {
            Ok(Self { _phantom: () })
        }
    }

    /// Assigns a process ID to this Job Object.
    ///
    /// On Windows, opens the process with `PROCESS_SET_QUOTA | PROCESS_TERMINATE`,
    /// assigns it to the job, and closes the process handle.
    /// Logs a warning if assignment fails (e.g., nested job restriction) but returns Ok.
    pub fn assign_pid(&self, pid: u32) -> VeloxResult<()> {
        #[cfg(windows)]
        {
            // SAFETY: OpenProcess with PROCESS_SET_QUOTA | PROCESS_TERMINATE is valid for job assignment.
            let process_handle =
                unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if process_handle.is_null() {
                tracing::warn!(pid, "OpenProcess failed, cannot assign to job object");
                return Ok(());
            }

            let handle_opt = self.handle.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(handle) = *handle_opt {
                // SAFETY: AssignProcessToJobObject with valid job and process handles.
                let result = unsafe { AssignProcessToJobObject(handle, process_handle) };
                if result == 0 {
                    tracing::warn!(
                        pid,
                        "AssignProcessToJobObject failed (nested job restriction?)"
                    );
                }
            }

            // SAFETY: Always close the process handle we opened.
            unsafe { CloseHandle(process_handle) };
            Ok(())
        }

        #[cfg(not(windows))]
        {
            let _ = pid;
            Ok(())
        }
    }

    /// Terminates all processes in the Job Object with exit code 1.
    pub fn terminate(&self) -> VeloxResult<()> {
        #[cfg(windows)]
        {
            let mut handle_opt = self.handle.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(handle) = handle_opt.take() {
                self.terminated.store(true, Ordering::SeqCst);
                // SAFETY: TerminateJobObject with valid handle and exit code 1.
                let result = unsafe { TerminateJobObject(handle, 1) };
                if result == 0 {
                    return Err(crate::domain::error::VeloxError::Pty(
                        "TerminateJobObject failed".to_string(),
                    ));
                }
                // SAFETY: Close handle after termination.
                unsafe { CloseHandle(handle) };
            }
            Ok(())
        }

        #[cfg(not(windows))]
        {
            Ok(())
        }
    }
}

impl Drop for ProcessJobObject {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if !self.terminated.load(Ordering::SeqCst) {
                let mut handle_opt = self.handle.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(handle) = handle_opt.take() {
                    // SAFETY: Close handle on drop if not already terminated.
                    unsafe { CloseHandle(handle) };
                }
            }
        }
    }
}

// ProcessJobObject is Send + Sync because:
// - On Windows: Mutex<Option<HANDLE>> is Send + Sync, AtomicBool is Send + Sync
// - On non-Windows: zero-sized type is trivially Send + Sync
unsafe impl Send for ProcessJobObject {}
unsafe impl Sync for ProcessJobObject {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_object_creates_and_drops_cleanly() {
        let job = ProcessJobObject::new().expect("Failed to create job object");
        // Drop should not panic or double-close
        drop(job);
    }

    #[test]
    fn job_object_assign_pid_noop() {
        let job = ProcessJobObject::new().expect("Failed to create job object");
        // assign_pid with invalid PID should not crash
        job.assign_pid(0).expect("assign_pid should not fail");
        drop(job);
    }

    #[test]
    fn job_object_terminate_idempotent() {
        let job = ProcessJobObject::new().expect("Failed to create job object");
        job.terminate().expect("terminate should succeed");
        // Second terminate should be safe (handle already taken)
        job.terminate().expect("second terminate should not fail");
        // Drop should not double-close
        drop(job);
    }
}
