use std::env;
use std::path::{Path, PathBuf};

use eyre::Result;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

use crate::date;
use crate::util::{package_dir, SortOrder};

/// The serializable format for the config format.
/// We have a few extra fields we need some values
/// to deserialize. All the descriptions are
/// in the getters below.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Inner<'a> {
    #[serde(default = "filename")]
    filename: &'a str,
    #[serde(default = "trim_whitespace")]
    trim_whitespace: bool,
    #[serde(default = "date_pattern")]
    date_pattern: date::Pattern<'a>,
    #[serde(default = "identifier")]
    identifier: Identifier<'a>,
    #[serde(default = "entry")]
    entry: Entry<'a>,
    #[serde(default = "section")]
    section: Section<'a>,
    //version_pattern: Pattern<'a>,
    // TODO(ahuszagh) Remove all the prefixes
    //  Use the format string.
    //    /// A prefix prior to all versions in the changelog.
    //    ///
    //    /// Defaults to `None`, but common values include `"v"`.
    //    version_prefix: Option<String>,
    //    /// The prefix before a changelog section (defaults to `## `).
    //    section_prefix: String,
    //    /// The prefix before a changelog change (defaults to `### `).
    //    change_prefix: String,
}

/// Keys specific to identifiers.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Identifier<'a> {
    #[serde(default = "sort_identifiers")]
    sort: Option<SortOrder>,
    #[serde(default = "default_changelog_separator")]
    default_changelog_separator: &'a str,
    #[serde(default = "default_file_name_separator")]
    default_file_name_separator: &'a str,
    #[serde(default = "commit_length")]
    commit_length: usize,
    #[serde(default = "number_prefix")]
    number_prefix: Option<char>,
    #[serde(default = "number_suffix")]
    number_suffix: Option<char>,
    #[serde(default = "commit_prefix")]
    commit_prefix: Option<char>,
    #[serde(default = "commit_suffix")]
    commit_suffix: Option<char>,
}

/// Keys specific to entries.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Entry<'a> {
    #[serde(default = "sort_entries")]
    sort: Option<SortOrder>,
    #[serde(default = "multiline")]
    multiline: bool,
    #[serde(default = "entry_prefix")]
    prefix: &'a str,
    #[serde(default = "breaking")]
    breaking: &'a str,
}

/// Keys specific to sections.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Section<'a> {
    #[serde(default = "change_types")]
    change_types: Vec<&'a str>,
    #[serde(default = "sort_change_types")]
    sort_change_types: Option<SortOrder>,
    #[serde(default = "verbatim")]
    verbatim: bool,
    #[serde(default = "unreleased")]
    unreleased: &'a str,
}

//// Global configuration options for the changelog.
#[derive(Debug, Clone)]
pub struct Config<'a> {
    inner: Inner<'a>,
}

