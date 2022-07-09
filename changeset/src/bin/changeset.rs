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

use clap::{Args, CommandFactory, Parser, Subcommand};
use eyre::Result;

pub fn main() -> Result<()> {
    color_eyre::config::HookBuilder::new()
        .display_env_section(false)
        .install()?;

    // TODO(ahuszagh) Here...

    Ok(())
}

// TODO(ahuszagh) Need the channel.
// TODO(ahuszagh) Need the manifest path

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
    /// The manifest path for the configuration file.
    #[clap(long)]
    manifest_path: String,
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
    /// The manifest path for the configuration file.
    #[clap(long)]
    manifest_path: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Build the changelog.
    BuildChangelog(BuildChangelog),
    /// Validate changelog entries.
    ValidateChangelog(ValidateChangelog),
}

#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Cli {
    /// Toolchain name/version to use (such as stable or 1.59.0).
    #[clap(value_parser = is_toolchain)]
    toolchain: Option<String>,
    #[clap(subcommand)]
    command: Commands,
}

// hidden implied parser so we can get matches without recursion.
#[derive(Parser, Debug)]
struct CliHidden {
    #[clap(subcommand)]
    command: Commands,
}

fn is_toolchain(toolchain: &str) -> Result<String> {
    if toolchain.starts_with('+') {
        Ok(toolchain.chars().skip(1).collect())
    } else {
        let _ = <CliHidden as CommandFactory>::command().get_matches();
        unreachable!();
    }
}
