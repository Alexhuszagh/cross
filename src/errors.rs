use crate::temp;

use std::sync::atomic::{AtomicBool, Ordering};

pub use color_eyre::Section;
pub use eyre::Context;
pub use eyre::Result;

pub static mut TERMINATED: AtomicBool = AtomicBool::new(false);

pub fn install_panic_hook() -> Result<()> {
    color_eyre::config::HookBuilder::new()
        .display_env_section(false)
        .install()
}

/// # Safety
/// Safe as long as we have single-threaded execution.
unsafe fn termination_handler() {
    // we can't warn the user here, since locks aren't signal-safe.
    // we can delete files, since fdopendir is thread-safe, and
    // `openat`, `unlinkat`, and `lstat` are signal-safe.
    //  https://man7.org/linux/man-pages/man7/signal-safety.7.html
    if !TERMINATED.swap(true, Ordering::SeqCst) && temp::has_tempfiles() {
        temp::clean();
    }

    // EOWNERDEAD, seems to be the same on linux, macos, and bash on windows.
    std::process::exit(130);
}

pub fn install_termination_hook() -> Result<()> {
    // SAFETY: safe since single-threaded execution.
    ctrlc::set_handler(|| unsafe { termination_handler() }).map_err(Into::into)
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("`{1}` failed with exit code: {0}")]
    NonZeroExitCode(std::process::ExitStatus, String),
    #[error("could not execute `{0}`")]
    CouldNotExecute(#[source] Box<dyn std::error::Error + Send + Sync>, String),
    #[error("`{0:?}` output was not UTF-8")]
    Utf8Error(#[source] std::string::FromUtf8Error, std::process::Output),
}
