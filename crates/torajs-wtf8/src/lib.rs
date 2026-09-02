//! WTF-8 — the "Wobbly Transformation Format" (Simon Sapin): UTF-8
//! generalized so surrogate code points U+D800..U+DFFF take their
//! natural 3-byte spelling (`ED A0..BF xx`). Any sequence of UTF-16
//! code units — an ECMAScript String value per §6.1.4 — then has one
//! canonical byte form, lone surrogates included.
//!
//! Well-formedness rule (the one that makes byte equality equal
//! code-unit equality): a surrogate *pair* is never spelled as two
//! 3-byte sequences; `Wtf8Buf::push_code_point` / `push_wtf8` join a
//! trailing high surrogate with a leading low one into the 4-byte
//! supplementary form. Valid UTF-8 is a strict subset, so a buffer
//! without lone surrogates round-trips to `&str` for free.
//!
//! Textbook precedents: Rust std's `sys_common::wtf8` (Windows
//! `OsString`), swc's `Wtf8Atom` for JS string literals.

#![no_std]

extern crate alloc;

mod buf;
mod decode;

pub use buf::{Wtf8Buf, push_code_point};
pub use decode::CodePoints;

use alloc::borrow::{Cow, ToOwned};
use alloc::string::String;
use core::fmt;

/// Borrowed WTF-8 slice. Bytes are always well-formed WTF-8 — the
/// only constructors are `new` and the owning [`Wtf8Buf`].
#[repr(transparent)]
pub struct Wtf8 {
    bytes: [u8],
}

impl Wtf8 {
    /// Every `&str` is WTF-8 as it stands.
    #[inline]
    pub fn new(s: &str) -> &Wtf8 {
        // SAFETY: `Wtf8` is `repr(transparent)` over `[u8]`, and
        // valid UTF-8 is well-formed WTF-8.
        unsafe { &*(s.as_bytes() as *const [u8] as *const Wtf8) }
    }

    /// `bytes` must be well-formed WTF-8 (produced by this crate).
    #[inline]
    pub(crate) fn from_bytes_unchecked(bytes: &[u8]) -> &Wtf8 {
        // SAFETY: `repr(transparent)`; well-formedness is the caller's
        // invariant and every caller is inside this crate.
        unsafe { &*(bytes as *const [u8] as *const Wtf8) }
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Byte length (not code units — see `code_units`).
    #[inline]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    #[inline]
    pub fn is_ascii(&self) -> bool {
        self.bytes.is_ascii()
    }

    /// `Some` iff the buffer is valid UTF-8 — i.e. holds no lone
    /// surrogate. std's validator rejects exactly the `ED A0..BF`
    /// sequences WTF-8 adds, so it is the precise test.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.bytes).ok()
    }

    #[inline]
    pub fn has_lone_surrogates(&self) -> bool {
        self.as_str().is_none()
    }

    /// Each lone surrogate replaced by U+FFFD. Borrows when there is
    /// nothing to replace.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        if let Some(s) = self.as_str() {
            return Cow::Borrowed(s);
        }
        let mut out = String::with_capacity(self.bytes.len());
        for cp in self.code_points() {
            out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
        }
        Cow::Owned(out)
    }

    /// Code points in order; surrogates come out as their own value
    /// (0xD800..=0xDFFF), supplementary characters as one value.
    #[inline]
    pub fn code_points(&self) -> CodePoints<'_> {
        CodePoints::new(&self.bytes)
    }

    /// UTF-16 code units — the ECMAScript view of the string.
    #[inline]
    pub fn code_units(&self) -> impl Iterator<Item = u16> + '_ {
        self.code_points().flat_map(decode::cp_to_units)
    }

    /// Number of UTF-16 code units (`String.prototype.length`).
    #[inline]
    pub fn code_unit_len(&self) -> usize {
        self.code_points()
            .map(|cp| usize::from(cp > 0xFFFF) + 1)
            .sum()
    }

    #[inline]
    pub fn starts_with(&self, prefix: &str) -> bool {
        self.bytes.starts_with(prefix.as_bytes())
    }

    #[inline]
    pub fn ends_with(&self, suffix: &str) -> bool {
        self.bytes.ends_with(suffix.as_bytes())
    }

    /// The rest after `prefix`, or `None` when it is not a prefix.
    /// A UTF-8 prefix ends on a code-point boundary and carries no
    /// surrogate, so the remainder is well-formed on its own.
    #[inline]
    pub fn strip_prefix(&self, prefix: &str) -> Option<&Wtf8> {
        self.bytes
            .strip_prefix(prefix.as_bytes())
            .map(Wtf8::from_bytes_unchecked)
    }

    /// True iff the last code point is a high surrogate — the only
    /// state in which appending a low surrogate must join.
    #[inline]
    pub(crate) fn ends_with_high_surrogate(&self) -> bool {
        let n = self.bytes.len();
        n >= 3 && self.bytes[n - 3] == 0xED && (0xA0..=0xAF).contains(&self.bytes[n - 2])
    }

    /// True iff the first code point is a low surrogate.
    #[inline]
    pub(crate) fn starts_with_low_surrogate(&self) -> bool {
        self.bytes.len() >= 3 && self.bytes[0] == 0xED && (0xB0..=0xBF).contains(&self.bytes[1])
    }
}

