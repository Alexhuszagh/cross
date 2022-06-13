use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::{env, fs};

use crate::cargo::CargoMetadata;
use crate::config::bool_from_envvar;
use crate::errors::*;
use crate::extensions::{CommandExt, SafeCommand};
use crate::file::{self, write_file};
use crate::id;
use crate::rustc;
use crate::{Config, Target};
use atty::Stream;
use eyre::bail;

pub const CROSS_IMAGE: &str = "ghcr.io/cross-rs";
const DOCKER_IMAGES: &[&str] = &include!(concat!(env!("OUT_DIR"), "/docker-images.rs"));
const DOCKER: &str = "docker";
const PODMAN: &str = "podman";
// secured profile based off the docker documentation for denied syscalls:
// https://docs.docker.com/engine/security/seccomp/#significant-syscalls-blocked-by-the-default-profile
// note that we've allow listed `clone` and `clone3`, which is necessary
// to fork the process, and which podman allows by default.
const SECCOMP: &str = include_str!("seccomp.json");

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EngineType {
    Docker,
    Podman,
    PodmanRemote,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    pub kind: EngineType,
    pub path: PathBuf,
    pub is_remote: bool,
}

impl Engine {
    pub fn new(is_remote: bool, verbose: bool) -> Result<Engine> {
        let path = get_container_engine()
            .map_err(|_| eyre::eyre!("no container engine found"))
            .with_suggestion(|| "is docker or podman installed?")?;
        Self::from_path(path, is_remote, verbose)
    }

    pub fn from_path(path: PathBuf, is_remote: bool, verbose: bool) -> Result<Engine> {
        let kind = get_engine_type(&path, verbose)?;
        Ok(Engine {
            path,
            kind,
            is_remote,
        })
    }

    pub fn needs_remote(&self) -> bool {
        self.is_remote && self.kind == EngineType::Podman
    }
}

struct DeleteVolume<'a>(&'a Engine, &'a VolumeId, bool);

impl<'a> Drop for DeleteVolume<'a> {
    fn drop(&mut self) {
        if let VolumeId::Discard(id) = self.1 {
            volume_rm(self.0, id, self.2).ok();
        }
    }
}

struct DeleteContainer<'a>(&'a Engine, &'a str, bool);

impl<'a> Drop for DeleteContainer<'a> {
    fn drop(&mut self) {
        container_stop(self.0, self.1, self.2).ok();
        container_rm(self.0, self.1, self.2).ok();
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Dead,
    Exited,
    DoesNotExist,
}

impl ContainerState {
    pub fn new(state: &str) -> Result<Self> {
        match state {
            "created" => Ok(ContainerState::Created),
            "running" => Ok(ContainerState::Running),
            "paused" => Ok(ContainerState::Paused),
            "restarting" => Ok(ContainerState::Restarting),
            "dead" => Ok(ContainerState::Dead),
            "exited" => Ok(ContainerState::Exited),
            "" => Ok(ContainerState::DoesNotExist),
            _ => eyre::bail!("unknown container state: got {state}"),
        }
    }

    pub fn is_stopped(&self) -> bool {
        matches!(self, Self::Exited | Self::DoesNotExist)
    }

