#![deny(missing_debug_implementations, rust_2018_idioms)]
#![warn(
    clippy::explicit_into_iter_loop,
    clippy::explicit_iter_loop,
    clippy::implicit_clone,
    clippy::inefficient_to_string,
    clippy::map_err_ignore,
    clippy::map_unwrap_or,
    clippy::ref_binding_to_reference,
    clippy::semicolon_if_nothing_returned,
    clippy::str_to_string,
    clippy::string_to_string,
    // needs clippy 1.61 clippy::unwrap_used
)]
#![allow(unused)] // TODO(ahuszagh) Remove this

use std::cmp;
use std::fmt;
use std::fs;
use std::path::Path;

use clap::Args;

mod format;
mod git;
mod id;

#[derive(Args, Debug)]
pub struct BuildChangelog {
    /// Provide verbose diagnostic output.
    #[clap(short, long, env = "CARGO_TERM_VERBOSE")]
    pub verbose: bool,
    /// Do not print cross log messages.
    #[clap(short, long, env = "CARGO_TERM_QUIET")]
    pub quiet: bool,
    /// Whether messages should use color output.
    #[clap(long, env = "CARGO_TERM_COLOR")]
    pub color: Option<String>,
    /// Build a release changelog.
    #[clap(long, env = "NEW_VERSION")]
    release: Option<String>,
    /// Whether we're doing a dry run or not.
    #[clap(long, env = "DRY_RUN")]
    dry_run: bool,
}

#[derive(Args, Debug)]
pub struct ValidateChangelog {
    /// List of changelog entries to validate.
    files: Vec<String>,
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Do not print cross log messages.
    #[clap(short, long)]
    pub quiet: bool,
    /// Whether messages should use color output.
    #[clap(long)]
    pub color: Option<String>,
}

pub fn main() -> eyre::Result<()> {
    color_eyre::config::HookBuilder::new()
        .display_env_section(false)
        .install()?;

    Ok(())
}


// TODO(ahuszagh) Need these..
// TODO(ahuszagh) Need a toml config file.

//pub fn cargo_metadata(msg_info: &mut MessageInfo) -> cross::Result<cross::CargoMetadata> {
//    cross::cargo_metadata_with_args(Some(Path::new(env!("CARGO_MANIFEST_DIR"))), None, msg_info)?
//        .ok_or_else(|| eyre::eyre!("could not find cross workspace"))
//}
//
//pub fn project_dir(msg_info: &mut MessageInfo) -> cross::Result<PathBuf> {
//    Ok(cargo_metadata(msg_info)?.workspace_root)
//}