impl ToOwned for Wtf8 {
    type Owned = Wtf8Buf;
    #[inline]
    fn to_owned(&self) -> Wtf8Buf {
        Wtf8Buf::from_bytes_unchecked(self.bytes.to_owned())
    }
}

impl PartialEq for Wtf8 {
    #[inline]
    fn eq(&self, other: &Wtf8) -> bool {
        self.bytes == other.bytes
    }
}
impl Eq for Wtf8 {}

impl PartialOrd for Wtf8 {
    #[inline]
    fn partial_cmp(&self, other: &Wtf8) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Wtf8 {
    #[inline]
    fn cmp(&self, other: &Wtf8) -> core::cmp::Ordering {
        self.bytes.cmp(&other.bytes)
    }
}

impl core::hash::Hash for Wtf8 {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.bytes.hash(state)
    }
}

impl PartialEq<str> for Wtf8 {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        &self.bytes == other.as_bytes()
    }
}
impl PartialEq<&str> for Wtf8 {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        &self.bytes == other.as_bytes()
    }
}
impl PartialEq<Wtf8> for str {
    #[inline]
    fn eq(&self, other: &Wtf8) -> bool {
        self.as_bytes() == &other.bytes
    }
}

impl AsRef<Wtf8> for Wtf8 {
    #[inline]
    fn as_ref(&self) -> &Wtf8 {
        self
    }
}
impl AsRef<Wtf8> for str {
    #[inline]
    fn as_ref(&self) -> &Wtf8 {
        Wtf8::new(self)
    }
}
impl AsRef<Wtf8> for String {
    #[inline]
    fn as_ref(&self) -> &Wtf8 {
        Wtf8::new(self)
    }
}
impl AsRef<[u8]> for Wtf8 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

/// Lossy — lone surrogates show as U+FFFD.
impl fmt::Display for Wtf8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

/// Like `str`'s `Debug` but a lone surrogate prints as `\u{d800}`.
impl fmt::Debug for Wtf8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;
        for cp in self.code_points() {
            match char::from_u32(cp) {
                Some(c) => {
                    for e in c.escape_debug() {
                        fmt::Write::write_char(f, e)?;
                    }
                }
                None => write!(f, "\\u{{{cp:x}}}")?,
            }
        }
        f.write_str("\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn strip_prefix_keeps_lone_surrogate_tail() {
        let mut b = Wtf8Buf::new();
        b.push_str("__getter_");
        b.push_code_point(0xD800);
        let tail = b.strip_prefix("__getter_").unwrap();
        assert_eq!(tail.as_bytes(), &[0xED, 0xA0, 0x80]);
        assert!(tail.has_lone_surrogates());
        assert!(b.strip_prefix("__setter_").is_none());
        assert_eq!(Wtf8::new("ab").strip_prefix("").unwrap(), Wtf8::new("ab"));
    }

    #[test]
    fn str_view_is_zero_cost_and_equal() {
        let w = Wtf8::new("héllo");
        assert_eq!(w.as_bytes(), "héllo".as_bytes());
        assert_eq!(w.as_str(), Some("héllo"));
        assert!(*w == *"héllo");
        assert!(!w.has_lone_surrogates());
        assert_eq!(w.code_unit_len(), 5);
    }

    #[test]
    fn lone_surrogate_is_not_str_but_is_lossy_displayable() {
        let mut b = Wtf8Buf::new();
        b.push_str("a");
        b.push_code_point(0xD800);
        b.push_str("b");
        assert_eq!(b.as_bytes(), &[b'a', 0xED, 0xA0, 0x80, b'b']);
        assert!(b.as_str().is_none());
        assert!(b.has_lone_surrogates());
        assert_eq!(b.to_string_lossy(), "a\u{FFFD}b");
        assert_eq!(format!("{b}"), "a\u{FFFD}b");
        assert_eq!(format!("{b:?}"), "\"a\\u{d800}b\"");
        assert_eq!(
            b.code_units().collect::<alloc::vec::Vec<_>>(),
            [0x61, 0xD800, 0x62]
        );
        assert_eq!(b.code_unit_len(), 3);
    }

    #[test]
    fn supplementary_counts_two_code_units() {
        let w = Wtf8::new("𝒢");
        assert_eq!(w.code_points().collect::<alloc::vec::Vec<_>>(), [0x1D4A2]);
        assert_eq!(
            w.code_units().collect::<alloc::vec::Vec<_>>(),
            [0xD835, 0xDCA2]
        );
        assert_eq!(w.code_unit_len(), 2);
    }
}
