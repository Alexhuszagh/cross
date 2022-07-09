use eyre::Result;

/// Consume the prefix if bytes starts with prefix
pub fn consume_if_bytes<'a>(bytes: &'a [u8], prefix: &[u8]) -> Result<&'a [u8]> {
    bytes.strip_prefix(prefix).ok_or(eyre::eyre!(
        "unable to strip prefix {prefix:?} for {bytes:?}"
    ))
}

/// Consume the prefix if str starts with prefix.
pub fn consume_if_str<'a>(s: &'a str, prefix: &str) -> Result<&'a str> {
    s.strip_prefix(prefix)
        .ok_or(eyre::eyre!("unable to strip prefix {prefix} for {s}"))
}

/// Take from the str until the condition is not valid.
pub fn take_while(s: &str, cb: impl Fn(&char) -> bool) -> (&str, &str) {
    for (index, c) in s.chars().enumerate() {
        if !cb(&c) {
            return s.split_at(index);
        }
    }

    (s, "")
}