    pub fn exists(&self) -> bool {
        !matches!(self, Self::DoesNotExist)
    }
}

// determine if the container engine is docker. this fixes issues with
// any aliases (#530), and doesn't fail if an executable suffix exists.
fn get_engine_type(ce: &Path, verbose: bool) -> Result<EngineType> {
    let stdout = Command::new(ce)
        .arg("--help")
        .run_and_get_stdout(verbose)?
        .to_lowercase();

    if stdout.contains("podman-remote") {
        Ok(EngineType::PodmanRemote)
    } else if stdout.contains("podman") {
        Ok(EngineType::Podman)
    } else if stdout.contains("docker") && !stdout.contains("emulate") {
        Ok(EngineType::Docker)
    } else {
        Ok(EngineType::Other)
    }
}

pub fn get_container_engine() -> Result<PathBuf, which::Error> {
    if let Ok(ce) = env::var("CROSS_CONTAINER_ENGINE") {
        which::which(ce)
    } else {
        which::which(DOCKER).or_else(|_| which::which(PODMAN))
    }
}

pub fn docker_command(engine: &Engine) -> Command {
    let mut command = Command::new(&engine.path);
    if engine.needs_remote() {
        // if we're using podman and not podman-remote, need `--remote`.
        command.arg("--remote");
    }
    command
}

pub fn docker_subcommand(engine: &Engine, subcommand: &str) -> Command {
    let mut command = docker_command(engine);
    command.arg(subcommand);
    command
}

/// Register binfmt interpreters
pub fn register(target: &Target, is_remote: bool, verbose: bool) -> Result<()> {
    let cmd = if target.is_windows() {
        // https://www.kernel.org/doc/html/latest/admin-guide/binfmt-misc.html
        "mount binfmt_misc -t binfmt_misc /proc/sys/fs/binfmt_misc && \
            echo ':wine:M::MZ::/usr/bin/run-detectors:' > /proc/sys/fs/binfmt_misc/register"
    } else {
        "apt-get update && apt-get install --no-install-recommends --assume-yes \
            binfmt-support qemu-user-static"
    };

    let engine = Engine::new(is_remote, verbose)?;
    docker_subcommand(&engine, "run")
        .args(&["--userns", "host"])
        .arg("--privileged")
        .arg("--rm")
        .arg("ubuntu:16.04")
        .args(&["sh", "-c", cmd])
        .run(verbose)
}

fn validate_env_var(var: &str) -> Result<(&str, Option<&str>)> {
    let (key, value) = match var.split_once('=') {
        Some((key, value)) => (key, Some(value)),
        _ => (var, None),
    };

    if key == "CROSS_RUNNER" {
        bail!("CROSS_RUNNER environment variable name is reserved and cannot be pass through");
    }

    Ok((key, value))
}

fn parse_docker_opts(value: &str) -> Result<Vec<String>> {
    shell_words::split(value).wrap_err_with(|| format!("could not parse docker opts of {}", value))
}

#[derive(Debug)]
pub struct Directories {
    pub cargo: PathBuf,
    pub xargo: PathBuf,
    pub target: PathBuf,
    pub nix_store: Option<PathBuf>,
    pub host_root: PathBuf,
    pub mount_root: PathBuf,
    pub mount_cwd: PathBuf,
    pub sysroot: PathBuf,
}

impl Directories {
    #[allow(unused_variables)]
    pub fn create(
        metadata: &CargoMetadata,
        cwd: &Path,
        sysroot: &Path,
        docker_in_docker: bool,
        verbose: bool,
    ) -> Result<Self> {
        let mount_finder = if docker_in_docker {
            MountFinder::new(docker_read_mount_paths()?)
        } else {
            MountFinder::default()
        };
        let home_dir =
            home::home_dir().ok_or_else(|| eyre::eyre!("could not find home directory"))?;
        let cargo = home::cargo_home()?;
        let xargo = env::var_os("XARGO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir.join(".xargo"));
        let nix_store = env::var_os("NIX_STORE").map(PathBuf::from);
        let target = &metadata.target_directory;

        // create the directories we are going to mount before we mount them,
        // otherwise `docker` will create them but they will be owned by `root`
        fs::create_dir(&cargo).ok();
        fs::create_dir(&xargo).ok();
        fs::create_dir(&target).ok();

        let cargo = mount_finder.find_mount_path(cargo);
        let xargo = mount_finder.find_mount_path(xargo);
        let target = mount_finder.find_mount_path(target);

        // root is either workspace_root, or, if we're outside the workspace root, the current directory
        let host_root = mount_finder.find_mount_path(if metadata.workspace_root.starts_with(cwd) {
            cwd
        } else {
            &metadata.workspace_root
        });

        // root is either workspace_root, or, if we're outside the workspace root, the current directory
        let mount_root: PathBuf;
        #[cfg(target_os = "windows")]
        {
            // On Windows, we can not mount the directory name directly. Instead, we use wslpath to convert the path to a linux compatible path.
            mount_root = wslpath(&host_root, verbose)?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            mount_root = mount_finder.find_mount_path(host_root.clone());
        }
        let mount_cwd: PathBuf;
        #[cfg(target_os = "windows")]
        {
            // On Windows, we can not mount the directory name directly. Instead, we use wslpath to convert the path to a linux compatible path.
            mount_cwd = wslpath(cwd, verbose)?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            mount_cwd = mount_finder.find_mount_path(cwd);
        }
        let sysroot = mount_finder.find_mount_path(sysroot);

        Ok(Directories {
            cargo,
            xargo,
            target,
            nix_store,
            host_root,
            mount_root,
            mount_cwd,
            sysroot,
        })
    }
}

#[derive(Debug)]
enum VolumeId {
    Keep(String),
    Discard(String),
}

impl VolumeId {
    fn create(engine: &Engine, container: &str, verbose: bool) -> Result<Self> {
        let keep_id = format!("{container}-keep");
        if volume_exists(engine, &keep_id, verbose)? {
            Ok(Self::Keep(keep_id))
        } else {
            Ok(Self::Discard(container.to_string()))
        }
    }
}

impl AsRef<str> for VolumeId {
    fn as_ref(&self) -> &str {
        match self {
            Self::Keep(s) => s,
            Self::Discard(s) => s,
        }
    }
}

fn cargo_cmd(uses_xargo: bool) -> SafeCommand {
    if uses_xargo {
        SafeCommand::new("xargo")
    } else {
        SafeCommand::new("cargo")
    }
}

#[allow(unused_variables)]
fn canonicalize_mount_path(path: &Path, verbose: bool) -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // On Windows, we can not mount the directory name directly. Instead, we use wslpath to convert the path to a linux compatible path.
        wslpath(path, verbose)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(path.to_path_buf())
    }
}

fn remote_mount_path(val: &Path, verbose: bool) -> Result<PathBuf> {
    let host_path = file::canonicalize(val)?;
    canonicalize_mount_path(&host_path, verbose)
}

fn mount(docker: &mut Command, val: &Path, prefix: &str, verbose: bool) -> Result<PathBuf> {
    let host_path = file::canonicalize(val)?;
    let mount_path = canonicalize_mount_path(&host_path, verbose)?;
    docker.args(&[
        "-v",
        &format!("{}:{prefix}{}", host_path.display(), mount_path.display()),
    ]);
    Ok(mount_path)
}

fn create_volume_dir(
    engine: &Engine,
    container: &str,
    dir: &Path,
    verbose: bool,
) -> Result<ExitStatus> {
    // make our parent directory if needed
    docker_subcommand(engine, "exec")
        .arg(container)
        .args(&["sh", "-c", &format!("mkdir -p '{}'", dir.display())])
        .run_and_get_status(verbose)
}

// copy files for a docker volume, for remote host support
fn copy_volume_files(
    engine: &Engine,
    container: &str,
    src: &Path,
    dst: &Path,
    verbose: bool,
) -> Result<ExitStatus> {
    docker_subcommand(engine, "cp")
        .arg("-a")
        .arg(&src.display().to_string())
        .arg(format!("{container}:{}", dst.display()))
        .run_and_get_status(verbose)
}

fn is_cachedir_tag(path: &Path) -> Result<bool> {
    let mut buffer = [b'0'; 43];
    let mut file = fs::OpenOptions::new().read(true).open(path)?;
    file.read_exact(&mut buffer)?;

    Ok(&buffer == b"Signature: 8a477f597d28d172789f06886806bc55")
}

fn is_cachedir(entry: &fs::DirEntry) -> bool {
    // avoid any cached directories when copying
    // see https://bford.info/cachedir/
    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
        let path = entry.path().join("CACHEDIR.TAG");
        path.exists() && is_cachedir_tag(&path).unwrap_or(false)
    } else {
        false
    }
}

