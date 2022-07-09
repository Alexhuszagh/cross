#![deny(missing_debug_implementations, rust_2018_idioms)]
#![warn(
    clippy::explicit_into_iter_loop,
    clippy::explicit_iter_loop,
    clippy::implicit_clone,
    clippy::inefficient_to_string,
    clippy::map_err_ignore,
    clippy::map_unwrap_or,
    clippy::ref_binding_to_reference,
    clippy::semicolon_if_nothing_returned,
    clippy::str_to_string,
    clippy::string_to_string,
    // needs clippy 1.61 clippy::unwrap_used
)]

use std::cmp;
use std::collections::BTreeMap;

use eyre::Result;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

#[macro_use]
pub mod pattern;

pub mod config;
pub mod date;
pub mod git;
pub mod id;
pub mod stream;
pub mod util;

use self::config::Config;
use self::git::Commit;
use self::id::Identifier;

/// The complete, parsed changelog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeLog<'a> {
    /// The text before any changelog sections were found.
    header: &'a str,
    /// All parsed sections from the changelog.
    sections: Vec<ChangeLogSection<'a>>,
    /// The text after all changelog sections were found.
    footer: &'a str,
    /// Verbatim string read from the changelog file.
    verbatim: &'a str,
}

impl<'a> ChangeLog<'a> {
    /// Parse the changelog from a string.
    #[allow(unused_variables)] // TODO(ahuszagh) Remove
    pub fn parse(s: &'a str, config: &Config<'_>) -> Result<ChangeLog<'a>> {
        todo!();
    }
}

/// A section from the changelog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeLogSection<'a> {
    /// A verbatim changelog section
    ///
    /// This string is not parsed or validated, except
    /// to validate the header for the section. This
    /// cannot be used for the unreleased section.
    Verbatim(&'a str),
    /// A parsed changelog section.
    ///
    /// This can be formatted verbatim or using the
    /// automatic formatting. Only the unreleased
    /// section must be parsed, and it cannot be
    /// formatted verbatim.
    Parsed(ChangeLogSectionParsed<'a>),
}

impl<'a> ChangeLogSection<'a> {
    /// Extract or parse the changelog section from a string.
    #[allow(unused_variables)] // TODO(ahuszagh) Remove
    pub fn parse(s: &'a str, config: &Config<'_>) -> Result<ChangeLogSection<'a>> {
        todo!();
    }
}

/// A parsed section from the changelog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeLogSectionParsed<'a> {
    /// The version of the release.
    version: Version,
    /// All changelog entries in the section.
    entries: BTreeMap<&'static str, ChangeLogEntry<'a>>,
    /// Verbatim string for the given section.
    verbatim: &'a str,
}

impl<'a> ChangeLogSectionParsed<'a> {
    /// Parse the changelog section from a string.
    #[allow(unused_variables)] // TODO(ahuszagh) Remove
    pub fn parse(s: &'a str, config: &Config<'_>) -> Result<ChangeLogSectionParsed<'a>> {
        todo!();
    }
}

impl<'a> cmp::PartialOrd for ChangeLogSectionParsed<'a> {
    fn partial_cmp(&self, other: &ChangeLogSectionParsed<'a>) -> Option<cmp::Ordering> {
        self.version.partial_cmp(&other.version)
    }
}

impl<'a> cmp::Ord for ChangeLogSectionParsed<'a> {
    fn cmp(&self, other: &ChangeLogSectionParsed<'a>) -> cmp::Ordering {
        self.version.cmp(&other.version)
    }
}

/// A changelog entry.
///
/// This contains both the identifier and the contents of the entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeLogEntry<'a> {
    /// The identifier for the changelog entry.
    id: Identifier<'a>,
    /// The contents of the changelog.
    ///
    /// This can be the parsed from the changelog
    /// or from a changeset.
    contents: ChangeLogEntryContents,
    /// The verbatim string of the changelog.
    ///
    /// If parsed from a changeset, this is None.
    verbatim: Option<&'a str>,
}

/// The contents of a changelog entry.
///
/// This is what gets deserialized from the JSON/YAML/TOML files.
/// This does not use lifetimes since our YAML parser does not
/// support zero-copy parsing.
#[derive(Debug, Clone, Deserialize, Serialize, PartialOrd, Ord, PartialEq, Eq)]
pub struct ChangeLogEntryContents {
    /// Issues associated with the changelog entry.
    issues: Option<Vec<u64>>,
    /// Commits associated with the changelog entry.
    commits: Option<Vec<Commit>>,
    /// Whether the commit was a breaking change.
    #[serde(default)]
    breaking: bool,
    /// The description of the commit.
    ///
    /// Depending on the config options, this may
    /// be multi-line.
    description: String,
    /// The type of the change.
    #[serde(rename = "type")]
    kind: String,
}

impl ChangeLogEntryContents {
    /// Parse the changelog contents from a string.
    ///
    /// This assumes the changelog entry has already been parsed.
    #[allow(unused_variables)] // TODO(ahuszagh) Remove
    pub fn parse(s: &str, kind: &str, config: &Config<'_>) -> Result<ChangeLogEntryContents> {
        // TODO(ahuszagh) Must match the whole string
        todo!();
    }
}

static VERSION_RE: OnceCell<regex::Regex> = OnceCell::new();

