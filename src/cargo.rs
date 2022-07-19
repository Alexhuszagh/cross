use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::cli::Args;
use crate::docker::{self, Image};
use crate::errors::*;
use crate::extensions::CommandExt;
use crate::file;
use crate::shell::{self, MessageInfo};
use crate::Target;
use once_cell::sync::OnceCell;
use serde::Deserialize;

type SubcommandSet = BTreeSet<String>;
type SubcommandMap = BTreeMap<String, SubcommandSet>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subcommand {
    Build,
    Check,
    Doc,
    Run,
    Rustc,
    Test,
    Bench,
    Clippy,
    Metadata,
    List,
    Clean,
    // both are custom subcommands, however, `Other` is not a
    // registered or known custom subcommand, while `Custom` is.
    Other(String),
    Custom(String),
}

impl Subcommand {
    #[must_use]
    pub fn needs_docker(&self, is_remote: bool) -> bool {
        match self {
            Subcommand::Other(_) | Subcommand::List => false,
            Subcommand::Clean if !is_remote => false,
            _ => true,
        }
    }

    #[must_use]
    pub fn needs_host(&self, is_remote: bool) -> bool {
        matches!(self, Subcommand::Clean) && is_remote
    }

    #[must_use]
    pub fn needs_interpreter(&self) -> bool {
        matches!(self, Subcommand::Run | Subcommand::Test | Subcommand::Bench)
    }

    #[must_use]
    pub fn needs_target_in_command(&self) -> bool {
        !matches!(self, Subcommand::Metadata)
    }

    pub fn known() -> &'static SubcommandSet {
        static INSTANCE: OnceCell<SubcommandSet> = OnceCell::new();
        INSTANCE.get_or_init(|| {
            [
                "asm",
                "audit",
                "binutils",
                "deb",
                "deny",
                "deps",
                "emit",
                "expand",
                "generate",
                "hack",
                "llvm-cov",
                "outdated",
                "release",
                "tarpaulin",
                "tree",
                "udeps",
                "when",
            ]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect()
        })
    }

    fn installed_cell() -> &'static mut OnceCell<SubcommandMap> {
        static mut INSTANCE: OnceCell<SubcommandMap> = OnceCell::new();
        // SAFETY: safe since OnceCell is thread-safe.
        unsafe { &mut INSTANCE }
    }

    fn json_path() -> Result<PathBuf> {
        Ok(file::cargo_dir()?.join("subcommands.json"))
    }

    // parse the installed subcommands, if present
    pub fn installed() -> Result<&'static SubcommandMap> {
        Self::installed_cell().get_or_try_init(|| {
            let path = Self::json_path()?;
            if path.exists() {
                let contents = fs::read_to_string(&path)
                    .wrap_err_with(|| eyre::eyre!("cannot find file {path:?}"))?;
                serde_json::from_str(&contents).map_err(Into::into)
            } else {
                Ok(BTreeMap::new())
            }
        })
    }

    // install a subcommand and register the subcommand
    pub fn install(
        engine: &docker::Engine,
        subcommand: &str,
        dirs: &docker::Directories,
        target: &Target,
        image: &Image,
        msg_info: &mut MessageInfo,
    ) -> Result<()> {
        if Self::is_installed(target, subcommand)? {
            return Ok(());
        }
        let map = Self::installed_cell()
            .get_mut()
            .ok_or_else(|| eyre::eyre!("installed subcommands not previously initialized."))?;
        docker::install_subcommand(engine, subcommand, dirs, target, image, msg_info)?;
        if !map.contains_key(&*target.triple()) {
            map.insert(target.triple().to_owned(), SubcommandSet::new());
        }
        let set = map
            .get_mut(&*target.triple())
            .ok_or_else(|| eyre::eyre!("subcommand map must contain target triple."))?;
        // TODO(ahuszagh) This needs to be specific for the toolchain. Fuck.
        set.insert(target.triple().to_owned());

        let json = serde_json::to_string(&map)?;
        let mut file = file::write_file(&Self::json_path()?, true)?;
        file.write_all(json.as_bytes())?;

        Ok(())
    }

    pub fn is_known(subcommand: &str) -> bool {
        Self::known().contains(subcommand)
    }

    pub fn is_installed(target: &Target, subcommand: &str) -> Result<bool> {
        Ok(Self::installed()?
            .get(target.triple())
            .and_then(|s| s.get(subcommand))
            .is_some())
    }
}

