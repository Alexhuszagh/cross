use std::fmt;

use crate::util::Utf8;
use eyre::Result;

// TODO(ahuszagh) This is a bit too much actually.
// Needs to be entry/identifier.

/// The type of a placeholder.
///
/// These enable the customization of the serialization
/// of identifiers by enabled or disabling certain types.
#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum Placeholder<'a> {
    /// An identifier replacement specifier for any kind.
    /// The placeholder is the separator.
    AnyIdentifier(&'a str),
    /// An identifier replacement specifier for a number.
    /// The placeholder is the separator.
    NumberIdentifier(&'a str),
    /// An identifier replacement specifier for a commit.
    /// The placeholder is the separator.
    CommitIdentifier(&'a str),
    /// A fallthrough identifier replacement specifier.
    OtherIdentifier,
    /// An issues contents replacement specifier.
    /// The placeholder is the separator.
    Issues(&'a str),
    /// A commits contents replacement specifier.
    /// The placeholder is the separator.
    Commits(&'a str),
    /// A breaking replacement specifier.
    Breaking,
    /// A description replacement specifier.
    Description,
    /// A date replacement specifier.
    Date,
    /// A wildcard specifier.
    /// This must be followed by a string section.
    Wildcard,
}

macro_rules! into_placeholder {
    (@sep $variant:ident, $sep:ident, $default_sep:ident) => {{
        match $sep {
            Some("") => eyre::bail!(
                "Placeholder::{} must have a non-empty separator.",
                stringify!($variant)
            ),
            Some(v) => Ok(Placeholder::$variant(v)),
            None => Ok(Placeholder::$variant($default_sep)),
        }
    }};

    (@nosep $variant:ident, $sep:ident) => {{
        match $sep.map_or(true, |s| s.is_empty()) {
            true => Ok(Placeholder::$variant),
            false => eyre::bail!(
                "Placeholder::{} must have an empty separator.",
                stringify!($variant)
            ),
        }
    }};
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
            "id" => into_placeholder!(@sep AnyIdentifier, sep, default_sep),
            "number" => into_placeholder!(@sep NumberIdentifier, sep, default_sep),
            "commit" => into_placeholder!(@sep CommitIdentifier, sep, default_sep),
            "other" => into_placeholder!(@nosep OtherIdentifier, sep),
            "issues" => into_placeholder!(@sep Issues, sep, default_sep),
            "commits" => into_placeholder!(@sep Commits, sep, default_sep),
            "breaking" => into_placeholder!(@nosep Breaking, sep),
            "description" => into_placeholder!(@nosep Description, sep),
            "date" => into_placeholder!(@nosep Date, sep),
            "*" => into_placeholder!(@nosep Wildcard, sep),
            _ => eyre::bail!("got unsupported placeholder \"{s}\""),
        }
    }
}

macro_rules! fmt_pattern {
    (@sep $f:ident, $variant:literal, $sep:ident) => {{
        $f.write_fmt(format_args!("{}[{}]", $variant, $sep))
    }};

    (@nosep $f:ident, $variant:literal) => {{
        $f.write_str($variant)
    }};
}

impl<'a> fmt::Display for Placeholder<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Placeholder::AnyIdentifier(sep) => fmt_pattern!(@sep f, "id", sep),
            Placeholder::NumberIdentifier(sep) => fmt_pattern!(@sep f, "number", sep),
            Placeholder::CommitIdentifier(sep) => fmt_pattern!(@sep f, "commit", sep),
            Placeholder::OtherIdentifier => fmt_pattern!(@nosep f, "other"),
            Placeholder::Issues(sep) => fmt_pattern!(@sep f, "issues", sep),
            Placeholder::Commits(sep) => fmt_pattern!(@sep f, "commits", sep),
            Placeholder::Breaking => fmt_pattern!(@nosep f, "breaking"),
            Placeholder::Description => fmt_pattern!(@nosep f, "description"),
            Placeholder::Date => fmt_pattern!(@nosep f, "date"),
            Placeholder::Wildcard => fmt_pattern!(@nosep f, "*"),
        }
    }
}