// copy files for a docker volume, for remote host support
fn copy_volume_files_nocache(
    engine: &Engine,
    container: &str,
    src: &Path,
    dst: &Path,
    verbose: bool,
) -> Result<ExitStatus> {
    // avoid any cached directories when copying
    // see https://bford.info/cachedir/
    let tempdir = tempfile::tempdir()?;
    let temppath = tempdir.path();
    copy_dir(src, temppath, 0, |e, _| !is_cachedir(e))?;
    copy_volume_files(engine, container, temppath, dst, verbose)
}

pub fn copy_volume_xargo(
    engine: &Engine,
    container: &str,
    xargo_dir: &Path,
    target: &Target,
    mount_prefix: &Path,
    verbose: bool,
) -> Result<()> {
    // only need to copy the rustlib files for our current target.
    let triple = target.triple();
    let relpath = Path::new("lib").join("rustlib").join(&triple);
    let src = xargo_dir.join(&relpath);
    let dst = mount_prefix.join("xargo").join(&relpath);
    if Path::new(&src).exists() {
        create_volume_dir(engine, container, dst.parent().unwrap(), verbose)?;
        copy_volume_files(engine, container, &src, &dst, verbose)?;
    }

    Ok(())
}

pub fn copy_volume_cargo(
    engine: &Engine,
    container: &str,
    cargo_dir: &Path,
    mount_prefix: &Path,
    copy_registry: bool,
    verbose: bool,
) -> Result<()> {
    let dst = mount_prefix.join("cargo");
    let copy_registry = env::var("CROSS_REMOTE_COPY_REGISTRY")
        .map(|s| bool_from_envvar(&s))
        .unwrap_or(copy_registry);

    if copy_registry {
        copy_volume_files(engine, container, cargo_dir, &dst, verbose)?;
    } else {
        // can copy a limit subset of files: the rest is present.
        create_volume_dir(engine, container, &dst, verbose)?;
        for entry in fs::read_dir(cargo_dir)? {
            let file = entry?;
            let basename = file.file_name().to_string_lossy().into_owned();
            if !basename.starts_with('.') && !matches!(basename.as_ref(), "git" | "registry") {
                copy_volume_files(engine, container, &file.path(), &dst, verbose)?;
            }
        }
    }

    Ok(())
}

// recursively copy a directory into another
fn copy_dir<Skip>(src: &Path, dst: &Path, depth: u32, skip: Skip) -> Result<()>
where
    Skip: Copy + Fn(&fs::DirEntry, u32) -> bool,
{
    for entry in fs::read_dir(src)? {
        let file = entry?;
        if skip(&file, depth) {
            continue;
        }

        let src_path = file.path();
        let dst_path = dst.join(file.file_name());
        if file.file_type()?.is_file() {
            fs::copy(&src_path, &dst_path)?;
        } else {
            fs::create_dir(&dst_path).ok();
            copy_dir(&src_path, &dst_path, depth + 1, skip)?;
        }
    }

    Ok(())
}

pub fn copy_volume_rust(
    engine: &Engine,
    container: &str,
    sysroot: &Path,
    target: &Target,
    mount_prefix: &Path,
    verbose: bool,
) -> Result<()> {
    // the rust toolchain is quite large, but most of it isn't needed
    // we need the bin, libexec, and etc directories, and part of the lib directory.
    let dst = mount_prefix.join("rust");
    create_volume_dir(engine, container, &dst, verbose)?;
    for basename in ["bin", "libexec", "etc"] {
        let file = sysroot.join(basename);
        copy_volume_files(engine, container, &file, &dst, verbose)?;
    }

    // the lib directories are rather large, so we want only a subset.
    // now, we use a temp directory for everything else in the libdir
    // we can pretty safely assume we don't have symlinks here.
    let rustlib = Path::new("lib").join("rustlib");
    let src_rustlib = sysroot.join(&rustlib);
    let dst_rustlib = dst.join(&rustlib);

    let tempdir = tempfile::tempdir()?;
    let temppath = tempdir.path();
    copy_dir(&sysroot.join("lib"), temppath, 0, |e, d| {
        d == 0 && e.file_name() == "rustlib"
    })?;
    fs::create_dir(&temppath.join("rustlib")).ok();
    copy_dir(
        &src_rustlib,
        &temppath.join("rustlib"),
        0,
        |entry, depth| {
            if depth != 0 {
                return false;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => return true,
            };
            let file_name = entry.file_name();
            !(file_type.is_file() || file_name == "src" || file_name == "etc")
        },
    )?;
    copy_volume_files(engine, container, temppath, &dst.join("lib"), verbose)?;
    // must make the `dst.join("lib")` **after** here, or we copy temp into lib.
    create_volume_dir(engine, container, &dst_rustlib, verbose)?;

    // we first copy over the toolchain file, then everything besides it.
    // since we don't want to call docker 100x, we copy the intermediate
    // files to a temp directory so they're cleaned up afterwards.
    let toolchain_path = src_rustlib.join(&target.triple());
    if toolchain_path.exists() {
        copy_volume_files(engine, container, &toolchain_path, &dst_rustlib, verbose)?;
    }

    // now we need to copy over the host toolchain too, since it has
    // some requirements to find std libraries, etc.
    let rustc = sysroot.join("bin").join("rustc");
    let libdir = Command::new(rustc)
        .args(&["--print", "target-libdir"])
        .run_and_get_stdout(verbose)?;
    let host_toolchain_path = Path::new(libdir.trim()).parent().unwrap();
    copy_volume_files(
        engine,
        container,
        host_toolchain_path,
        &dst_rustlib,
        verbose,
    )?;

    Ok(())
}

pub fn volume_create(engine: &Engine, volume: &str, verbose: bool) -> Result<ExitStatus> {
    docker_subcommand(engine, "volume")
        .args(&["create", volume])
        .run_and_get_status(verbose)
}

