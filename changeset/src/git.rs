use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::process::Command;
use std::str;

use crate::util::Utf8;
use eyre::Result;
use once_cell::sync::OnceCell;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

static COMMITS: OnceCell<CommitVec> = OnceCell::new();
static COMMIT_MAP: OnceCell<CommitMap> = OnceCell::new();

/// A structure for a Git commit.
#[derive(Debug, Clone)]
pub struct Commit([u8; 40], usize);

impl Commit {
    /// Create the Git commit from a hex digest.
    pub fn from_hex(s: &str) -> Result<Commit> {
        let bytes = s.as_bytes();
        let length = bytes.len();
        let mut buffer = [0u8; 40];
        // the default short hash is 7 characters, can be up to 40.
        match (7..=40).contains(&length) && s.chars().all(|c| c.is_ascii_hexdigit()) {
            true => {
                buffer[..length].copy_from_slice(bytes);
                Ok(Commit(buffer, length))
            }
            false => eyre::bail!("invalid SHA1 hexdigest, got \"{s}\""),
        }
    }

    /// Format the Git commit as a hex digest.
    pub fn to_hex(&self) -> Result<&str> {
        self.as_bytes().to_utf8()
    }

    /// Get the shorter hash from the commit.
    pub fn short_hash(&self, length: usize) -> Option<Commit> {
        match length <= self.1 {
            true => Some(Commit(self.0, length)),
            false => None,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.0[..self.1]
    }

    fn max_long_hash(&self) -> Commit {
        let mut new = Commit(self.0, 40);
        new.0[self.1..].fill(b'f');

        new
    }

    fn to_digits(&self) -> impl Iterator<Item = Option<u32>> + '_ {
        self.as_bytes().iter().map(|&x| (x as char).to_digit(16))
    }
}

impl PartialEq for Commit {
    fn eq(&self, other: &Commit) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for Commit {}

impl PartialOrd for Commit {
    fn partial_cmp(&self, other: &Commit) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Commit {
    fn cmp(&self, other: &Commit) -> Ordering {
        self.to_digits().cmp(other.to_digits())
    }
}

impl str::FromStr for Commit {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Commit> {
        Commit::from_hex(s)
    }
}

impl fmt::Display for Commit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_hex().map_err(|_ignore| fmt::Error::default())?)
    }
}

impl Serialize for Commit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::Error;
        serializer.serialize_str(self.to_hex().map_err(Error::custom)?)
    }
}

impl<'de> Deserialize<'de> for Commit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        Self::from_hex(s).map_err(de::Error::custom)
    }
}

/// List of git commits, with newer commits first.
pub type CommitVec = Vec<Commit>;

/// Get the list of Git commits for the current repository.
pub fn get_commits() -> &'static CommitVec {
    COMMITS
        .get_or_try_init::<_, eyre::Report>(|| {
            Command::new("git")
                .args(&["log", "--pretty=format:%H"])
                .output()?
                .stdout
                .to_utf8()?
                .lines()
                .map(Commit::from_hex)
                .collect::<Result<CommitVec, _>>()
                .map_err(Into::into)
        })
        .unwrap()
}

/// Map to get the order of Git commits.
///
/// This uses a binary search map so short
/// commits can be efficiently retrieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMap(BTreeMap<Commit, usize>);

impl CommitMap {
    /// Get if the map contains the desired key.
    pub fn contains_key(&self, commit: &Commit) -> bool {
        self.get(commit).is_some()
    }

    /// Get the commit from the range.
    pub fn get(&self, commit: &Commit) -> Option<&usize> {
        let max = commit.max_long_hash();
        let mut iter = self.0.range(commit..=&max);
        // NOTE: assume there are no collisions
        iter.next().map(|x| x.1)
    }
}