macro_rules! define_pattern {
    (@base) => {
        use serde::{Serialize, Serializer};

        /// An item within the pattern: a string or placeholder.
        #[derive(Debug, Clone, Eq, PartialEq)]
        pub enum Item<'a> {
            /// The contents of the string data
            Str(&'a str),
            /// A placeholder to replace when converting.
            Placeholder(Placeholder<'a>),
        }

        impl<'a> fmt::Display for Item<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    Item::Str(value) => f.write_str(value),
                    Item::Placeholder(value) => f.write_fmt(format_args!("{{{value}}}")),
                }
            }
        }

        /// A pattern to parse and format changelogs.
        #[derive(Debug, Clone, Eq, PartialEq)]
        pub struct Pattern<'a>(Vec<Item<'a>>);

        impl<'a> Pattern<'a> {
            /// Create a new, empty pattern.
            pub const fn new() -> Pattern<'a> {
                Pattern(vec![])
            }
        }

        impl<'a> fmt::Display for Pattern<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                for item in &self.0 {
                    item.fmt(f)?;
                }
                Ok(())
            }
        }

        impl<'a> Serialize for Pattern<'a> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        /// Parse and format patterns.
        #[derive(Debug, Clone, Eq, PartialEq)]
        pub struct Converter<'a> {
            pattern: Pattern<'a>,
        }

        impl<'a> Converter<'a> {
            /// Create a new converter from a pattern.
            pub const fn new(pattern: Pattern<'a>) -> Converter<'a> {
                Converter { pattern }
            }
        }
    };

    (@sep) => {
        define_pattern!(@base);

        impl<'a> Pattern<'a> {
            /// Parse the pattern from a specifier string.
            pub fn parse(specifier: &'a str, default_sep: &'a str) -> Result<Pattern<'a>> {
                Ok(Pattern(crate::pattern::parse_pattern(
                    specifier,
                    |s| Ok(Item::Placeholder(Placeholder::parse(s, default_sep)?)),
                    |s| Ok(Item::Str(s)),
                )?))
            }
        }
    };
}

define_pattern!(@sep);

macro_rules! pattern_error {
    (@empty) => {
        eyre::bail!("cannot have empty placeholder.")
    };
    (@nested) => {
        eyre::bail!("cannot have nested start indicators.")
    };
    (@start) => {
        eyre::bail!("cannot have start indicator without end.")
    };
    (@end) => {
        eyre::bail!("cannot have end indicator without start.")
    };
}

// extracts a group with a start/end where the value
// cannot be escaped (ie, `[[...]]` cannot exist).
// returns the index to the index of the start and
// index to the last element.
#[track_caller]
fn extract_unescaped(bytes: &[u8], start: u8, end: u8) -> Result<Option<(usize, usize)>> {
    let mut i = 0;
    let mut inside = false;
    for (j, &b) in bytes.iter().enumerate() {
        let is_start = b == start;
        let is_end = b == end;
        match (is_start, is_end, inside) {
            (true, false, true) => pattern_error!(@nested),
            (true, false, false) => {
                i = j;
                inside = true;
            }
            (false, true, true) => match j == i + 1 {
                true => pattern_error!(@empty),
                false => return Ok(Some((i, j))),
            },
            (false, true, false) => pattern_error!(@end),
            _ => (),
        }
    }
    match inside {
        true => pattern_error!(@start),
        false => Ok(None),
    }
}

// extracts an unescaped group only if it happens
// at the end of a string.
#[track_caller]
pub(crate) fn extract_unescaped_at_end(bytes: &[u8], start: u8, end: u8) -> Result<Option<usize>> {
    match extract_unescaped(bytes, start, end)? {
        // note: `j` cannot be 0.
        Some((i, j)) if j == bytes.len() - 1 => Ok(Some(i)),
        Some(_) => (eyre::bail!("extracted group must be at the end of the string.")),
        None => Ok(None),
    }
}

// extracts a group with a start/end where the value
// cannot be escaped (ie, `{{...}}` can exist).
// the compiler has an incorrect "never read" warning
#[track_caller]
#[allow(unused_assignments)]
fn extract_escaped(bytes: &[u8], start: u8, end: u8) -> Result<Option<(usize, usize)>> {
    let mut i = 0;
    let mut previous: u8 = b'\0';
    let mut inside = false;
    let mut outside = false;
    for (j, &b) in bytes.iter().enumerate() {
        if b == start {
            let was_start = previous == start;
            if inside && !was_start {
                pattern_error!(@nested);
            }
            // need the inside check for `{{{...}`
            inside = !(inside && was_start);
            match inside {
                true => i = j,
                // `{{{pr}` is valid, and needs to be handled
                false => {
                    i = 0;
                    previous = b'\0';
                }
            }
        } else if b == end {
            if inside && j == 1 {
                pattern_error!(@empty);
            } else if inside {
                return Ok(Some((i, j)));
            } else if outside && previous != end {
                pattern_error!(@end);
            } else if outside {
                outside = false;
            } else {
                outside = true;
            }
        } else if outside {
            pattern_error!(@end);
        }
        previous = b;
    }

    match (inside, outside) {
        (true, _) => pattern_error!(@start),
        (_, true) => pattern_error!(@end),
        _ => Ok(None),
    }
}