pub fn volume_rm(engine: &Engine, volume: &str, verbose: bool) -> Result<ExitStatus> {
    docker_subcommand(engine, "volume")
        .args(&["rm", volume])
        .run_and_get_status(verbose)
}

pub fn volume_exists(engine: &Engine, volume: &str, verbose: bool) -> Result<bool> {
    let output = docker_subcommand(engine, "volume")
        .args(&["inspect", volume])
        .run_and_get_output(verbose)?;
    Ok(output.status.success())
}

pub fn container_stop(engine: &Engine, container: &str, verbose: bool) -> Result<ExitStatus> {
    docker_subcommand(engine, "stop")
        .arg(container)
        .run_and_get_status(verbose)
}

pub fn container_rm(engine: &Engine, container: &str, verbose: bool) -> Result<ExitStatus> {
    docker_subcommand(engine, "rm")
        .arg(container)
        .run_and_get_status(verbose)
}

pub fn container_state(engine: &Engine, container: &str, verbose: bool) -> Result<ContainerState> {
    let stdout = docker_subcommand(engine, "ps")
        .arg("-a")
        .args(&["--filter", &format!("name={container}")])
        .args(&["--format", "{{.State}}"])
        .run_and_get_stdout(verbose)?;
    ContainerState::new(stdout.trim())
}

fn path_hash(path: &Path) -> String {
    sha1_smol::Sha1::from(path.display().to_string().as_bytes())
        .digest()
        .to_string()
        .get(..5)
        .expect("sha1 is expected to be at least 5 characters long")
        .to_string()
}

pub fn remote_identifier(
    target: &Target,
    metadata: &CargoMetadata,
    dirs: &Directories,
) -> Result<String> {
    let host_version_meta = rustc::version_meta()?;
    let commit_hash = host_version_meta
        .commit_hash
        .unwrap_or(host_version_meta.short_version_string);

    let workspace_root = &metadata.workspace_root;
    let package = metadata
        .packages
        .iter()
        .find(|p| p.manifest_path.parent().unwrap() == workspace_root)
        .unwrap_or_else(|| metadata.packages.get(0).unwrap());

    let name = &package.name;
    let triple = target.triple();
    let project_hash = path_hash(&package.manifest_path);
    let toolchain_hash = path_hash(&dirs.sysroot);
    Ok(format!(
        "cross-{name}-{triple}-{project_hash}-{toolchain_hash}-{commit_hash}"
    ))
}

#[allow(unused_variables)]
fn docker_seccomp(
    docker: &mut Command,
    engine_type: EngineType,
    target: &Target,
    verbose: bool,
) -> Result<()> {
    // docker uses seccomp now on all installations
    if target.needs_docker_seccomp() {
        let seccomp = if engine_type == EngineType::Docker && cfg!(target_os = "windows") {
            // docker on windows fails due to a bug in reading the profile
            // https://github.com/docker/for-win/issues/12760
            "unconfined".to_string()
        } else {
            #[allow(unused_mut)] // target_os = "windows"
            let mut path = env::current_dir()
                .wrap_err("couldn't get current directory")?
                .canonicalize()
                .wrap_err_with(|| "when canonicalizing current_dir".to_string())?
                .join("target")
                .join(target.triple())
                .join("seccomp.json");
            if !path.exists() {
                write_file(&path, false)?.write_all(SECCOMP.as_bytes())?;
            }
            #[cfg(target_os = "windows")]
            if matches!(engine_type, EngineType::Podman | EngineType::PodmanRemote) {
                // podman weirdly expects a WSL path here, and fails otherwise
                path = wslpath(&path, verbose)?;
            }
            path.display().to_string()
        };

        docker.args(&["--security-opt", &format!("seccomp={}", seccomp)]);
    }

    Ok(())
}

fn user_id() -> String {
    env::var("CROSS_CONTAINER_UID").unwrap_or_else(|_| id::user().to_string())
}

fn group_id() -> String {
    env::var("CROSS_CONTAINER_GID").unwrap_or_else(|_| id::group().to_string())
}

fn docker_user_id(docker: &mut Command, engine_type: EngineType) {
    // We need to specify the user for Docker, but not for Podman.
    if engine_type == EngineType::Docker {
        docker.args(&["--user", &format!("{}:{}", user_id(), group_id(),)]);
    }
}

fn docker_envvars(docker: &mut Command, config: &Config, target: &Target) -> Result<()> {
    for ref var in config.env_passthrough(target)? {
        validate_env_var(var)?;

        // Only specifying the environment variable name in the "-e"
        // flag forwards the value from the parent shell
        docker.args(&["-e", var]);
    }

    let runner = config.runner(target)?;
    let cross_runner = format!("CROSS_RUNNER={}", runner.unwrap_or_default());
    docker
        .args(&["-e", "PKG_CONFIG_ALLOW_CROSS=1"])
        .args(&["-e", "XARGO_HOME=/xargo"])
        .args(&["-e", "CARGO_HOME=/cargo"])
        .args(&["-e", "CARGO_TARGET_DIR=/target"])
        .args(&["-e", &cross_runner]);

    if let Some(username) = id::username().unwrap() {
        docker.args(&["-e", &format!("USER={username}")]);
    }

    if let Ok(value) = env::var("QEMU_STRACE") {
        docker.args(&["-e", &format!("QEMU_STRACE={value}")]);
    }

    if let Ok(value) = env::var("CROSS_DEBUG") {
        docker.args(&["-e", &format!("CROSS_DEBUG={value}")]);
    }

    if let Ok(value) = env::var("CROSS_CONTAINER_OPTS") {
        if env::var("DOCKER_OPTS").is_ok() {
            eprintln!("Warning: using both `CROSS_CONTAINER_OPTS` and `DOCKER_OPTS`.");
        }
        docker.args(&parse_docker_opts(&value)?);
    } else if let Ok(value) = env::var("DOCKER_OPTS") {
        // FIXME: remove this when we deprecate DOCKER_OPTS.
        docker.args(&parse_docker_opts(&value)?);
    };

    Ok(())
}

