use std::path::Path;
use std::process::Command;

use crate::cargo::{cargo_metadata_with_args, CargoMetadata};
use crate::docker::*;
use crate::errors::Result;
use crate::extensions::CommandExt;
use crate::rustc::{target_list, version_meta, VersionMetaExt};
use crate::{get_sysroot, Target};

use atty::Stream;
use clap::Args;

#[derive(Args, Debug)]
pub struct ListVolumes {
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
}

#[derive(Args, Debug)]
pub struct RemoveVolumes {
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Force removal of volumes.
    #[clap(short, long)]
    pub force: bool,
    /// Remove volumes. Default is a dry run.
    #[clap(short, long)]
    pub execute: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
}

#[derive(Args, Debug)]
pub struct PruneVolumes {
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
}

#[derive(Args, Debug)]
pub struct CreateCrateVolume {
    /// Triple for the target platform.
    #[clap(long)]
    pub target: String,
    /// If cross is running inside a container.
    #[clap(short, long)]
    pub docker_in_docker: bool,
    /// If we should copy the cargo registry to the volume.
    #[clap(short, long)]
    pub copy_registry: bool,
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
}

#[derive(Args, Debug)]
pub struct RemoveCrateVolume {
    /// Triple for the target platform.
    #[clap(long)]
    pub target: String,
    /// If cross is running inside a container.
    #[clap(short, long)]
    pub docker_in_docker: bool,
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
}

#[derive(Args, Debug)]
pub struct ListContainers {
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
}

#[derive(Args, Debug)]
pub struct RemoveContainers {
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Force removal of containers.
    #[clap(short, long)]
    pub force: bool,
    /// Remove containers. Default is a dry run.
    #[clap(short, long)]
    pub execute: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
}

fn get_cross_volumes(engine: &Path, verbose: bool) -> Result<Vec<String>> {
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

pub fn list_volumes(ListVolumes { verbose, .. }: ListVolumes, engine: &Path) -> Result<()> {
    get_cross_volumes(engine, verbose)?
        .iter()
        .for_each(|line| println!("{}", line));

    Ok(())
}

pub fn remove_volumes(
    RemoveVolumes {
        verbose,
        force,
        execute,
        ..
    }: RemoveVolumes,
    engine: &Path,
) -> Result<()> {
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

pub fn prune_volumes(PruneVolumes { verbose, .. }: PruneVolumes, engine: &Path) -> Result<()> {
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
) -> Result<(Target, CargoMetadata, Directories)> {
    let target_list = target_list(false)?;
    let target = Target::from(target, &target_list);
    let metadata = cargo_metadata_with_args(None, None, verbose)?
        .ok_or(eyre::eyre!("unable to get project metadata"))?;
    let cwd = std::env::current_dir()?;
    let host_meta = version_meta()?;
    let host = host_meta.host();
    let sysroot = get_sysroot(&host, &target, channel, verbose)?.1;
    let dirs = Directories::create(&metadata, &cwd, &sysroot, docker_in_docker, verbose)?;

    Ok((target, metadata, dirs))
}

pub fn create_crate_volume(
    CreateCrateVolume {
        target,
        docker_in_docker,
        copy_registry,
        verbose,
        ..
    }: CreateCrateVolume,
    engine: &Engine,
    channel: Option<&str>,
) -> Result<()> {
    let (target, metadata, dirs) = get_package_info(&target, channel, docker_in_docker, verbose)?;
    let container = remote_identifier(&target, &metadata, &dirs)?;
    let volume = format!("{container}-keep");

    if volume_exists(engine, &volume, verbose)? {
        eyre::bail!("error: volume {volume} already exists.");
    }

    docker_command(engine)
        .args(&["volume", "create", &volume])
        .run_and_get_status(verbose)?;

    // stop the container if it's already running
    let state = container_state(engine, &container, verbose)?;
    if !state.is_stopped() {
        eprintln!("warning: container {container} was running.");
        container_stop(engine, &container, verbose)?;
    }
    if state.exists() {
        eprintln!("warning: container {container} was exited.");
        container_rm(engine, &container, verbose)?;
    }

    // create a dummy running container to copy data over
    let mount_prefix = Path::new("/cross");
    let mut docker = docker_command(engine);
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

    copy_volume_xargo(
        engine,
        &container,
        &dirs.xargo,
        &target,
        mount_prefix,
        verbose,
    )?;
    copy_volume_cargo(
        engine,
        &container,
        &dirs.cargo,
        mount_prefix,
        copy_registry,
        verbose,
    )?;
    copy_volume_rust(
        engine,
        &container,
        &dirs.sysroot,
        &target,
        mount_prefix,
        verbose,
    )?;

    container_stop(engine, &container, verbose)?;
    container_rm(engine, &container, verbose)?;

    Ok(())
}

pub fn remove_crate_volume(
    RemoveCrateVolume {
        target,
        docker_in_docker,
        verbose,
        ..
    }: RemoveCrateVolume,
    engine: &Engine,
    channel: Option<&str>,
) -> Result<()> {
    let (target, metadata, dirs) = get_package_info(&target, channel, docker_in_docker, verbose)?;
    let container = remote_identifier(&target, &metadata, &dirs)?;
    let volume = format!("{container}-keep");

    if !volume_exists(engine, &volume, verbose)? {
        eyre::bail!("error: volume {volume} does not exist.");
    }

    volume_rm(engine, &volume, verbose)?;

    Ok(())
}

fn get_cross_containers(engine: &Path, verbose: bool) -> Result<Vec<String>> {
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

pub fn list_containers(
    ListContainers { verbose, .. }: ListContainers,
    engine: &Path,
) -> Result<()> {
    get_cross_containers(engine, verbose)?
        .iter()
        .for_each(|line| println!("{}", line));

    Ok(())
}

pub fn remove_containers(
    RemoveContainers {
        verbose,
        force,
        execute,
        ..
    }: RemoveContainers,
    engine: &Path,
) -> Result<()> {
    let containers = get_cross_containers(engine, verbose)?;
    let mut running = vec![];
    let mut stopped = vec![];
    for container in containers.iter() {
        // cannot fail, formatted as {{.Names}}: {{.State}}
        let (name, state) = container.split_once(':').unwrap();
        let name = name.trim();
        let state = ContainerState::new(state.trim())?;
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