impl<'a> Config<'a> {
    /// The prefix for a changelog entry (default `-`).
    pub fn entry_prefix(&self) -> &'a str {
        self.inner.entry.prefix
    }

    /// The breaking replacement text (default `Breaking: `).
    pub fn breaking(&self) -> &'a str {
        self.inner.entry.breaking
    }

    /// Sort order for identifiers in a given entry.
    /// If `None`, do not sort (default `Ascending`).
    pub fn sort_identifiers(&self) -> Option<SortOrder> {
        self.inner.identifier.sort
    }

    /// Sort order for entries by identifier in each section.
    /// If `None`, do not sort (default `Descending`).
    pub fn sort_entries(&self) -> Option<SortOrder> {
        self.inner.entry.sort
    }

    /// Sort order for change types (defaults to `Ascending`).
    pub fn sort_change_types(&self) -> Option<SortOrder> {
        self.inner.section.sort_change_types
    }

    /// Use the verbatim, parsed changelog for previous releases.
    pub fn verbatim(&self) -> bool {
        self.inner.section.verbatim
    }

    /// UThe unreleased version replacement text (default `Unreleased`).
    pub fn unreleased(&self) -> &'a str {
        self.inner.section.unreleased
    }

    /// Trim whitespace when parsing data (default true).
    ///
    /// Used when:
    /// - Parsing identifiers from filenames/entries.
    /// - Trimming leading/trailing whitespace from CHANGELOG lines.
    pub fn trim_whitespace(&self) -> bool {
        self.inner.trim_whitespace
    }

    /// Get the pattern to serialize/deserialize dates.
    ///
    /// For example, `{year}-{month}-{date}`.
    pub fn date_pattern(&self) -> &date::Pattern<'a> {
        &self.inner.date_pattern
    }

    /// Length of the git commit hashes (default `9`).
    pub fn commit_length(&self) -> usize {
        self.inner.identifier.commit_length
    }

    /// Filename for the changelog (default `CHANGELOG.md`).
    pub fn filename(&self) -> &'a str {
        self.inner.filename
    }

    /// Whether to allow multi-line entries (default true).
    pub fn multiline(&self) -> bool {
        self.inner.entry.multiline
    }

    /// The list of valid changes, in order of how they are formatted.
    /// If `sort_change_types` is `None`, then the sort
    /// order will be undefined.
    ///
    /// Defaults to:
    /// - Added
    /// - Changed
    /// - Removed
    /// - Fixed
    /// - Internal
    /// - Deprecated
    /// - Security
    pub fn change_types(&self) -> &[&'a str] {
        &self.inner.section.change_types
    }

    /// Get the default separator for identifiers in the changelog.
    pub fn default_changelog_separator(&self) -> &'a str {
        self.inner.identifier.default_changelog_separator
    }

    /// Get the default separator for identifiers in file names.
    pub fn default_file_name_separator(&self) -> &'a str {
        self.inner.identifier.default_file_name_separator
    }

    /// Validate if the config options are valid.
    pub fn validate(&self) -> bool {
        self.commit_length() <= 40
            && !self.entry_prefix().is_empty()
            && !self.filename().is_empty()
            && !self.change_types().iter().any(|v| v.is_empty())
    }

    /// Parse config from TOML string.
    pub fn parse(s: &'a str) -> Result<Config<'a>> {
        Ok(Config {
            inner: toml::from_str(s)?,
        })
    }

    /// Serialize config to TOML string.
    pub fn to_string(&self) -> Result<String> {
        toml::to_string(&self.inner).map_err(Into::into)
    }
}

/// Get the path to the changesets configuration file.
///
/// This value can be override by `CHANGESETS_CONFIG_PATH`,
/// which is particularly useful for non-Rust projects.
pub fn config_path(manifest_path: Option<&Path>) -> Result<PathBuf> {
    match env::var_os("CHANGESETS_CONFIG_PATH") {
        Some(path) => Ok(path.into()),
        None => Ok(package_dir(manifest_path)?.join("changesets.toml")),
    }
}

static DEFAULT_CONFIG: OnceCell<Config<'static>> = OnceCell::new();

/// Get the default config settings.
pub fn default_config() -> &'static Config<'static> {
    DEFAULT_CONFIG
        .get_or_try_init::<_, eyre::Report>(|| {
            let identifier = Identifier {
                default_changelog_separator: ",",
                default_file_name_separator: "-",
                sort: Some(SortOrder::Ascending),
                commit_length: 9,
                number_prefix: Some('#'),
                number_suffix: None,
                commit_prefix: None,
                commit_suffix: None,
            };
            let entry = Entry {
                sort: Some(SortOrder::Descending),
                prefix: "-",
                breaking: "Breaking:",
                multiline: true,
            };
            let section = Section {
                change_types: vec![
                    "Added",
                    "Changed",
                    "Removed",
                    "Fixed",
                    "Internal",
                    "Deprecated",
                    "Security",
                ],
                sort_change_types: Some(SortOrder::Ascending),
                verbatim: true,
                unreleased: "Unreleased",
            };
            let inner = Inner {
                filename: "CHANGELOG.md",
                trim_whitespace: true,
                date_pattern: date::Pattern::parse("{year}-{month}-{day}")?,
                identifier,
                entry,
                section,
            };
            Ok(Config { inner })
        })
        .unwrap()
}

