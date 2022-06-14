use std::path::{Path, PathBuf};

use crate::cargo::CargoMetadata;
use crate::errors::Result;

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
