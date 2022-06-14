use std::path::Path;
use std::process::Command;

use crate::docker::CROSS_IMAGE;
use crate::errors::Result;
use crate::extensions::CommandExt;

use clap::Args;

// known image prefixes, with their registry
// the docker.io registry can also be implicit
const GHCR_IO: &str = CROSS_IMAGE;
const RUST_EMBEDDED: &str = "rustembedded/cross:";
const DOCKER_IO: &str = "docker.io/rustembedded/cross:";
const IMAGE_PREFIXES: &[&str] = &[GHCR_IO, DOCKER_IO, RUST_EMBEDDED];

#[derive(Args, Debug)]
pub struct ListImages {
    /// Provide verbose diagnostic output.
    #[clap(short, long)]
    pub verbose: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
}

#[derive(Args, Debug)]
pub struct RemoveImages {
    /// If not provided, remove all images.
    pub targets: Vec<String>,
    /// Remove images matching provided targets.
    #[clap(short, long)]
    pub verbose: bool,
    /// Force removal of images.
    #[clap(short, long)]
    pub force: bool,
    /// Remove local (development) images.
    #[clap(short, long)]
    pub local: bool,
    /// Remove images. Default is a dry run.
    #[clap(short, long)]
    pub execute: bool,
    /// Container engine (such as docker or podman).
    #[clap(long)]
    pub engine: Option<String>,
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

fn get_cross_images(engine: &Path, verbose: bool, local: bool) -> Result<Vec<Image>> {
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

fn get_image_target(image: &Image) -> Result<String> {
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

pub fn list_images(ListImages { verbose, .. }: ListImages, engine: &Path) -> Result<()> {
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
) -> Result<()> {
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

pub fn remove_all_images(
    RemoveImages {
        verbose,
        force,
        local,
        execute,
        ..
    }: RemoveImages,
    engine: &Path,
) -> Result<()> {
    let images = get_cross_images(engine, verbose, local)?;
    let ids: Vec<&str> = images.iter().map(|i| i.id.as_ref()).collect();
    remove_images(engine, &ids, verbose, force, execute)
}

pub fn remove_target_images(
    RemoveImages {
        targets,
        verbose,
        force,
        local,
        execute,
        ..
    }: RemoveImages,
    engine: &Path,
) -> Result<()> {
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