#[allow(clippy::too_many_arguments)] // TODO: refactor
fn docker_mount(
    docker: &mut Command,
    metadata: &CargoMetadata,
    config: &Config,
    target: &Target,
    cwd: &Path,
    verbose: bool,
    mount_cb: impl Fn(&mut Command, &Path, bool) -> Result<PathBuf>,
    mut store_cb: impl FnMut((String, PathBuf)),
) -> Result<bool> {
    let mut mount_volumes = false;
    // FIXME(emilgardis 2022-04-07): This is a fallback so that if it's hard for us to do mounting logic, make it simple(r)
    // Preferably we would not have to do this.
    if cwd.strip_prefix(&metadata.workspace_root).is_err() {
        mount_volumes = true;
    }

    for ref var in config.env_volumes(target)? {
        let (var, value) = validate_env_var(var)?;
        let value = match value {
            Some(v) => Ok(v.to_string()),
            None => env::var(var),
        };

        if let Ok(val) = value {
            let mount_path = mount_cb(docker, val.as_ref(), verbose)?;
            docker.args(&["-e", &format!("{}={}", var, mount_path.display())]);
            store_cb((val, mount_path));
            mount_volumes = true;
        }
    }

    for path in metadata.path_dependencies() {
        let mount_path = mount_cb(docker, path, verbose)?;
        store_cb((path.display().to_string(), mount_path));
        mount_volumes = true;
    }

    Ok(mount_volumes)
}

fn docker_cwd(
    docker: &mut Command,
    metadata: &CargoMetadata,
    dirs: &Directories,
    cwd: &Path,
    mount_volumes: bool,
) -> Result<()> {
    if mount_volumes {
        docker.args(&["-w".as_ref(), dirs.mount_cwd.as_os_str()]);
    } else if dirs.mount_cwd == metadata.workspace_root {
        docker.args(&["-w", "/project"]);
    } else {
        // We do this to avoid clashes with path separators. Windows uses `\` as a path separator on Path::join
        let cwd = &cwd;
        let working_dir = Path::new("project").join(cwd.strip_prefix(&metadata.workspace_root)?);
        // No [T].join for OsStr
        let mut mount_wd = std::ffi::OsString::new();
        for part in working_dir.iter() {
            mount_wd.push("/");
            mount_wd.push(part);
        }
        docker.args(&["-w".as_ref(), mount_wd.as_os_str()]);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)] // TODO: refactor
