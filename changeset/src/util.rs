use std::cmp;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;

use eyre::Result;
use serde::{Deserialize, Serialize};

// Convert to/from
pub trait Utf8 {
    fn to_utf8(&self) -> Result<&str>;
    fn from_ut8(s: &str) -> Result<&Self>;
}

impl Utf8 for [u8] {
    fn to_utf8(&self) -> Result<&str> {
        str::from_utf8(self)
            .map_err(|_ignore| eyre::eyre!("unable to convert `{self:?}` to UTF-8 string"))
    }

    fn from_ut8(s: &str) -> Result<&[u8]> {
        Ok(s.as_bytes())
    }
}

impl Utf8 for OsStr {
    fn to_utf8(&self) -> Result<&str> {
        self.to_str()
            .ok_or_else(|| eyre::eyre!("unable to convert `{self:?}` to UTF-8 string"))
    }

    fn from_ut8(s: &str) -> Result<&OsStr> {
        Ok(s.as_ref())
    }
}

impl Utf8 for Path {
    fn to_utf8(&self) -> Result<&str> {
        self.as_os_str().to_utf8()
    }

    fn from_ut8(s: &str) -> Result<&Path> {
        Ok(Path::new(s))
    }
}

/// Sort order to allow sorting by ascending or descending type.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    /// allow custo
    pub fn sort_by<T, Cmp>(&self, x: &T, y: &T, cmp_fn: Cmp) -> cmp::Ordering
    where
        Cmp: Fn(&T, &T) -> cmp::Ordering,
    {
        let ord = cmp_fn(x, y);
        match self {
            SortOrder::Ascending => ord,
            SortOrder::Descending => ord.reverse(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    pub workspace_root: PathBuf,
}

/// Get the workspace root of the current package.
pub fn package_dir(manifest_path: Option<&Path>) -> Result<PathBuf> {
    // FIXME: allow manifest paths and channels
    let metadata: CargoMetadata = serde_json::from_str({
        let mut cmd = Command::new("cargo");
        cmd.arg("metadata")
            .arg("--no-deps")
            .args(&["--format-version", "1"]);
        if let Some(path) = manifest_path {
            cmd.args(&["--manifest-path".as_ref(), path]);
        }

        cmd.output()?.stdout.to_utf8()?
    })?;

    Ok(metadata.workspace_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_serde_sort_order() -> Result<()> {
        let ascending = format!("\"ascending\"");
        let descending = format!("\"descending\"");
        let asc: SortOrder = serde_json::from_str(&ascending)?;
        let desc: SortOrder = serde_json::from_str(&descending)?;
        assert_eq!(asc, SortOrder::Ascending);
        assert_eq!(desc, SortOrder::Descending);
        assert_eq!(serde_json::to_string(&asc)?, ascending);
        assert_eq!(serde_json::to_string(&desc)?, descending);

        Ok(())
    }

    #[test]
    fn test_package_dir() -> Result<()> {
        // we have no idea where the command is invoked from,
        // but we assume it's a subdirectory for the test suite.
        let path = package_dir(None)?;
        assert!(path.exists());
        assert!(env::current_dir()?.starts_with(&path));

        Ok(())
    }
}
