use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::ExitStatus;

use crate::config::bool_from_envvar;
use crate::errors::Result;
use crate::rustc;
use super::Engine;

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