fn version_re() -> &'static regex::Regex {
    VERSION_RE
        .get_or_try_init::<_, eyre::Report>(|| {
            // semver requires 3 version components, but we accept
            // less in case we ever accept non-semver versions
            regex::Regex::new(r"^(\d(?:\.\d){0,2}(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z-]+)?)")
                .map_err(Into::into)
        })
        .unwrap()
}

/// A version compatible with https://semver.org.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Version {
    /// An unreleased version section.
    /// This is what the changesets will modify.
    Unreleased,
    /// A previously-released version.
    SemVer(semver::Version),
}

impl Version {
    /// Serialize the version to string.
    pub fn to_string(&self, config: &Config<'_>) -> String {
        match self {
            Version::Unreleased => config.unreleased().to_owned(),
            Version::SemVer(version) => version.to_string(),
        }
    }

    /// Parse the version from string.
    /// Returns any bytes remaining after the
    pub fn parse<'a>(s: &'a str, config: &Config<'a>) -> Result<(Version, &'a str)> {
        match s.strip_prefix(config.unreleased()) {
            Some(v) => Ok((Version::Unreleased, v)),
            None => {
                let m = version_re()
                    .find(s)
                    .ok_or(eyre::eyre!("unable to find version match for {s}"))?;
                let (found, rest) = s.split_at(m.end());
                let version = semver::Version::parse(found)?;
                Ok((Version::SemVer(version), rest))
            }
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> cmp::Ordering {
        use cmp::Ordering::*;
        use Version::*;

        match (self, other) {
            (Unreleased, Unreleased) => Equal,
            (Unreleased, _) => Greater,
            (_, Unreleased) => Less,
            (x, y) => x.cmp(y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config;

    fn semver(s: &str) -> Result<Version> {
        Ok(Version::SemVer(semver::Version::parse(s)?))
    }

    macro_rules! s {
        ($s:literal) => {
            $s.to_owned()
        };
    }

    #[test]
    fn test_version_parse() -> Result<()> {
        let config = default_config();
        let (ver, rest) = Version::parse("0.1.0 trailing", config)?;
        assert_eq!(ver, semver("0.1.0")?);
        assert_eq!(rest, " trailing");

        let (ver, rest) = Version::parse("Unreleased trailing", config)?;
        assert_eq!(ver, Version::Unreleased);
        assert_eq!(rest, " trailing");

        let (ver, rest) = Version::parse("0.1.0-dev trailing", config)?;
        assert_eq!(ver, semver("0.1.0-dev")?);
        assert_eq!(rest, " trailing");

        let (ver, rest) = Version::parse("0.1.0+libgit2 trailing", config)?;
        assert_eq!(ver, semver("0.1.0+libgit2")?);
        assert_eq!(rest, " trailing");

        let (ver, rest) = Version::parse("0.1.0-dev+libgit2 trailing", config)?;
        assert_eq!(ver, semver("0.1.0-dev+libgit2")?);
        assert_eq!(rest, " trailing");

        let (ver, rest) = Version::parse("0.1.0-dev_x trailing", config)?;
        assert_eq!(ver, semver("0.1.0-dev")?);
        assert_eq!(rest, "_x trailing");

        let (ver, rest) = Version::parse("0.1.0- trailing", config)?;
        assert_eq!(ver, semver("0.1.0")?);
        assert_eq!(rest, "- trailing");

        let (ver, rest) = Version::parse("0.1.0+ trailing", config)?;
        assert_eq!(ver, semver("0.1.0")?);
        assert_eq!(rest, "+ trailing");

        Ok(())
    }

    #[test]
    fn test_version_to_string() -> Result<()> {
        let config = default_config();
        assert_eq!(semver("0.1.0-dev")?.to_string(&config), s!("0.1.0-dev"));
        assert_eq!(
            Version::Unreleased.to_string(&config),
            config.unreleased().to_owned()
        );

        Ok(())
    }

    #[test]
    fn test_entry_parse_file() -> Result<()> {
        let expected = ChangeLogEntryContents {
            issues: Some(vec![437]),
            commits: None,
            kind: s!("fixed"),
            description: s!("sample description for a PR adding one CHANGELOG entry."),
            breaking: false,
        };
        let json_entry: ChangeLogEntryContents = serde_json::from_str(JSON_ENTRY)?;
        assert_eq!(json_entry, expected,);

        let yaml_entry: ChangeLogEntryContents = serde_yaml::from_str(YAML_ENTRY)?;
        assert_eq!(yaml_entry, expected,);

        let toml_entry: ChangeLogEntryContents = toml::from_str(TOML_ENTRY)?;
        assert_eq!(toml_entry, expected,);

        Ok(())
    }

    const YAML_ENTRY: &str = r#"
description: "sample description for a PR adding one CHANGELOG entry."
issues:
    - 437
type: "fixed"
"#;

    const JSON_ENTRY: &str = r#"
{
    "description": "sample description for a PR adding one CHANGELOG entry.",
    "issues": [437],
    "type": "fixed"
}
"#;

    const TOML_ENTRY: &str = r#"
description = "sample description for a PR adding one CHANGELOG entry."
issues = [437]
type = "fixed"
"#;

    // TODO(ahuszagh) Test everything else.
}
