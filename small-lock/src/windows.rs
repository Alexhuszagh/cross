#![cfg(target_family = "windows")]
#![doc(hidden)]

use std::io;
use std::ptr;

use winapi::shared::minwindef::{DWORD, MAX_PATH};
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use winapi::um::winbase::{INFINITE, WAIT_ABANDONED, WAIT_OBJECT_0};
use winapi::um::winnt::{HANDLE, LPCWSTR};

use crate::{Error, Result};

/// Named (or by path) lock.
#[derive(Debug)]
pub struct InnerLock {
    handle: HANDLE,
    lpname: LPCWSTR,
}

// the logic for the mutex functionality is based off of this example:
// https://docs.microsoft.com/en-us/windows/win32/sync/using-mutex-objects
impl InnerLock {
    pub fn create(name: impl AsRef<str>) -> Result<InnerLock> {
        match is_valid_namespace(name.as_ref()) {
            // SAFETY: safe since it contains valid characters
            true => unsafe { InnerLock::_create(name.as_ref()) },
            false => Err(Error::InvalidCharacter),
        }
    }

    /// # Safety
    ///
    /// Name must have valid characters.
    unsafe fn _create(name: &str) -> Result<InnerLock> {
        let lpname = widestring::WideCString::from_str(name)
            .expect("must be valid due to input UTF-8 string.")
            .into_raw();
        // we want the security descriptor structure to be null,
        // so it cannot be inherited by child processes.
        let handle = CreateMutexW(ptr::null_mut(), 0, lpname);
        match handle.is_null() {
            true => {
                let code = GetLastError();
                Err(Error::CreateFailed(io::Error::from_raw_os_error(
                    code as i32,
                )))
            }
            false => Ok(InnerLock { handle, lpname }),
        }
    }

    pub fn lock(&self) -> Result<InnerLockGuard<'_>> {
        self._lock(INFINITE)
    }

    pub fn try_lock(&self) -> Result<InnerLockGuard<'_>> {
        self._lock(0)
    }

    fn _lock(&self, millis: DWORD) -> Result<InnerLockGuard<'_>> {
        // Safe, since the handle must be valid.
        match unsafe { WaitForSingleObject(self.handle, millis) } {
            WAIT_ABANDONED | WAIT_OBJECT_0 => Ok(InnerLockGuard { lock: self }),
            WAIT_TIMEOUT => Err(Error::WouldBlock),
            code => Err(Error::LockFailed(io::Error::from_raw_os_error(code as i32))),
        }
    }

    pub fn unlock(&self) -> Result<()> {
        self._unlock()
    }

    /// # SAFETY
    ///
    /// Safe, since the handle must be valid.
    fn _unlock(&self) -> Result<()> {
        // Safe, since the handle must be valid.
        match unsafe { ReleaseMutex(self.handle) } {
            0 => {
                // SAFETY: safe, since GetLastError is always safe.
                let code = unsafe { GetLastError() };
                Err(Error::CreateFailed(io::Error::from_raw_os_error(
                    code as i32,
                )))
            }
            _ => Ok(()),
        }
    }
}

impl Drop for InnerLock {
    fn drop(&mut self) {
        // SAFETY: safe, since the handle is valid.
        unsafe { CloseHandle(self.handle) };
        // SAFETY: safe, since the `lpname` must be allocated and valid.
        let _ = unsafe { widestring::WideCString::from_raw(self.lpname as *mut _) };
    }
}

#[derive(Debug)]
pub struct InnerLockGuard<'a> {
    lock: &'a InnerLock,
}

impl<'a> Drop for InnerLockGuard<'a> {
    fn drop(&mut self) {
        self.lock.unlock().ok();
    }
}

fn is_valid_namespace(name: &str) -> bool {
    // the name cannot have a backslash, and must be <= MAX_PATH characters.
    // https://docs.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createmutexw
    name.len() <= MAX_PATH && !name.contains('/')
}
