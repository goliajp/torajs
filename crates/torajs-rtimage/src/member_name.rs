//! An archive member's name, kept as the bytes it was read from.

use std::fmt;

/// The name of an archive member: the bytes of its archive header, or
/// of the runtime image's name region. Both producers start from a
/// `&str` (the archive parser checks the header bytes as UTF-8, the
/// image builder copies a parsed name), so a link compares, hashes and
/// copies it as bytes and spells it as text only when it prints it.
/// Byte order is the order of the `&str` it was made from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemberName<'a>(&'a [u8]);

impl<'a> MemberName<'a> {
    pub fn new(name: &'a str) -> Self {
        MemberName(name.as_bytes())
    }

    /// A name the image builder wrote from a `&str`.
    pub(crate) fn from_image(bytes: &'a [u8]) -> Self {
        MemberName(bytes)
    }

    pub fn as_bytes(self) -> &'a [u8] {
        self.0
    }

    /// Whether `part` occurs anywhere in the name.
    pub fn contains(self, part: &str) -> bool {
        self.0.windows(part.len()).any(|w| w == part.as_bytes())
    }

    pub fn starts_with(self, prefix: &str) -> bool {
        self.0.starts_with(prefix.as_bytes())
    }
}

impl fmt::Display for MemberName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(self.0))
    }
}

impl fmt::Debug for MemberName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*String::from_utf8_lossy(self.0), f)
    }
}

impl PartialEq<&str> for MemberName<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_and_prints_as_its_str() {
        let mut names = ["b.o", "a-long.o", "a.o", "ä.o"];
        let mut wrapped: Vec<MemberName<'_>> = names.iter().map(|n| MemberName::new(n)).collect();
        names.sort();
        wrapped.sort();
        let printed: Vec<String> = wrapped.iter().map(|n| n.to_string()).collect();
        assert_eq!(printed, names);
        assert_eq!(format!("{:?}", MemberName::new("x.o")), "\"x.o\"");
        assert!(MemberName::new("rc.o") == "rc.o");
        assert!(MemberName::new("rc.o").starts_with("rc"));
    }
}
