use std::path::{Path, PathBuf};
use std::process::Command;

use rustc_version::{Channel, Version};

use crate::errors::*;
pub use crate::extensions::{CommandExt, OutputExt};
use crate::Target;

#[derive(Debug)]
pub struct AvailableTargets {
    default: String,
    installed: Vec<String>,
    not_installed: Vec<String>,
}

impl AvailableTargets {
    pub fn contains(&self, target: &Target) -> bool {
        let triple = target.triple();
        self.is_installed(target) || self.not_installed.iter().any(|x| x == triple)
    }

    pub fn is_installed(&self, target: &Target) -> bool {
        let target = target.triple();
        target == self.default || self.installed.iter().any(|x| x == target)
    }
}

pub fn installed_toolchains(verbose: bool) -> Result<Vec<String>> {
    let out = Command::new("rustup")
        .args(&["toolchain", "list"])
        .run_and_get_stdout(verbose)?;

    Ok(out
        .lines()
        .map(|l| {
            l.replace(" (default)", "")
                .replace(" (override)", "")
                .trim()
                .to_owned()
        })
        .collect())
}

pub fn available_targets(toolchain: &str, verbose: bool) -> Result<AvailableTargets> {
    let mut cmd = Command::new("rustup");
    cmd.args(&["target", "list", "--toolchain", toolchain]);
    let output = cmd
        .run_and_get_output(verbose)
        .suggestion("is rustup installed?")?;

    if !output.status.success() {
        if String::from_utf8_lossy(&output.stderr).contains("is a custom toolchain") {
            eyre::bail!("{toolchain} is a custom toolchain. To use it, you'll need to set the environment variable `CROSS_CUSTOM_TOOLCHAIN=1`")
        }
        return Err(cmd
            .status_result(verbose, output.status, Some(&output))
            .unwrap_err()
            .to_section_report());
    }
    let out = output.stdout()?;
    let mut default = String::new();
    let mut installed = vec![];
    let mut not_installed = vec![];

    for line in out.lines() {
        let target = line.split(' ').next().unwrap().to_string();
        if line.contains("(default)") {
            assert!(default.is_empty());
            default = target;
        } else if line.contains("(installed)") {
            installed.push(target)
        } else {
            not_installed.push(target)
        }
    }

    Ok(AvailableTargets {
        default,
        installed,
        not_installed,
    })
}

pub fn install_toolchain(toolchain: &str, verbose: bool) -> Result<()> {
    Command::new("rustup")
        .args(&["toolchain", "add", toolchain, "--profile", "minimal"])
        .run(verbose, false)
        .wrap_err_with(|| format!("couldn't install toolchain `{toolchain}`"))
}

pub fn install(target: &Target, toolchain: &str, verbose: bool) -> Result<()> {
    let target = target.triple();

    Command::new("rustup")
        .args(&["target", "add", target, "--toolchain", toolchain])
        .run(verbose, false)
        .wrap_err_with(|| format!("couldn't install `std` for {target}"))
}

pub fn install_component(component: &str, toolchain: &str, verbose: bool) -> Result<()> {
    Command::new("rustup")
        .args(&["component", "add", component, "--toolchain", toolchain])
        .run(verbose, false)
        .wrap_err_with(|| format!("couldn't install the `{component}` component"))
}

pub fn component_is_installed(component: &str, toolchain: &str, verbose: bool) -> Result<bool> {
    Ok(Command::new("rustup")
        .args(&["component", "list", "--toolchain", toolchain])
        .run_and_get_stdout(verbose)?
        .lines()
        .any(|l| l.starts_with(component) && l.contains("installed")))
}

fn rustc_channel(version: &Version) -> Result<Channel> {
    match version.pre.split('.').next().unwrap() {
        "" => Ok(Channel::Stable),
        "dev" => Ok(Channel::Dev),
        "beta" => Ok(Channel::Beta),
        "nightly" => Ok(Channel::Nightly),
        x => eyre::bail!("unknown prerelease tag {x}"),
    }
}

fn multirust_channel_manifest_path(toolchain_path: &Path) -> PathBuf {
    toolchain_path.join("lib/rustlib/multirust-channel-manifest.toml")
}

pub fn rustc_version_string(toolchain_path: &Path) -> Result<Option<String>> {
    let path = multirust_channel_manifest_path(toolchain_path);
    if path.exists() {
        let contents =
            std::fs::read(&path).wrap_err_with(|| format!("couldn't open file `{path:?}`"))?;
        let manifest: toml::value::Table = toml::from_slice(&contents)?;
        return Ok(manifest
            .get("pkg")
            .and_then(|pkg| pkg.get("rust"))
            .and_then(|rust| rust.get("version"))
            .and_then(|version| version.as_str())
            .map(|version| version.to_string()));
    }
    Ok(None)
}

pub fn rustc_version(toolchain_path: &Path) -> Result<Option<(Version, Channel, String)>> {
    let path = multirust_channel_manifest_path(toolchain_path);
    if let Some(rust_version) = rustc_version_string(toolchain_path)? {
        // Field is `"{version} ({commit} {date})"`
        if let Some((version, meta)) = rust_version.split_once(' ') {
            let version = Version::parse(version)
                .wrap_err_with(|| format!("invalid rust version found in {path:?}"))?;
            let channel = rustc_channel(&version)?;
            return Ok(Some((version, channel, meta.to_owned())));
        }
    }
    Ok(None)
}
