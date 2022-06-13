#![deny(missing_debug_implementations, rust_2018_idioms)]

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
    ListImages(cross::commands::ListImages),
    /// Remove cross images in local storage.
    RemoveImages(cross::commands::RemoveImages),
    /// List cross data volumes in local storage.
    ListVolumes(cross::commands::ListVolumes),
    /// Remove cross data volumes in local storage.
    RemoveVolumes(cross::commands::RemoveVolumes),
    /// Prune volumes not used by any container.
    PruneVolumes(cross::commands::PruneVolumes),
    /// Create a persistent data volume for the current crate.
    CreateCrateVolume(cross::commands::CreateCrateVolume),
    /// Remove a persistent data volume for the current crate.
    RemoveCrateVolume(cross::commands::RemoveCrateVolume),
    /// List cross containers in local storage.
    ListContainers(cross::commands::ListContainers),
    /// Stop and remove cross containers in local storage.
    RemoveContainers(cross::commands::RemoveContainers),
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

pub fn main() -> cross::Result<()> {
    cross::install_panic_hook()?;
    let cli = Cli::parse();
    match cli.command {
        Commands::ListImages(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            cross::commands::list_images(args, &engine)?;
        }
        Commands::RemoveImages(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            if args.targets.is_empty() {
                cross::commands::remove_all_images(args, &engine)?;
            } else {
                cross::commands::remove_target_images(args, &engine)?;
            }
        }
        Commands::ListVolumes(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            cross::commands::list_volumes(args, &engine)?;
        }
        Commands::RemoveVolumes(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            cross::commands::remove_volumes(args, &engine)?;
        }
        Commands::PruneVolumes(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            cross::commands::prune_volumes(args, &engine)?;
        }
        Commands::CreateCrateVolume(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            let engine = cross::Engine::from_path(engine, true, args.verbose)?;
            cross::commands::create_crate_volume(args, &engine, cli.toolchain.as_deref())?;
        }
        Commands::RemoveCrateVolume(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            let engine = cross::Engine::from_path(engine, true, args.verbose)?;
            cross::commands::remove_crate_volume(args, &engine, cli.toolchain.as_deref())?;
        }
        Commands::ListContainers(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            cross::commands::list_containers(args, &engine)?;
        }
        Commands::RemoveContainers(args) => {
            let engine = get_container_engine(args.engine.as_deref())?;
            cross::commands::remove_containers(args, &engine)?;
        }
    }

    Ok(())
}
