#![deny(missing_debug_implementations, rust_2018_idioms)]

use std::path::{Path, PathBuf};
use std::process::Command;

use atty::Stream;
use clap::{Parser, Subcommand};
use cross::{CommandExt, VersionMetaExt};

// known image prefixes, with their registry
// the docker.io registry can also be implicit
const GHCR_IO: &str = "ghcr.io/cross-rs/";
const RUST_EMBEDDED: &str = "rustembedded/cross:";
const DOCKER_IO: &str = "docker.io/rustembedded/cross:";
const IMAGE_PREFIXES: &[&str] = &[GHCR_IO, DOCKER_IO, RUST_EMBEDDED];

#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Cli {
    /// Toolchain name/version to use (such as stable or 1.59.0).
    #[clap(validator = is_toolchain)]
    toolchain: Option<String>,
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List cross images in local storage.
    ListImages {
        /// Provide verbose diagnostic output.
        #[clap(short, long)]
        verbose: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
    /// Remove cross images in local storage.
    RemoveImages {
        /// If not provided, remove all images.
        targets: Vec<String>,
        /// Remove images matching provided targets.
        #[clap(short, long)]
        verbose: bool,
        /// Force removal of images.
        #[clap(short, long)]
        force: bool,
        /// Remove local (development) images.
        #[clap(short, long)]
        local: bool,
        /// Remove images. Default is a dry run.
        #[clap(short, long)]
        execute: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
    /// List cross data volumes in local storage.
    ListVolumes {
        /// Provide verbose diagnostic output.
        #[clap(short, long)]
        verbose: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
    /// Remove cross data volumes in local storage.
    RemoveVolumes {
        /// Provide verbose diagnostic output.
        #[clap(short, long)]
        verbose: bool,
        /// Force removal of volumes.
        #[clap(short, long)]
        force: bool,
        /// Remove volumes. Default is a dry run.
        #[clap(short, long)]
        execute: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
    /// Prune volumes not used by any container.
    PruneVolumes {
        /// Provide verbose diagnostic output.
        #[clap(short, long)]
        verbose: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
    /// Create a persistent data volume for the current crate.
    CreateCrateVolume {
        /// Triple for the target platform.
        #[clap(long)]
        target: String,
        /// If cross is running inside a container.
        #[clap(short, long)]
        docker_in_docker: bool,
        /// If we should copy the cargo registry to the volume.
        #[clap(short, long)]
        copy_registry: bool,
        /// Provide verbose diagnostic output.
        #[clap(short, long)]
        verbose: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
    /// Remove a persistent data volume for the current crate.
    RemoveCrateVolume {
        /// Triple for the target platform.
        #[clap(long)]
        target: String,
        /// If cross is running inside a container.
        #[clap(short, long)]
        docker_in_docker: bool,
        /// Provide verbose diagnostic output.
        #[clap(short, long)]
        verbose: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
    /// List cross containers in local storage.
    ListContainers {
        /// Provide verbose diagnostic output.
        #[clap(short, long)]
        verbose: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
    /// Stop and remove cross containers in local storage.
    RemoveContainers {
        /// Provide verbose diagnostic output.
        #[clap(short, long)]
        verbose: bool,
        /// Force removal of containers.
        #[clap(short, long)]
        force: bool,
        /// Remove containers. Default is a dry run.
        #[clap(short, long)]
        execute: bool,
        /// Container engine (such as docker or podman).
        #[clap(long)]
        engine: Option<String>,
    },
}

#[derive(Debug, PartialOrd, Ord, PartialEq, Eq)]
struct Image {
    repository: String,
    tag: String,
    // need to remove images by ID, not just tag
    id: String,
}

impl Image {
    fn name(&self) -> String {
        format!("{}:{}", self.repository, self.tag)
    }
}

fn is_toolchain(toolchain: &str) -> cross::Result<String> {
    if toolchain.starts_with('+') {
        Ok(toolchain.chars().skip(1).collect())
    } else {
        eyre::bail!("not a toolchain")
    }
}

fn get_container_engine(engine: Option<&str>) -> Result<PathBuf, which::Error> {
    if let Some(ce) = engine {
        which::which(ce)
    } else {
        cross::get_container_engine()
    }
}

fn parse_image(image: &str) -> Image {
    // this cannot panic: we've formatted our image list as `${repo}:${tag} ${id}`
    let (repository, rest) = image.split_once(':').unwrap();
    let (tag, id) = rest.split_once(' ').unwrap();
    Image {
        repository: repository.to_string(),
        tag: tag.to_string(),
        id: id.to_string(),
    }
}

fn is_cross_image(repository: &str) -> bool {
    IMAGE_PREFIXES.iter().any(|i| repository.starts_with(i))
}

fn is_local_image(tag: &str) -> bool {
    tag.starts_with("local")
}

fn get_cross_images(engine: &Path, verbose: bool, local: bool) -> cross::Result<Vec<Image>> {
    let stdout = Command::new(engine)
        .arg("images")
        .arg("--format")
        .arg("{{.Repository}}:{{.Tag}} {{.ID}}")
        .run_and_get_stdout(verbose)?;

    let mut images: Vec<Image> = stdout
        .lines()
        .map(parse_image)
        .filter(|image| is_cross_image(&image.repository))
        .filter(|image| local || !is_local_image(&image.tag))
        .collect();
    images.sort();

    Ok(images)
}

// the old rustembedded targets had the following format:
//  repository = (${registry}/)?rustembedded/cross
//  tag = ${target}(-${version})?
// the last component must match `[A-Za-z0-9_-]` and
// we must have at least 3 components. the first component
// may contain other characters, such as `thumbv8m.main-none-eabi`.
fn rustembedded_target(tag: &str) -> String {
    let is_target_char = |c: char| c == '_' || c.is_ascii_alphanumeric();
    let mut components = vec![];
    for (index, component) in tag.split('-').enumerate() {
        if index <= 2 || (!component.is_empty() && component.chars().all(is_target_char)) {
            components.push(component)
        } else {
            break;
        }
    }

    components.join("-")
}

fn get_image_target(image: &Image) -> cross::Result<String> {
    if let Some(stripped) = image.repository.strip_prefix(GHCR_IO) {
        Ok(stripped.to_string())
    } else if let Some(tag) = image.tag.strip_prefix(RUST_EMBEDDED) {
        Ok(rustembedded_target(tag))
    } else if let Some(tag) = image.tag.strip_prefix(DOCKER_IO) {
        Ok(rustembedded_target(tag))
    } else {
        eyre::bail!("cannot get target for image {}", image.name())
    }
}

fn list_images(engine: &Path, verbose: bool) -> cross::Result<()> {
    get_cross_images(engine, verbose, true)?
        .iter()
        .for_each(|line| println!("{}", line.name()));

    Ok(())
}

fn remove_images(
    engine: &Path,
    images: &[&str],
    verbose: bool,
    force: bool,
    execute: bool,
) -> cross::Result<()> {
    let mut command = Command::new(engine);
    command.arg("rmi");
    if force {
        command.arg("--force");
    }
    command.args(images);
    if execute {
        command.run(verbose)
    } else {
        println!("{:?}", command);
        Ok(())
    }
}

fn remove_all_images(
    engine: &Path,
    verbose: bool,
    force: bool,
    local: bool,
    execute: bool,
) -> cross::Result<()> {
    let images = get_cross_images(engine, verbose, local)?;
    let ids: Vec<&str> = images.iter().map(|i| i.id.as_ref()).collect();
    remove_images(engine, &ids, verbose, force, execute)
}

fn remove_target_images(
    engine: &Path,
    targets: &[String],
    verbose: bool,
    force: bool,
    local: bool,
    execute: bool,
) -> cross::Result<()> {
    let images = get_cross_images(engine, verbose, local)?;
    let mut ids = vec![];
    for image in images.iter() {
        let target = get_image_target(image)?;
        if targets.contains(&target) {
            ids.push(image.id.as_ref());
        }
    }
    remove_images(engine, &ids, verbose, force, execute)
}

fn get_cross_volumes(engine: &Path, verbose: bool) -> cross::Result<Vec<String>> {
    let stdout = Command::new(engine)
        .args(&["volume", "list"])
        .arg("--format")
        .arg("{{.Name}}")
        .arg("--filter")
        // handles simple regex: ^ for start of line.
        .arg("name=^cross-")
        .run_and_get_stdout(verbose)?;

    let mut volumes: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();
    volumes.sort();

    Ok(volumes)
}

fn list_volumes(engine: &Path, verbose: bool) -> cross::Result<()> {
    get_cross_volumes(engine, verbose)?
        .iter()
        .for_each(|line| println!("{}", line));

    Ok(())
}

fn remove_volumes(engine: &Path, verbose: bool, force: bool, execute: bool) -> cross::Result<()> {
    let volumes = get_cross_volumes(engine, verbose)?;

    let mut command = Command::new(engine);
    command.args(&["volume", "rm"]);
    if force {
        command.arg("--force");
    }
    command.args(&volumes);
    if execute {
        command.run(verbose)
    } else {
        println!("{:?}", command);
        Ok(())
    }
}

fn prune_volumes(engine: &Path, verbose: bool) -> cross::Result<()> {
    Command::new(engine)
        .args(&["volume", "prune", "--force"])
        .run_and_get_status(verbose)?;

    Ok(())
}

fn get_package_info(
    target: &str,
    channel: Option<&str>,
    docker_in_docker: bool,
    verbose: bool,
) -> cross::Result<(cross::Target, cross::CargoMetadata, cross::Directories)> {
    let target_list = cross::target_list(false)?;
    let target = cross::Target::from(target, &target_list);
    let metadata = cross::cargo_metadata_with_args(None, None, verbose)?
        .ok_or(eyre::eyre!("unable to get project metadata"))?;
    let cwd = std::env::current_dir()?;
    let host_meta = cross::version_meta()?;
    let host = host_meta.host();
    let sysroot = cross::get_sysroot(&host, &target, channel, verbose)?.1;
    let dirs = cross::Directories::create(&metadata, &cwd, &sysroot, docker_in_docker, verbose)?;

    Ok((target, metadata, dirs))
}

fn create_crate_volume(
    engine: &cross::Engine,
    target: &str,
    docker_in_docker: bool,
    channel: Option<&str>,
    copy_registry: bool,
    verbose: bool,
) -> cross::Result<()> {
    let (target, metadata, dirs) = get_package_info(target, channel, docker_in_docker, verbose)?;
    let container = cross::remote_identifier(&target, &metadata, &dirs)?;
    let volume = format!("{container}-keep");

    if cross::volume_exists(engine, &volume, verbose)? {
        eyre::bail!("error: volume {volume} already exists.");
    }

    cross::docker_command(engine)
        .args(&["volume", "create", &volume])
        .run_and_get_status(verbose)?;

    // stop the container if it's already running
    let state = cross::container_state(engine, &container, verbose)?;
    if !state.is_stopped() {
        eprintln!("warning: container {container} was running.");
        cross::container_stop(engine, &container, verbose)?;
    }
    if state.exists() {
        eprintln!("warning: container {container} was exited.");
        cross::container_rm(engine, &container, verbose)?;
    }

    // create a dummy running container to copy data over
    let mount_prefix = Path::new("/cross");
    let mut docker = cross::docker_command(engine);
    docker.arg("run");
    docker.args(&["--name", &container]);
    docker.args(&["-v", &format!("{}:{}", volume, mount_prefix.display())]);
    docker.arg("-d");
    if atty::is(Stream::Stdin) && atty::is(Stream::Stdout) && atty::is(Stream::Stderr) {
        docker.arg("-t");
    }
    docker.arg("ubuntu:16.04");
    // ensure the process never exits until we stop it
    docker.args(&["sh", "-c", "sleep infinity"]);
    docker.run_and_get_status(verbose)?;

    cross::copy_volume_xargo(
        engine,
        &container,
        &dirs.xargo,
        &target,
        mount_prefix,
        verbose,
    )?;
    cross::copy_volume_cargo(
        engine,
        &container,
        &dirs.cargo,
        mount_prefix,
        copy_registry,
        verbose,
    )?;
    cross::copy_volume_rust(
        engine,
        &container,
        &dirs.sysroot,
        &target,
        mount_prefix,
        verbose,
    )?;

    cross::container_stop(engine, &container, verbose)?;
    cross::container_rm(engine, &container, verbose)?;

    Ok(())
}

fn remove_crate_volume(
    engine: &cross::Engine,
    target: &str,
    docker_in_docker: bool,
    channel: Option<&str>,
    verbose: bool,
) -> cross::Result<()> {
    let (target, metadata, dirs) = get_package_info(target, channel, docker_in_docker, verbose)?;
    let container = cross::remote_identifier(&target, &metadata, &dirs)?;
    let volume = format!("{container}-keep");

    if !cross::volume_exists(engine, &volume, verbose)? {
        eyre::bail!("error: volume {volume} does not exist.");
    }

    cross::volume_rm(engine, &volume, verbose)?;

    Ok(())
}

fn get_cross_containers(engine: &Path, verbose: bool) -> cross::Result<Vec<String>> {
    let stdout = Command::new(engine)
        .args(&["ps", "-a"])
        .arg("--format")
        .arg("{{.Names}}: {{.State}}")
        .arg("--filter")
        // handles simple regex: ^ for start of line.
        .arg("name=^cross-")
        .run_and_get_stdout(verbose)?;

    let mut containers: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();
    containers.sort();

    Ok(containers)
}

fn list_containers(engine: &Path, verbose: bool) -> cross::Result<()> {
    get_cross_containers(engine, verbose)?
        .iter()
        .for_each(|line| println!("{}", line));

    Ok(())
}

fn remove_containers(
    engine: &Path,
    verbose: bool,
    force: bool,
    execute: bool,
) -> cross::Result<()> {
    let containers = get_cross_containers(engine, verbose)?;
    let mut running = vec![];
    let mut stopped = vec![];
    for container in containers.iter() {
        // cannot fail, formatted as {{.Names}}: {{.State}}
        let (name, state) = container.split_once(':').unwrap();
        let name = name.trim();
        let state = cross::ContainerState::new(state.trim())?;
        if state.is_stopped() {
            stopped.push(name);
        } else {
            running.push(name);
        }
    }

    let mut commands = vec![];
    if !running.is_empty() {
        let mut stop = Command::new(engine);
        stop.arg("stop");
        stop.args(&running);
        commands.push(stop);
    }

    if !(stopped.is_empty() && running.is_empty()) {
        let mut rm = Command::new(engine);
        rm.arg("rm");
        if force {
            rm.arg("--force");
        }
        rm.args(&running);
        rm.args(&stopped);
        commands.push(rm);
    }
    if execute {
        for mut command in commands {
            command.run(verbose)?;
        }
    } else {
        for command in commands {
            println!("{:?}", command);
        }
    }

    Ok(())
}

pub fn main() -> cross::Result<()> {
    cross::install_panic_hook()?;
    let cli = Cli::parse();
    match &cli.command {
        Commands::ListImages { verbose, engine } => {
            let engine = get_container_engine(engine.as_deref())?;
            list_images(&engine, *verbose)?;
        }
        Commands::RemoveImages {
            targets,
            verbose,
            force,
            local,
            execute,
            engine,
        } => {
            let engine = get_container_engine(engine.as_deref())?;
            if targets.is_empty() {
                remove_all_images(&engine, *verbose, *force, *local, *execute)?;
            } else {
                remove_target_images(&engine, targets, *verbose, *force, *local, *execute)?;
            }
        }
        Commands::ListVolumes { verbose, engine } => {
            let engine = get_container_engine(engine.as_deref())?;
            list_volumes(&engine, *verbose)?;
        }
        Commands::RemoveVolumes {
            verbose,
            force,
            execute,
            engine,
        } => {
            let engine = get_container_engine(engine.as_deref())?;
            remove_volumes(&engine, *verbose, *force, *execute)?;
        }
        Commands::PruneVolumes { verbose, engine } => {
            let engine = get_container_engine(engine.as_deref())?;
            prune_volumes(&engine, *verbose)?;
        }
        Commands::CreateCrateVolume {
            target,
            docker_in_docker,
            verbose,
            engine,
            copy_registry,
        } => {
            let engine = get_container_engine(engine.as_deref())?;
            let engine = cross::Engine::from_path(engine, true, *verbose)?;
            create_crate_volume(
                &engine,
                target,
                *docker_in_docker,
                cli.toolchain.as_deref(),
                *copy_registry,
                *verbose,
            )?;
        }
        Commands::RemoveCrateVolume {
            target,
            docker_in_docker,
            verbose,
            engine,
        } => {
            let engine = get_container_engine(engine.as_deref())?;
            let engine = cross::Engine::from_path(engine, true, *verbose)?;
            remove_crate_volume(
                &engine,
                target,
                *docker_in_docker,
                cli.toolchain.as_deref(),
                *verbose,
            )?;
        }
        Commands::ListContainers { verbose, engine } => {
            let engine = get_container_engine(engine.as_deref())?;
            list_containers(&engine, *verbose)?;
        }
        Commands::RemoveContainers {
            verbose,
            force,
            execute,
            engine,
        } => {
            let engine = get_container_engine(engine.as_deref())?;
            remove_containers(&engine, *verbose, *force, *execute)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rustembedded_target() {
        let targets = [
            "x86_64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "thumbv8m.main-none-eabi",
        ];
        for target in targets {
            let versioned = format!("{target}-0.2.1");
            assert_eq!(rustembedded_target(target), target.to_string());
            assert_eq!(rustembedded_target(&versioned), target.to_string());
        }
    }
}