/// Parse a generic pattern specifier.
pub fn parse_pattern<'a, T>(
    specifier: &'a str,
    placeholder: impl Fn(&'a str) -> Result<T>,
    string: impl Fn(&'a str) -> Result<T>,
) -> Result<Vec<T>> {
    // we want to have a syntax **fairly** consistent with
    // rust's format strings, only slightly different. we
    // just use the `{name}` syntax, where a separator for
    // some can be specified with `{name[sep]}`. escapes
    // can be done `{{...}}`. the `[sep]` was chosen since
    // `[...]` is generally a link replacement in markdown.
    //
    // we can use bytes since all our control characters
    // are valid ASCII, and our specifier is valid UTF-8.
    let mut bytes = specifier.as_bytes();
    let mut result = vec![];

    while !bytes.is_empty() {
        match extract_escaped(bytes, b'{', b'}')? {
            Some((start, end)) => {
                if start != 0 {
                    let leading = bytes[..start].to_utf8()?;
                    result.push(string(leading)?);
                }
                let extracted = bytes[start + 1..end].to_utf8()?;
                result.push(placeholder(extracted)?);
                bytes = &bytes[end + 1..];
            }
            None => {
                result.push(string(bytes.to_utf8()?)?);
                bytes = &[];
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder_parse() -> Result<()> {
        let sep = ",";
        let parse = |s| Placeholder::parse(s, sep);
        assert_eq!(parse("id")?, Placeholder::AnyIdentifier(sep));
        assert_eq!(parse("number")?, Placeholder::NumberIdentifier(sep));
        assert_eq!(parse("commit")?, Placeholder::CommitIdentifier(sep));
        assert_eq!(parse("other")?, Placeholder::OtherIdentifier);
        assert_eq!(parse("issues")?, Placeholder::Issues(sep));
        assert_eq!(parse("commits")?, Placeholder::Commits(sep));
        assert_eq!(parse("breaking")?, Placeholder::Breaking);
        assert_eq!(parse("description")?, Placeholder::Description);

        assert_eq!(parse("id[-]")?, Placeholder::AnyIdentifier("-"));
        assert_eq!(parse("number[_]")?, Placeholder::NumberIdentifier("_"));
        assert_eq!(parse("commit[/]")?, Placeholder::CommitIdentifier("/"));
        assert_eq!(parse("issues[-_]")?, Placeholder::Issues("-_"));
        assert_eq!(parse("commits[//]")?, Placeholder::Commits("//"));

        assert!(parse("other[/]").is_err());
        assert!(parse("breaking[/]").is_err());
        assert!(parse("description[/]").is_err());

        assert!(parse("idx").is_err());
        assert!(parse("others").is_err());
        assert!(parse("hey[,]").is_err());

        Ok(())
    }

    macro_rules! s {
        ($s:literal) => {
            $s.to_owned()
        };
    }

    #[test]
    fn test_placeholder_to_string() {
        assert_eq!(Placeholder::AnyIdentifier(",").to_string(), s!("id[,]"));
        assert_eq!(
            Placeholder::NumberIdentifier("/").to_string(),
            s!("number[/]")
        );
        assert_eq!(
            Placeholder::CommitIdentifier("-").to_string(),
            s!("commit[-]")
        );
        assert_eq!(Placeholder::OtherIdentifier.to_string(), s!("other"));
        assert_eq!(Placeholder::Issues("//").to_string(), s!("issues[//]"));
        assert_eq!(Placeholder::Commits("+").to_string(), s!("commits[+]"));
        assert_eq!(Placeholder::Breaking.to_string(), s!("breaking"));
        assert_eq!(Placeholder::Description.to_string(), s!("description"));
    }

    #[test]
    fn test_pattern_item_to_string() {
        assert_eq!(Item::Str("prefix").to_string(), s!("prefix"));
        assert_eq!(
            Item::Placeholder(Placeholder::Commits("+")).to_string(),
            s!("{commits[+]}")
        );
    }

    #[test]
    fn test_pattern_parse() -> Result<()> {
        let sep = ",";
        let parse = |s| Pattern::parse(s, sep);
        assert_eq!(
            parse("issue{id}suffix")?,
            Pattern(vec![
                Item::Str("issue"),
                Item::Placeholder(Placeholder::AnyIdentifier(",")),
                Item::Str("suffix"),
            ]),
        );

        assert_eq!(
            parse("issue{number[/]}suffix")?,
            Pattern(vec![
                Item::Str("issue"),
                Item::Placeholder(Placeholder::NumberIdentifier("/")),
                Item::Str("suffix"),
            ]),
        );

        Ok(())
    }

    #[test]
    fn test_pattern_to_string() {
        assert_eq!(
            Pattern(vec![
                Item::Str("issue"),
                Item::Placeholder(Placeholder::AnyIdentifier(",")),
                Item::Str("suffix"),
            ])
            .to_string(),
            s!("issue{id[,]}suffix")
        );

        assert_eq!(
            Pattern(vec![
                Item::Str("issue"),
                Item::Placeholder(Placeholder::NumberIdentifier("/")),
                Item::Str("suffix"),
            ])
            .to_string(),
            s!("issue{number[/]}suffix")
        );
    }

    #[test]
    fn test_extract_unescaped() -> Result<()> {
        let extract = |bytes| extract_unescaped(bytes, b'[', b']');
        assert_eq!(extract(b"pr[x]")?, Some((2, 4)));
        assert_eq!(extract(b"[x]aaa")?, Some((0, 2)));
        assert!(extract(b"[]").is_err());
        assert!(extract(b"[aaaaa").is_err());
        assert!(extract(b"[aaa[aa]]").is_err());
        assert!(extract(b"aaa]bbb").is_err());
        assert!(extract(b"]bbb").is_err());
        assert!(extract(b"]b[b]b").is_err());
        assert!(extract(b"abgahaha")?.is_none());

        // these are fine, since they extract the first one.
        assert_eq!(extract(b"[x]aa]a")?, Some((0, 2)));
        assert_eq!(extract(b"[x]aa[a")?, Some((0, 2)));

        Ok(())
    }

    #[test]
    fn test_extract_unescaped_at_end() -> Result<()> {
        let extract = |bytes| extract_unescaped_at_end(bytes, b'[', b']');
        assert_eq!(extract(b"pr[x]")?, Some(2));
        assert_eq!(extract(b"[x]")?, Some(0));
        assert!(extract(b"[x]aaa").is_err());
        assert!(extract(b"[]").is_err());
        assert!(extract(b"[aaaaa").is_err());
        assert!(extract(b"[aaa[aa]]").is_err());
        assert!(extract(b"aaa]bbb").is_err());
        assert!(extract(b"]bbb").is_err());
        assert!(extract(b"]b[b]b").is_err());
        assert!(extract(b"abgahaha")?.is_none());

        Ok(())
    }

    #[test]
    fn test_extract_escaped() -> Result<()> {
        let extract = |bytes| extract_escaped(bytes, b'{', b'}');
        assert_eq!(extract(b"pr{x}")?, Some((2, 4)));
        assert_eq!(extract(b"pr{{x")?, None);
        assert_eq!(extract(b"pr{{x}}")?, None);
        assert_eq!(extract(b"pr{{{x}")?, Some((4, 6)));
        assert_eq!(extract(b"pr}}")?, None);
        assert_eq!(extract(b"pr{[x]}")?, Some((2, 6)));

        assert!(extract(b"pr}").is_err());
        assert!(extract(b"pr{{}").is_err());
        assert!(extract(b"pr{{{").is_err());
        assert!(extract(b"pr}}{").is_err());
        assert!(extract(b"pr{").is_err());
        assert!(extract(b"pr{x{}").is_err());
        assert!(extract(b"pr{x{}").is_err());
        assert!(extract(b"abgahaha")?.is_none());

        // these are fine, since they extract the first one.
        assert_eq!(extract(b"{x}aa}a")?, Some((0, 2)));
        assert_eq!(extract(b"{x}aa{a")?, Some((0, 2)));
        assert_eq!(extract(b"pr}}{x}{a")?, Some((4, 6)));

        Ok(())
    }
}