fn remote_run(
    target: &Target,
    args: &[String],
    metadata: &CargoMetadata,
    config: &Config,
    uses_xargo: bool,
    sysroot: &Path,
    verbose: bool,
    docker_in_docker: bool,
    cwd: &Path,
) -> Result<ExitStatus> {
    let dirs = Directories::create(metadata, cwd, sysroot, docker_in_docker, verbose)?;

    let mut cmd = cargo_cmd(uses_xargo);
    cmd.args(args);

    let engine = Engine::new(true, verbose)?;
    let mount_prefix = "/cross";

    // the logic is broken into the following steps
    // 1. get our unique identifiers and cleanup from a previous run.
    // 2. create a data volume to store everything
    // 3. start our container with the data volume and all envvars
    // 4. copy all mounted volumes over
    // 5. create symlinks for all mounted data
    // 6. execute our cargo command inside the container
    // 7. copy data from target dir back to host
    // 8. stop container and delete data volume
    //
    // we use structs that wrap the resources to ensure they're dropped
    // in the correct order even on error, to ensure safe cleanup

    // 1. get our unique identifiers and cleanup from a previous run.
    // this can happen if we didn't gracefully exit before
    let container = remote_identifier(target, metadata, &dirs)?;
    let volume = VolumeId::create(&engine, &container, verbose)?;
    let state = container_state(&engine, &container, verbose)?;
    if !state.is_stopped() {
        eprintln!("warning: container {container} was running.");
        container_stop(&engine, &container, verbose)?;
    }
    if state.exists() {
        eprintln!("warning: container {container} was exited.");
        container_rm(&engine, &container, verbose)?;
    }
    if let VolumeId::Discard(ref id) = volume {
        if volume_exists(&engine, id, verbose)? {
            eprintln!("warning: temporary volume {container} existed.");
            volume_rm(&engine, id, verbose)?;
        }
    }

    // 2. create our volume to copy all our data over to
    if let VolumeId::Discard(ref id) = volume {
        volume_create(&engine, id, verbose)?;
    }
    let _volume_deletter = DeleteVolume(&engine, &volume, verbose);

    // 3. create our start container command here
    let mut docker = docker_subcommand(&engine, "run");
    docker.args(&["--userns", "host"]);
    docker.args(&["--name", &container]);
    docker.args(&["-v", &format!("{}:{mount_prefix}", volume.as_ref())]);
    docker_envvars(&mut docker, config, target)?;

    let mut volumes = vec![];
    let mount_volumes = docker_mount(
        &mut docker,
        metadata,
        config,
        target,
        cwd,
        verbose,
        |_, val, verbose| remote_mount_path(val, verbose),
        |(src, dst)| volumes.push((src, dst)),
    )?;

    docker_seccomp(&mut docker, engine.kind, target, verbose)?;

    // Prevent `bin` from being mounted inside the Docker container.
    docker.args(&["-v", &format!("{mount_prefix}/cargo/bin")]);

    // When running inside NixOS or using Nix packaging we need to add the Nix
    // Store to the running container so it can load the needed binaries.
    if let Some(ref nix_store) = dirs.nix_store {
        volumes.push((nix_store.display().to_string(), nix_store.to_path_buf()))
    }

    docker.arg("-d");
    if atty::is(Stream::Stdin) && atty::is(Stream::Stdout) && atty::is(Stream::Stderr) {
        docker.arg("-t");
    }

    docker
        .arg(&image(config, target)?)
        // ensure the process never exits until we stop it
        .args(&["sh", "-c", "sleep infinity"])
        .run_and_get_status(verbose)?;
    let _container_deletter = DeleteContainer(&engine, &container, verbose);

    // 4. copy all mounted volumes over
    let copy_cache = env::var("CROSS_REMOTE_COPY_CACHE")
        .map(|s| bool_from_envvar(&s))
        .unwrap_or_default();
    let copy = |src, dst: &PathBuf| {
        if copy_cache {
            copy_volume_files(&engine, &container, src, dst, verbose)
        } else {
            copy_volume_files_nocache(&engine, &container, src, dst, verbose)
        }
    };
    let mount_prefix_path = mount_prefix.as_ref();
    if let VolumeId::Discard(_) = volume {
        copy_volume_xargo(
            &engine,
            &container,
            &dirs.xargo,
            target,
            mount_prefix_path,
            verbose,
        )?;
        copy_volume_cargo(
            &engine,
            &container,
            &dirs.cargo,
            mount_prefix_path,
            false,
            verbose,
        )?;
        copy_volume_rust(
            &engine,
            &container,
            &dirs.sysroot,
            target,
            mount_prefix_path,
            verbose,
        )?;
    }
    let mount_root = if mount_volumes {
        // cannot panic: absolute unix path, must have root
        let rel_mount_root = dirs.mount_root.strip_prefix("/").unwrap();
        let mount_root = mount_prefix_path.join(rel_mount_root);
        if rel_mount_root != PathBuf::new() {
            create_volume_dir(&engine, &container, mount_root.parent().unwrap(), verbose)?;
        }
        mount_root
    } else {
        mount_prefix_path.join("project")
    };
    copy(&dirs.host_root, &mount_root)?;

    let mut copied = vec![
        (&dirs.xargo, mount_prefix_path.join("xargo")),
        (&dirs.cargo, mount_prefix_path.join("cargo")),
        (&dirs.sysroot, mount_prefix_path.join("rust")),
        (&dirs.host_root, mount_root.clone()),
    ];
    let mut to_symlink = vec![];
    let target_dir = file::canonicalize(&dirs.target)?;
    let target_dir = if let Ok(relpath) = target_dir.strip_prefix(&dirs.host_root) {
        // target dir is in the project, just symlink it in
        let target_dir = mount_root.join(relpath);
        to_symlink.push((target_dir.clone(), "/target".to_string()));
        target_dir
    } else {
        // outside project, need to copy the target data over
        // only do if we're copying over cached files.
        let target_dir = mount_prefix_path.join("target");
        if copy_cache {
            copy(&dirs.target, &target_dir)?;
        } else {
            create_volume_dir(&engine, &container, &target_dir, verbose)?;
        }

        copied.push((&dirs.target, target_dir.clone()));
        target_dir
    };
    for (src, dst) in volumes.iter() {
        let src: &Path = src.as_ref();
        if let Some((psrc, pdst)) = copied.iter().find(|(p, _)| src.starts_with(p)) {
            // path has already been copied over
            let relpath = src.strip_prefix(psrc).unwrap();
            to_symlink.push((pdst.join(relpath), dst.display().to_string()));
        } else {
            let rel_dst = dst.strip_prefix("/").unwrap();
            let mount_dst = mount_prefix_path.join(rel_dst);
            if rel_dst != PathBuf::new() {
                create_volume_dir(&engine, &container, mount_dst.parent().unwrap(), verbose)?;
            }
            copy(src, &mount_dst)?;
        }
    }

    // 5. create symlinks for copied data
    let mut symlink = vec!["set -e pipefail".to_string()];
    if verbose {
        symlink.push("set -x".to_string());
    }
    symlink.push(format!(
        "chown -R {uid}:{gid} {mount_prefix}/*",
        uid = user_id(),
        gid = group_id(),
    ));
    // need a simple script to add symlinks, but not override existing files.
    symlink.push(format!(
        "prefix=\"{mount_prefix}\"

symlink_recurse() {{
    for f in \"${{1}}\"/*; do
        dst=${{f#\"$prefix\"}}
        if [ -f \"${{dst}}\" ]; then
            echo \"invalid: got unexpected file at ${{dst}}\" 1>&2
            exit 1
        elif [ -d \"${{dst}}\" ]; then
            symlink_recurse \"${{f}}\"
        else
            ln -s \"${{f}}\" \"${{dst}}\"
        fi
    done
}}

symlink_recurse \"${{prefix}}\"
"
    ));
    for (src, dst) in to_symlink {
        symlink.push(format!("ln -s \"{}\" \"{}\"", src.display(), dst));
    }
    docker_subcommand(&engine, "exec")
        .arg(&container)
        .args(&["sh", "-c", &symlink.join("\n")])
        .run_and_get_status(verbose)?;

    // 6. execute our cargo command inside the container
    let mut docker = docker_subcommand(&engine, "exec");
    docker_user_id(&mut docker, engine.kind);
    docker_cwd(&mut docker, metadata, &dirs, cwd, mount_volumes)?;
    docker.arg(&container);
    docker.args(&["sh", "-c", &format!("PATH=$PATH:/rust/bin {:?}", cmd)]);
    let status = docker.run_and_get_status(verbose);

    // 7. copy data from our target dir back to host
    docker_subcommand(&engine, "cp")
        .arg("-a")
        .arg(&format!("{container}:{}", target_dir.display()))
        .arg(&dirs.target.parent().unwrap())
        .run_and_get_status(verbose)?;

    status
}

#[allow(clippy::too_many_arguments)] // TODO: refactor
fn local_run(
    target: &Target,
    args: &[String],
    metadata: &CargoMetadata,
    config: &Config,
    uses_xargo: bool,
    sysroot: &Path,
    verbose: bool,
    docker_in_docker: bool,
    cwd: &Path,
) -> Result<ExitStatus> {
    let dirs = Directories::create(metadata, cwd, sysroot, docker_in_docker, verbose)?;

    let mut cmd = cargo_cmd(uses_xargo);
    cmd.args(args);

    let engine = Engine::new(false, verbose)?;

    let mut docker = docker_subcommand(&engine, "run");
    docker.args(&["--userns", "host"]);
    docker_envvars(&mut docker, config, target)?;

    let mount_volumes = docker_mount(
        &mut docker,
        metadata,
        config,
        target,
        cwd,
        verbose,
        |docker, val, verbose| mount(docker, val, "", verbose),
        |_| {},
    )?;

    docker.arg("--rm");

    docker_seccomp(&mut docker, engine.kind, target, verbose)?;
    docker_user_id(&mut docker, engine.kind);

    docker
        .args(&["-v", &format!("{}:/xargo:Z", dirs.xargo.display())])
        .args(&["-v", &format!("{}:/cargo:Z", dirs.cargo.display())])
        // Prevent `bin` from being mounted inside the Docker container.
        .args(&["-v", "/cargo/bin"]);
    if mount_volumes {
        docker.args(&[
            "-v",
            &format!(
                "{}:{}:Z",
                dirs.host_root.display(),
                dirs.mount_root.display()
            ),
        ]);
    } else {
        docker.args(&["-v", &format!("{}:/project:Z", dirs.host_root.display())]);
    }
    docker
        .args(&["-v", &format!("{}:/rust:Z,ro", dirs.sysroot.display())])
        .args(&["-v", &format!("{}:/target:Z", dirs.target.display())]);
    docker_cwd(&mut docker, metadata, &dirs, cwd, mount_volumes)?;

    // When running inside NixOS or using Nix packaging we need to add the Nix
    // Store to the running container so it can load the needed binaries.
    if let Some(ref nix_store) = dirs.nix_store {
        docker.args(&[
            "-v",
            &format!("{}:{}:Z", nix_store.display(), nix_store.display()),
        ]);
    }

    if atty::is(Stream::Stdin) {
        docker.arg("-i");
        if atty::is(Stream::Stdout) && atty::is(Stream::Stderr) {
            docker.arg("-t");
        }
    }

    docker
        .arg(&image(config, target)?)
        .args(&["sh", "-c", &format!("PATH=$PATH:/rust/bin {:?}", cmd)])
        .run_and_get_status(verbose)
}

#[allow(clippy::too_many_arguments)] // TODO: refactor
pub fn run(
    target: &Target,
    args: &[String],
    metadata: &CargoMetadata,
    config: &Config,
    uses_xargo: bool,
    sysroot: &Path,
    verbose: bool,
    docker_in_docker: bool,
    is_remote: bool,
    cwd: &Path,
) -> Result<ExitStatus> {
    if is_remote {
        remote_run(
            target,
            args,
            metadata,
            config,
            uses_xargo,
            sysroot,
            verbose,
            docker_in_docker,
            cwd,
        )
    } else {
        local_run(
            target,
            args,
            metadata,
            config,
            uses_xargo,
            sysroot,
            verbose,
            docker_in_docker,
            cwd,
        )
    }
}

pub fn image(config: &Config, target: &Target) -> Result<String> {
    if let Some(image) = config.image(target)? {
        return Ok(image);
    }

    if !DOCKER_IMAGES.contains(&target.triple()) {
        bail!(
            "`cross` does not provide a Docker image for target {target}, \
               specify a custom image in `Cross.toml`."
        );
    }

    let version = if include_str!(concat!(env!("OUT_DIR"), "/commit-info.txt")).is_empty() {
        env!("CARGO_PKG_VERSION")
    } else {
        "main"
    };

    Ok(format!("{CROSS_IMAGE}/{target}:{version}"))
}

#[cfg(target_os = "windows")]
fn wslpath(path: &Path, verbose: bool) -> Result<PathBuf> {
    let wslpath = which::which("wsl.exe")
        .map_err(|_| eyre::eyre!("could not find wsl.exe"))
        .warning("usage of `env.volumes` requires WSL on Windows")
        .suggestion("is WSL installed on the host?")?;

    Command::new(wslpath)
        .arg("-e")
        .arg("wslpath")
        .arg("-a")
        .arg(path)
        .run_and_get_stdout(verbose)
        .wrap_err_with(|| {
            format!(
                "could not get linux compatible path for `{}`",
                path.display()
            )
        })
        .map(|s| s.trim().into())
}

fn docker_read_mount_paths() -> Result<Vec<MountDetail>> {
    let hostname = env::var("HOSTNAME").wrap_err("HOSTNAME environment variable not found")?;

    let docker_path = which::which(DOCKER)?;
    let mut docker: Command = {
        let mut command = Command::new(docker_path);
        command.arg("inspect");
        command.arg(hostname);
        command
    };

    let output = docker.run_and_get_stdout(false)?;
    let info = serde_json::from_str(&output).wrap_err("failed to parse docker inspect output")?;
    dockerinfo_parse_mounts(&info)
}

fn dockerinfo_parse_mounts(info: &serde_json::Value) -> Result<Vec<MountDetail>> {
    let mut mounts = dockerinfo_parse_user_mounts(info);
    let root_info = dockerinfo_parse_root_mount_path(info)?;
    mounts.push(root_info);
    Ok(mounts)
}

fn dockerinfo_parse_root_mount_path(info: &serde_json::Value) -> Result<MountDetail> {
    let driver_name = info
        .pointer("/0/GraphDriver/Name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| eyre::eyre!("no driver name found"))?;

    if driver_name == "overlay2" {
        let path = info
            .pointer("/0/GraphDriver/Data/MergedDir")
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre::eyre!("No merge directory found"))?;

        Ok(MountDetail {
            source: PathBuf::from(&path),
            destination: PathBuf::from("/"),
        })
    } else {
        eyre::bail!("want driver overlay2, got {driver_name}")
    }
}

fn dockerinfo_parse_user_mounts(info: &serde_json::Value) -> Vec<MountDetail> {
    info.pointer("/0/Mounts")
        .and_then(|v| v.as_array())
        .map(|v| {
            let make_path = |v: &serde_json::Value| PathBuf::from(&v.as_str().unwrap());
            let mut mounts = vec![];
            for details in v {
                let source = make_path(&details["Source"]);
                let destination = make_path(&details["Destination"]);
                mounts.push(MountDetail {
                    source,
                    destination,
                });
            }
            mounts
        })
        .unwrap_or_else(Vec::new)
}

#[derive(Debug, Default)]
struct MountFinder {
    mounts: Vec<MountDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MountDetail {
    source: PathBuf,
    destination: PathBuf,
}

impl MountFinder {
    fn new(mounts: Vec<MountDetail>) -> MountFinder {
        // sort by length (reverse), to give mounts with more path components a higher priority;
        let mut mounts = mounts;
        mounts.sort_by(|a, b| {
            let la = a.destination.as_os_str().len();
            let lb = b.destination.as_os_str().len();
            la.cmp(&lb).reverse()
        });
        MountFinder { mounts }
    }

    fn find_mount_path(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();

        for info in &self.mounts {
            if let Ok(stripped) = path.strip_prefix(&info.destination) {
                return info.source.join(stripped);
            }
        }

        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod mount_finder {
        use super::*;

        #[test]
        fn test_default_finder_returns_original() {
            let finder = MountFinder::default();
            assert_eq!(
                PathBuf::from("/test/path"),
                finder.find_mount_path("/test/path"),
            );
        }

        #[test]
        fn test_longest_destination_path_wins() {
            let finder = MountFinder::new(vec![
                MountDetail {
                    source: PathBuf::from("/project/path"),
                    destination: PathBuf::from("/project"),
                },
                MountDetail {
                    source: PathBuf::from("/target/path"),
                    destination: PathBuf::from("/project/target"),
                },
            ]);
            assert_eq!(
                PathBuf::from("/target/path/test"),
                finder.find_mount_path("/project/target/test")
            )
        }

        #[test]
        fn test_adjust_multiple_paths() {
            let finder = MountFinder::new(vec![
                MountDetail {
                    source: PathBuf::from("/var/lib/docker/overlay2/container-id/merged"),
                    destination: PathBuf::from("/"),
                },
                MountDetail {
                    source: PathBuf::from("/home/project/path"),
                    destination: PathBuf::from("/project"),
                },
            ]);
            assert_eq!(
                PathBuf::from("/var/lib/docker/overlay2/container-id/merged/container/path"),
                finder.find_mount_path("/container/path")
            );
            assert_eq!(
                PathBuf::from("/home/project/path"),
                finder.find_mount_path("/project")
            );
            assert_eq!(
                PathBuf::from("/home/project/path/target"),
                finder.find_mount_path("/project/target")
            );
        }
    }

    mod parse_docker_inspect {
        use super::*;
        use serde_json::json;

        #[test]
        fn test_parse_container_root() {
            let actual = dockerinfo_parse_root_mount_path(&json!([{
                "GraphDriver": {
                    "Data": {
                        "LowerDir": "/var/lib/docker/overlay2/f107af83b37bc0a182d3d2661f3d84684f0fffa1a243566b338a388d5e54bef4-init/diff:/var/lib/docker/overlay2/dfe81d459bbefada7aa897a9d05107a77145b0d4f918855f171ee85789ab04a0/diff:/var/lib/docker/overlay2/1f704696915c75cd081a33797ecc66513f9a7a3ffab42d01a3f17c12c8e2dc4c/diff:/var/lib/docker/overlay2/0a4f6cb88f4ace1471442f9053487a6392c90d2c6e206283d20976ba79b38a46/diff:/var/lib/docker/overlay2/1ee3464056f9cdc968fac8427b04e37ec96b108c5050812997fa83498f2499d1/diff:/var/lib/docker/overlay2/0ec5a47f1854c0f5cfe0e3f395b355b5a8bb10f6e622710ce95b96752625f874/diff:/var/lib/docker/overlay2/f24c8ad76303838b49043d17bf2423fe640836fd9562d387143e68004f8afba0/diff:/var/lib/docker/overlay2/462f89d5a0906805a6f2eec48880ed1e48256193ed506da95414448d435db2b7/diff",
                        "MergedDir": "/var/lib/docker/overlay2/f107af83b37bc0a182d3d2661f3d84684f0fffa1a243566b338a388d5e54bef4/merged",
                        "UpperDir": "/var/lib/docker/overlay2/f107af83b37bc0a182d3d2661f3d84684f0fffa1a243566b338a388d5e54bef4/diff",
                        "WorkDir": "/var/lib/docker/overlay2/f107af83b37bc0a182d3d2661f3d84684f0fffa1a243566b338a388d5e54bef4/work"
                    },
                    "Name": "overlay2"
                },
            }])).unwrap();
            let want = MountDetail {
                source: PathBuf::from("/var/lib/docker/overlay2/f107af83b37bc0a182d3d2661f3d84684f0fffa1a243566b338a388d5e54bef4/merged"),
                destination: PathBuf::from("/"),
            };
            assert_eq!(want, actual);
        }

        #[test]
        fn test_parse_empty_user_mounts() {
            let actual = dockerinfo_parse_user_mounts(&json!([{
                "Mounts": [],
            }]));
            assert_eq!(Vec::<MountDetail>::new(), actual);
        }

        #[test]
        fn test_parse_missing_user_moutns() {
            let actual = dockerinfo_parse_user_mounts(&json!([{
                "Id": "test",
            }]));
            assert_eq!(Vec::<MountDetail>::new(), actual);
        }
    }
}