// defaults: present for serde deserialization

fn filename() -> &'static str {
    default_config().inner.filename
}

fn trim_whitespace() -> bool {
    default_config().inner.trim_whitespace
}

fn date_pattern() -> date::Pattern<'static> {
    default_config().inner.date_pattern.clone()
}

fn identifier() -> Identifier<'static> {
    default_config().inner.identifier.clone()
}

fn entry() -> Entry<'static> {
    default_config().inner.entry.clone()
}

fn section() -> Section<'static> {
    default_config().inner.section.clone()
}

fn default_changelog_separator() -> &'static str {
    default_config()
        .inner
        .identifier
        .default_changelog_separator
}

fn default_file_name_separator() -> &'static str {
    default_config()
        .inner
        .identifier
        .default_file_name_separator
}

fn sort_identifiers() -> Option<SortOrder> {
    default_config().inner.identifier.sort
}

fn commit_length() -> usize {
    default_config().inner.identifier.commit_length
}

fn number_prefix() -> Option<char> {
    default_config().inner.identifier.number_prefix
}

fn number_suffix() -> Option<char> {
    default_config().inner.identifier.number_suffix
}

fn commit_prefix() -> Option<char> {
    default_config().inner.identifier.commit_prefix
}

fn commit_suffix() -> Option<char> {
    default_config().inner.identifier.commit_suffix
}

fn sort_entries() -> Option<SortOrder> {
    default_config().inner.entry.sort
}

fn multiline() -> bool {
    default_config().inner.entry.multiline
}

fn entry_prefix() -> &'static str {
    default_config().inner.entry.prefix
}

fn breaking() -> &'static str {
    default_config().inner.entry.breaking
}

fn change_types() -> Vec<&'static str> {
    default_config().inner.section.change_types.clone()
}

fn sort_change_types() -> Option<SortOrder> {
    default_config().inner.section.sort_change_types
}

fn verbatim() -> bool {
    default_config().inner.section.verbatim
}

fn unreleased() -> &'static str {
    default_config().inner.section.unreleased
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() -> Result<()> {
        let config = Config::parse(TOML_SIMPLE)?;
        assert_eq!(config.filename(), "CHANGES.md");
        assert_eq!(config.sort_identifiers(), Some(SortOrder::Descending));
        assert_eq!(config.commit_length(), 12);
        assert_eq!(config.change_types(), &["Changed", "Fixed", "Added"]);

        Ok(())
    }

    #[test]
    fn test_write_config() -> Result<()> {
        let config = Config::parse(TOML_SIMPLE)?;
        assert_eq!(config.to_string()?.trim(), TOML_COMPLETE.trim());

        Ok(())
    }

    const TOML_SIMPLE: &str = r#"
filename = "CHANGES.md"

[identifier]
sort = "descending"
commit-length = 12

[section]
change-types = ["Changed", "Fixed", "Added"]
"#;

    const TOML_COMPLETE: &str = concat!(
        r#"
filename = "CHANGES.md"
trim-whitespace = true
date-pattern = "{year}-{month}-{day}"

[identifier]
sort = "descending"
default-changelog-separator = ","
default-file-name-separator = "-"
commit-length = 12
number-prefix = ""#,
        "#",
        r#""

[entry]
sort = "descending"
multiline = true
prefix = "-"
breaking = "Breaking:"

[section]
change-types = ["Changed", "Fixed", "Added"]
sort-change-types = "ascending"
verbatim = true
unreleased = "Unreleased"
"#
    );
}