/// Get the Git commit map for the current repository.
pub fn get_commit_map() -> &'static CommitMap {
    COMMIT_MAP
        .get_or_try_init::<_, eyre::Report>(|| {
            Ok(CommitMap(
                get_commits()
                    .iter()
                    .cloned()
                    .enumerate()
                    // newer commits need to be larger
                    .map(|(index, commit)| (commit, usize::MAX - index))
                    .collect(),
            ))
        })
        .unwrap()
}

pub(crate) fn sort_by(x: &Commit, y: &Commit) -> Ordering {
    let map = get_commit_map();
    map.get(x).cmp(&map.get(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH1: &str = "5bb63ae9b7f222a495d75c2975e3c0d12f76f748";
    const HASH2: &str = "a60d4864ff641091a01f6317c7d9348265eb4463";
    const HASH3: &str = "531a8b8880178df2480a240ed4261263f78c51c0";

    #[test]
    fn test_to_from_hex() -> Result<()> {
        let expected = Commit(*b"5bb63ae9b7f222a495d75c2975e3c0d12f76f748", 40);
        assert_eq!(Commit::from_hex(HASH1)?, expected);
        assert_eq!(expected.to_hex()?, HASH1);

        Ok(())
    }

    #[test]
    fn test_short_hash() -> Result<()> {
        let commit1 = Commit::from_hex(HASH1)?;
        let short1 = Commit::from_hex(&HASH1[..9])?;
        assert_eq!(Some(short1), commit1.short_hash(9));

        Ok(())
    }

    #[test]
    fn test_get_commits() -> Result<()> {
        let commits = get_commits();
        assert_eq!(commits[commits.len() - 1], Commit::from_hex(HASH3)?);

        Ok(())
    }

    #[test]
    fn test_get_commit_map() -> Result<()> {
        let map = get_commit_map();
        let commit1 = Commit::from_hex(HASH1)?;
        let commit2 = Commit::from_hex(HASH2)?;
        let commit3 = Commit::from_hex(HASH3)?;
        let short1 = Commit::from_hex(&HASH1[..9])?;
        let short2 = Commit::from_hex(&HASH2[..7])?;
        let short3 = Commit::from_hex(&HASH2[..12])?;

        // full commits
        assert!(map.contains_key(&commit1));
        assert!(map.contains_key(&commit2));
        assert!(map.contains_key(&commit3));

        // let's try short commits
        assert!(map.contains_key(&short1));
        assert!(map.contains_key(&short2));
        assert!(map.contains_key(&short3));

        // test invalid hashes
        let invalid1 = Commit::from_hex("1234567890abcdef")?;
        assert!(!map.contains_key(&invalid1));

        Ok(())
    }

    #[test]
    fn test_sort_by() -> Result<()> {
        let commit1 = Commit::from_hex(HASH1)?;
        let commit2 = Commit::from_hex(HASH2)?;
        let commit3 = Commit::from_hex(HASH3)?;
        assert_eq!(sort_by(&commit1, &commit2), Ordering::Greater);
        assert_eq!(sort_by(&commit2, &commit3), Ordering::Greater);

        Ok(())
    }

    #[test]
    fn test_serde_commit() -> Result<()> {
        let hash1 = format!("\"{HASH1}\"");
        let hash2 = format!("\"{HASH2}\"");
        let hash3 = format!("\"{HASH3}\"");
        let commit1: Commit = serde_json::from_str(&hash1)?;
        let commit2: Commit = serde_json::from_str(&hash2)?;
        let commit3: Commit = serde_json::from_str(&hash3)?;

        assert_eq!(commit1, Commit::from_hex(HASH1)?);
        assert_eq!(commit2, Commit::from_hex(HASH2)?);
        assert_eq!(commit3, Commit::from_hex(HASH3)?);

        assert_eq!(serde_json::to_string(&commit1)?, hash1);
        assert_eq!(serde_json::to_string(&commit2)?, hash2);
        assert_eq!(serde_json::to_string(&commit3)?, hash3);

        Ok(())
    }
}
