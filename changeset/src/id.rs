use std::fmt;

use crate::git::{self, CommitVec};
use crate::pattern::extract_unescaped_at_end;
use crate::util::{SortOrder, Utf8};

use eyre::Result;

// TODO(ahuszagh) This needs custom parsers.
// TODO(ahuszagh) This needs an optional prefix for each item
//  {#number[,]}
//  First has to be the control symbol
//      Must be a symbol

/// The identifier type for the changelog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identifier<'a> {
    /// A number associated with the entry.
    /// This may be a PR or issue number.
    Number(Vec<u64>),
    /// A Git commit for the entry.
    /// These are sorted by the order of the commits.
    Commit(CommitVec),
    /// Another valid identifier.
    Other(&'a str),
}

impl<'a> Identifier<'a> {
    /// If the identifier is a number.
    pub fn is_number(&self) -> bool {
        matches!(self, Identifier::Number(_))
    }

    /// If the identifier is a commit.
    pub fn is_commit(&self) -> bool {
        matches!(self, Identifier::Commit(_))
    }

    /// If the identifier is another identifier.
    pub fn is_other(&self) -> bool {
        matches!(self, Identifier::Other(_))
    }

    pub fn sort(&mut self, order: SortOrder) {
        match self {
            Identifier::Number(nums) => {
                nums.sort_unstable_by(|x, y| order.sort_by(x, y, |xi, yi| xi.cmp(yi)));
            }
            Identifier::Commit(commits) => {
                commits.sort_by(|x, y| order.sort_by(x, y, git::sort_by));
            }
            // already sorted: a single, fall-through identifier
            Identifier::Other(_) => (),
        }
    }
}

/// A placeholder for an identifier.
///
/// The placeholder is the separator.
#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum Placeholder<'a> {
    /// An identifier replacement specifier for any kind.
    /// The placeholder is the separator.
    Any(Option<char>, &'a str, Option<char>),
    /// An identifier replacement specifier for a number.
    /// The placeholder is the separator.
    Number(Option<char>, &'a str, Option<char>),
    /// An identifier replacement specifier for a commit.
    /// The placeholder is the separator.
    Commit(Option<char>, &'a str, Option<char>),
    /// A fallthrough identifier replacement specifier.
    Other,
}

impl<'a> Placeholder<'a> {
    /// Parse the placeholder from the format specifier.
    ///
    /// The braces signifiers have been previously removed,
    /// but any separators have not.
    pub fn parse(s: &'a str, default_sep: &'a str) -> Result<Placeholder<'a>> {
        let b = s.as_bytes();
        let (kind, sep) = match extract_unescaped_at_end(b, b'[', b']')? {
            Some(index) => {
                let kind = &b[..index];
                let sep = &b[index + 1..b.len() - 1];
                (kind.to_utf8()?, Some(sep.to_utf8()?))
            }
            None => (b.to_utf8()?, None),
        };

        match kind {
            // TODO(ahuszagh) Here...
            //"id" => into_placeholder!(@sep Any, sep, default_sep),
            //"number" => into_placeholder!(@sep Number, sep, default_sep),
            //"commit" => into_placeholder!(@sep Commit, sep, default_sep),
            //"other" => into_placeholder!(@nosep Other, sep),
            _ => eyre::bail!("got unsupported placeholder \"{s}\""),
        }
    }
}

impl<'a> fmt::Display for Placeholder<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            _ => todo!(),
            //Placeholder::Any(sep) => fmt_pattern!(@sep f, "id", sep),
            //Placeholder::Number(sep) => fmt_pattern!(@sep f, "number", sep),
            //Placeholder::Commit(sep) => fmt_pattern!(@sep f, "commit", sep),
            //Placeholder::Other => fmt_pattern!(@nosep f, "other"),
        }
    }
}

//define_pattern!(@sep);

#[cfg(test)]
mod tests {
    use super::*;

    // TODO(ahuszagh) Implement

//    #[test]
//    fn test_placeholder_parse() -> Result<()> {
//        let sep = ",";
//        let parse = |s| Placeholder::parse(s, sep);
//        assert_eq!(parse("id")?, Placeholder::Any(sep));
//        assert_eq!(parse("number")?, Placeholder::Number(sep));
//        assert_eq!(parse("commit")?, Placeholder::Commit(sep));
//        assert_eq!(parse("other")?, Placeholder::Other);
//
//        assert_eq!(parse("id[-]")?, Placeholder::Any("-"));
//        assert_eq!(parse("number[_]")?, Placeholder::Number("_"));
//        assert_eq!(parse("commit[/]")?, Placeholder::Commit("/"));
//
//        assert!(parse("other[/]").is_err());
//        assert!(parse("idx").is_err());
//
//        Ok(())
//    }
}