impl<'a> From<&'a str> for Subcommand {
    fn from(s: &str) -> Subcommand {
        match s {
            "b" | "build" => Subcommand::Build,
            "c" | "check" => Subcommand::Check,
            "clean" => Subcommand::Clean,
            "doc" => Subcommand::Doc,
            "r" | "run" => Subcommand::Run,
            "rustc" => Subcommand::Rustc,
            "t" | "test" => Subcommand::Test,
            "bench" => Subcommand::Bench,
            "clippy" => Subcommand::Clippy,
            "metadata" => Subcommand::Metadata,
            "--list" => Subcommand::List,
            _ => Subcommand::Other(s.to_owned()),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CargoMetadata {
    pub workspace_root: PathBuf,
    pub target_directory: PathBuf,
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
}

impl CargoMetadata {
    fn non_workspace_members(&self) -> impl Iterator<Item = &Package> {
        self.packages
            .iter()
            .filter(|p| !self.workspace_members.iter().any(|m| m == &p.id))
    }

    pub fn path_dependencies(&self) -> impl Iterator<Item = &Path> {
        // TODO: Also filter out things that are in workspace, but not a workspace member
        self.non_workspace_members().filter_map(|p| p.crate_path())
    }

    #[cfg(feature = "dev")]
    #[must_use]
    pub fn get_package(&self, package: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.name == package)
    }
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub manifest_path: PathBuf,
    pub source: Option<String>,
    pub version: String,
    pub license: Option<String>,
}

impl Package {
    /// Returns the absolute path to the packages manifest "folder"
    fn crate_path(&self) -> Option<&Path> {
        // when source is none, this package is a path dependency or a workspace member
        if self.source.is_none() {
            self.manifest_path.parent()
        } else {
            None
        }
    }
}

#[must_use]
pub fn cargo_command() -> Command {
    Command::new("cargo")
}

/// Cargo metadata with specific invocation
pub fn cargo_metadata_with_args(
    cd: Option<&Path>,
    args: Option<&Args>,
    msg_info: &mut MessageInfo,
) -> Result<Option<CargoMetadata>> {
    let mut command = cargo_command();
    if let Some(channel) = args.and_then(|x| x.channel.as_deref()) {
        command.arg(format!("+{channel}"));
    }
    command.arg("metadata").args(&["--format-version", "1"]);
    if let Some(cd) = cd {
        command.current_dir(cd);
    }
    if let Some(config) = args {
        if let Some(ref manifest_path) = config.manifest_path {
            command.args(["--manifest-path".as_ref(), manifest_path.as_os_str()]);
        }
    } else {
        command.arg("--no-deps");
    }
    if let Some(target) = args.and_then(|a| a.target.as_ref()) {
        command.args(["--filter-platform", target.triple()]);
    }
    if let Some(features) = args.map(|a| &a.features).filter(|v| !v.is_empty()) {
        command.args([String::from("--features"), features.join(",")]);
    }
    let output = command.run_and_get_output(msg_info)?;
    if !output.status.success() {
        msg_info.warn("unable to get metadata for package")?;
        let indented = shell::indent(&String::from_utf8(output.stderr)?, shell::default_ident());
        msg_info.debug(indented)?;
        return Ok(None);
    }
    let manifest: Option<CargoMetadata> = serde_json::from_slice(&output.stdout)?;
    manifest
        .map(|m| -> Result<_> {
            Ok(CargoMetadata {
                target_directory: args
                    .and_then(|a| a.target_dir.clone())
                    .unwrap_or(m.target_directory),
                ..m
            })
        })
        .transpose()
}

/// Pass-through mode
pub fn run(args: &[String], msg_info: &mut MessageInfo) -> Result<ExitStatus> {
    cargo_command()
        .args(args)
        .run_and_get_status(msg_info, false)
}

/// run cargo and get the output, does not check the exit status
pub fn run_and_get_output(
    args: &[String],
    msg_info: &mut MessageInfo,
) -> Result<std::process::Output> {
    cargo_command().args(args).run_and_get_output(msg_info)
}
