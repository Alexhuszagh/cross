use std::fmt;
use std::io::Write;
use std::marker::PhantomData;

use crate::stream::{consume_if_str, take_while};
use chrono::{Datelike, Utc};
use eyre::Result;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

/// A calendar date.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    month: u32,
    day: u32,
}

/// Get the date as a year/month/day tuple.
pub fn get_current_date() -> Date {
    let utc = Utc::now();
    let date = utc.date();

    Date {
        year: date.year(),
        month: date.month(),
        day: date.day(),
    }
}

/// A placeholder for a date.
#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum Placeholder<'a> {
    Day,
    Month,
    Year,
    _Phantom(PhantomData<&'a ()>),
}

impl<'a> Placeholder<'a> {
    /// Parse the placeholder from the format specifier.
    pub fn parse(s: &str) -> Result<Placeholder<'a>> {
        Ok(match s {
            "day" => Placeholder::Day,
            "month" => Placeholder::Month,
            "year" => Placeholder::Year,
            _ => eyre::bail!("got unsupported date placeholder \"{s}\""),
        })
    }
}

impl<'a> fmt::Display for Placeholder<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Placeholder::Day => f.write_str("day"),
            Placeholder::Month => f.write_str("month"),
            Placeholder::Year => f.write_str("year"),
            _ => unreachable!(),
        }
    }
}

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

    /// Parse the pattern from a specifier string.
    pub fn parse(specifier: &'a str) -> Result<Pattern<'a>> {
        Ok(Pattern(crate::pattern::parse_pattern(
            specifier,
            |s| Ok(Item::Placeholder(Placeholder::parse(s)?)),
            |s| Ok(Item::Str(s)),
        )?))
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

impl<'de: 'a, 'a> Deserialize<'de> for Pattern<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        Self::parse(s).map_err(de::Error::custom)
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

    /// Write date to sink.
    pub fn write(&self, sink: &mut impl Write, date: &Date) -> Result<()> {
        for item in &self.pattern.0 {
            match item {
                Item::Str(v) => sink.write_all(v.as_bytes())?,
                Item::Placeholder(v) => match v {
                    Placeholder::Day => sink.write_fmt(format_args!("{}", date.day))?,
                    Placeholder::Month => sink.write_fmt(format_args!("{}", date.month))?,
                    Placeholder::Year => sink.write_fmt(format_args!("{}", date.year))?,
                    _ => unreachable!(),
                },
            }
        }

        Ok(())
    }

    /// Write date to string.
    pub fn to_string(&self, date: &Date) -> Result<String> {
        let mut buffer = Vec::new();
        self.write(&mut buffer, date)?;

        String::from_utf8(buffer).map_err(Into::into)
    }

    /// Read data from buffer.
    pub fn parse(&self, mut s: &'a str) -> Result<(Date, &'a str)> {
        let mut date = Date::default();
        for item in &self.pattern.0 {
            match item {
                Item::Str(v) => s = consume_if_str(s, v)?,
                Item::Placeholder(v) => {
                    let (found, rest) = take_while(s, char::is_ascii_digit);
                    match v {
                        Placeholder::Day => date.day = found.parse()?,
                        Placeholder::Month => date.month = found.parse()?,
                        Placeholder::Year => date.year = found.parse()?,
                        _ => unreachable!(),
                    }
                    s = rest;
                }
            }
        }

        Ok((date, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! s {
        ($s:literal) => {
            $s.to_owned()
        };
    }

    #[test]
    fn test_pattern_parse() -> Result<()> {
        assert_eq!(
            Pattern::parse("{year}-{month}-{day}")?,
            Pattern(vec![
                Item::Placeholder(Placeholder::Year),
                Item::Str("-"),
                Item::Placeholder(Placeholder::Month),
                Item::Str("-"),
                Item::Placeholder(Placeholder::Day),
            ]),
        );

        assert_eq!(
            Pattern::parse("{day}/{month}/{year}")?,
            Pattern(vec![
                Item::Placeholder(Placeholder::Day),
                Item::Str("/"),
                Item::Placeholder(Placeholder::Month),
                Item::Str("/"),
                Item::Placeholder(Placeholder::Year),
            ]),
        );

        Ok(())
    }

    #[test]
    fn test_convert_date() -> Result<()> {
        let conv = Converter::new(Pattern::parse("{day}/{month}/{year}")?);
        let string = s!("23/4/2012");
        let date = Date {
            year: 2012,
            month: 4,
            day: 23,
        };
        assert_eq!(conv.to_string(&date)?, string);
        assert_eq!(conv.parse(&string)?, (date, ""));
        assert_eq!(
            conv.parse(&format!("{string} some trail"))?,
            (date, " some trail")
        );

        let conv = Converter::new(Pattern::parse("{year}-{month}-{day}")?);
        let string = s!("2012-4-23");
        assert_eq!(conv.to_string(&date)?, string);
        assert_eq!(conv.parse(&string)?, (date, ""));
        assert_eq!(
            conv.parse(&format!("{string} some trail"))?,
            (date, " some trail")
        );

        let conv = Converter::new(Pattern::parse("{year}-{month}-{day}")?);
        assert!(conv.parse("23/4/2012").is_err());

        Ok(())
    }

    #[test]
    fn test_serde_pattern() -> Result<()> {
        let string = "{year}-{month}-{day}";
        let pattern = Pattern::parse(string)?;

        let expected = format!("\"{string}\"");
        let actual: Pattern<'_> = serde_json::from_str(&expected)?;
        assert_eq!(serde_json::to_string(&pattern)?, expected);
        assert_eq!(actual, pattern);

        Ok(())
    }
}
