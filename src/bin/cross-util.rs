#![deny(missing_debug_implementations, rust_2018_idioms)]

use clap::{Parser, Subcommand};
use cross::docker;

mod commands;

#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Cli {
    /// Toolchain name/version to use (such as stable or 1.59.0).
    #[clap(value_parser = is_toolchain)]
    toolchain: Option<String>,
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Work with cross images in local storage.
    #[clap(subcommand)]
    Images(commands::Images),
    /// Work with cross volumes in local storage.
    #[clap(subcommand)]
    Volumes(commands::Volumes),
    /// Work with cross containers in local storage.
    #[clap(subcommand)]
    Containers(commands::Containers),
    /// Clean all cross data in local storage.
    Clean(commands::Clean),
}

fn is_toolchain(toolchain: &str) -> cross::Result<String> {
    if toolchain.starts_with('+') {
        Ok(toolchain.chars().skip(1).collect())
    } else {
        eyre::bail!("not a toolchain")
    }
}

fn get_container_engine(engine: Option<&str>, verbose: bool) -> cross::Result<docker::Engine> {
    let engine = if let Some(ce) = engine {
        which::which(ce)?
    } else {
        docker::get_container_engine()?
    };
    docker::Engine::from_path(engine, verbose)
}

pub fn main() -> cross::Result<()> {
    cross::install_panic_hook()?;
    let cli = Cli::parse();
    match cli.command {
        Commands::Images(args) => {
            let engine = get_container_engine(args.engine(), args.verbose())?;
            args.run(engine)?;
        }
        Commands::Volumes(args) => {
            let engine = get_container_engine(args.engine(), args.verbose())?;
            args.run(engine, cli.toolchain.as_deref())?;
        }
        Commands::Containers(args) => {
            let engine = get_container_engine(args.engine(), args.verbose())?;
            args.run(engine)?;
        }
        Commands::Clean(args) => {
            let engine = get_container_engine(args.engine.as_deref(), args.verbose)?;
            args.run(engine)?;
        }
    }

    Ok(())
}
