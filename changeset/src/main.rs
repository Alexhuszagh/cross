use std::cmp;
use std::fmt;
use std::fs;
use std::path::Path;

use clap::Args;

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

pub fn main() {}


// the type for the identifier: if it's a PR, sort
// by the number, otherwise, sort as 0. the numbers
// should be sorted, and the `max(values) || 0` should
// be used
#[derive(Debug, Clone, PartialEq, Eq)]
enum IdType {
    PullRequest(Vec<u64>),
    Issue(Vec<u64>),
}

// TODO(ahuszagh) Should have a validator or parser type

impl IdType {
    fn numbers(&self) -> &[u64] {
        match self {
            IdType::PullRequest(v) => v,
            IdType::Issue(v) => v,
        }
    }

    fn max_number(&self) -> u64 {
        self.numbers().iter().max().map_or_else(|| 0, |v| *v)
    }

    // TODO(ahuszagh) Probably need a commit-based formatter.

    fn parse_stem(file_stem: &str) -> cross::Result<IdType> {
        let (is_issue, rest) = match file_stem.strip_prefix("issue") {
            Some(n) => (true, n),
            None => (false, file_stem),
        };
        let mut numbers = rest
            .split('-')
            .map(|x| x.parse::<u64>())
            .collect::<Result<Vec<u64>, _>>()?;
        numbers.sort_unstable();

        Ok(match is_issue {
            false => IdType::PullRequest(numbers),
            true => IdType::Issue(numbers),
        })
    }

    fn parse_changelog(prs: &str) -> cross::Result<IdType> {
        let mut numbers = prs
            .split(',')
            .map(|x| x.trim().parse::<u64>())
            .collect::<Result<Vec<u64>, _>>()?;
        numbers.sort_unstable();

        Ok(IdType::PullRequest(numbers))
    }
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
