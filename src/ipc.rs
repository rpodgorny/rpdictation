// Windows IPC using named events for stop signaling.
// This file is only compiled on Windows (gated by #[cfg(windows)] mod declaration in main.rs).

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, ReleaseMutex, SetEvent, WaitForSingleObject,
    EVENT_MODIFY_STATE,
};

/// Handle to a Windows named event used for stop signaling between processes.
/// The recording process creates the event and waits on it.
/// The `rpdictation stop` process opens and signals it.
pub struct StopEvent {
    handle: HANDLE,
}

// SAFETY: Windows event handles can be safely used from any thread.
unsafe impl Send for StopEvent {}
unsafe impl Sync for StopEvent {}

impl StopEvent {
    /// Creates the named stop event `Global\rpdictation_stop`.
    /// Call this from the recording process at startup.
    pub fn create() -> Result<Self> {
        let handle = unsafe {
            CreateEventW(
                None,  // default security
                true,  // manual reset
                false, // initially not signaled
                w!("Global\\rpdictation_stop"),
            )
            .context("Failed to create stop event")?
        };
        Ok(Self { handle })
    }

    /// Waits for the stop event to be signaled.
    /// Polls with 100ms timeout in a blocking thread to remain async-friendly.
    /// Checks the cancellation token between polls so the wait can be cancelled.
    pub async fn wait(&self, cancel: &CancellationToken) -> Result<()> {
        // Extract raw handle value for Send across thread boundary.
        // HANDLE is a newtype; cast its inner value to usize for portability.
        let raw: usize = unsafe { std::mem::transmute_copy(&self.handle) };
        let cancel = cancel.clone();
        tokio::task::spawn_blocking(move || {
            let handle: HANDLE = unsafe { std::mem::transmute(raw) };
            loop {
                if cancel.is_cancelled() {
                    anyhow::bail!("Cancelled");
                }
                let result = unsafe { WaitForSingleObject(handle, 100) };
                // WAIT_OBJECT_0 is 0 — the event was signaled
                if result.0 == 0 {
                    return Ok(());
                }
                // WAIT_TIMEOUT (258) or other values mean keep polling
            }
        })
        .await
        .context("Stop event wait task panicked")?
    }
}

impl Drop for StopEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// RAII guard for the Windows named mutex used for single-instance detection.
/// Hold this for the lifetime of the recording to prevent concurrent instances.
pub struct InstanceMutex {
    handle: HANDLE,
}

// SAFETY: Windows mutex handles can be safely used from any thread.
unsafe impl Send for InstanceMutex {}
unsafe impl Sync for InstanceMutex {}

impl InstanceMutex {
    /// Acquires the named mutex `Global\rpdictation_instance`.
    /// Returns Ok if acquired, Err if another instance already holds it.
    pub fn acquire() -> Result<Self> {
        let handle = unsafe {
            CreateMutexW(
                None, // default security
                true, // initial owner
                w!("Global\\rpdictation_instance"),
            )
            .context("Failed to create instance mutex")?
        };
        // Check if the mutex already existed (another instance is running)
        let last_error = unsafe { GetLastError() };
        if last_error == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            anyhow::bail!("Another instance is already running");
        }
        Ok(Self { handle })
    }
}

impl Drop for InstanceMutex {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Check if another instance is running by attempting to create the instance mutex.
/// Creates a temporary mutex handle just to check, then immediately releases it.
pub fn check_instance_running() -> bool {
    let result = unsafe {
        CreateMutexW(
            None,
            false, // don't take ownership
            w!("Global\\rpdictation_instance"),
        )
    };
    match result {
        Ok(handle) => {
            let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            unsafe {
                let _ = CloseHandle(handle);
            }
            already_exists
        }
        Err(_) => false,
    }
}

/// Signal the stop event from another process (used by `rpdictation stop`).
/// Opens the existing named event and signals it. Returns an error if no
/// recording is in progress (event doesn't exist).
pub fn signal_stop() -> Result<()> {
    let handle = unsafe {
        OpenEventW(EVENT_MODIFY_STATE, false, w!("Global\\rpdictation_stop"))
            .context("No recording in progress (stop event not found)")?
    };
    unsafe {
        SetEvent(handle).context("Failed to signal stop event")?;
        let _ = CloseHandle(handle);
    }
    println!("Stop signal sent to recording process");
    Ok(())
}
