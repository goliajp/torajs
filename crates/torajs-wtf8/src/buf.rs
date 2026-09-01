//! Owned WTF-8 buffer. The one place well-formedness is enforced:
//! a low surrogate appended after a trailing high surrogate joins
//! into the 4-byte supplementary spelling, never two 3-byte ones.

use alloc::borrow::Borrow;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Deref;

use crate::Wtf8;
use crate::decode::{encode_cp, is_well_formed};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Wtf8Buf {
    bytes: Vec<u8>,
}

impl Wtf8Buf {
    #[inline]
    pub fn new() -> Self {
        Wtf8Buf { bytes: Vec::new() }
    }

    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        Wtf8Buf {
            bytes: Vec::with_capacity(n),
        }
    }

    #[inline]
    pub fn from_string(s: String) -> Self {
        Wtf8Buf {
            bytes: s.into_bytes(),
        }
    }

    #[inline]
    pub(crate) fn from_bytes_unchecked(bytes: Vec<u8>) -> Self {
        Wtf8Buf { bytes }
    }

    /// The `String::from_utf8` of WTF-8: accepts bytes assembled by
    /// [`push_code_point`] plus raw UTF-8, rejects anything that is
    /// not well-formed WTF-8 (malformed sequences, overlongs, or a
    /// surrogate pair spelled as two 3-byte sequences).
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Wtf8Buf, Vec<u8>> {
        if is_well_formed(&bytes) {
            Ok(Wtf8Buf { bytes })
        } else {
            Err(bytes)
        }
    }

    #[inline]
    pub fn as_wtf8(&self) -> &Wtf8 {
        Wtf8::from_bytes_unchecked(&self.bytes)
    }

    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// `Ok` iff no lone surrogate is present; otherwise hands the
    /// buffer back untouched.
    pub fn into_string(self) -> Result<String, Wtf8Buf> {
        match String::from_utf8(self.bytes) {
            Ok(s) => Ok(s),
            Err(e) => Err(Wtf8Buf {
                bytes: e.into_bytes(),
            }),
        }
    }

    /// Lossy owned copy: each lone surrogate becomes U+FFFD.
    #[inline]
    pub fn to_string_lossy_owned(&self) -> String {
        self.as_wtf8().to_string_lossy().into_owned()
    }

    #[inline]
    pub fn push_str(&mut self, s: &str) {
        self.bytes.extend_from_slice(s.as_bytes());
    }

    /// Append one code point. A low surrogate landing right after a
    /// high one collapses the pair into its supplementary code point
    /// (§11.8.4 SV: two escapes spelling a pair are one code point).
    #[inline]
    pub fn push_code_point(&mut self, cp: u32) {
        push_code_point(&mut self.bytes, cp);
    }

    /// Append a WTF-8 slice, joining across the seam when needed.
    pub fn push_wtf8(&mut self, other: &Wtf8) {
        if self.as_wtf8().ends_with_high_surrogate() && other.starts_with_low_surrogate() {
            let mut cps = other.code_points();
            self.push_code_point(cps.next().unwrap());
            self.bytes.extend_from_slice(&other.as_bytes()[3..]);
        } else {
            self.bytes.extend_from_slice(other.as_bytes());
        }
    }
}

/// Append one code point to a byte buffer being assembled as WTF-8
/// (finish it with [`Wtf8Buf::from_bytes`]). Same join rule as
/// [`Wtf8Buf::push_code_point`] — this is that method's body, exposed
/// for lexers that interleave raw source bytes with decoded escapes.
pub fn push_code_point(bytes: &mut Vec<u8>, cp: u32) {
    let cp = if (0xDC00..=0xDFFF).contains(&cp)
        && Wtf8::from_bytes_unchecked(bytes).ends_with_high_surrogate()
    {
        let n = bytes.len();
        // `ED A0..AF xx` spells 0xD800 + ((b1 & 0x0F) << 6 | (b2 & 0x3F)).
        let hi = ((bytes[n - 2] & 0x0F) as u32) << 6 | (bytes[n - 1] & 0x3F) as u32;
        bytes.truncate(n - 3);
        0x10000 + (hi << 10) + (cp - 0xDC00)
    } else {
        cp
    };
    let mut tmp = [0u8; 4];
    let n = encode_cp(cp, &mut tmp);
    bytes.extend_from_slice(&tmp[..n]);
}

impl Deref for Wtf8Buf {
    type Target = Wtf8;
    #[inline]
    fn deref(&self) -> &Wtf8 {
        self.as_wtf8()
    }
}

impl Borrow<Wtf8> for Wtf8Buf {
    #[inline]
    fn borrow(&self) -> &Wtf8 {
        self.as_wtf8()
    }
}

impl AsRef<Wtf8> for Wtf8Buf {
    #[inline]
    fn as_ref(&self) -> &Wtf8 {
        self.as_wtf8()
    }
}

impl From<String> for Wtf8Buf {
    #[inline]
    fn from(s: String) -> Self {
        Wtf8Buf::from_string(s)
    }
}
impl From<&str> for Wtf8Buf {
    #[inline]
    fn from(s: &str) -> Self {
        Wtf8Buf {
            bytes: s.as_bytes().to_vec(),
        }
    }
}
impl From<&Wtf8> for Wtf8Buf {
    #[inline]
    fn from(w: &Wtf8) -> Self {
        Wtf8Buf {
            bytes: w.as_bytes().to_vec(),
        }
    }
}

impl PartialEq<str> for Wtf8Buf {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.bytes == other.as_bytes()
    }
}
impl PartialEq<&str> for Wtf8Buf {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.bytes == other.as_bytes()
    }
}
impl PartialEq<String> for Wtf8Buf {
    #[inline]
    fn eq(&self, other: &String) -> bool {
        self.bytes == other.as_bytes()
    }
}
impl PartialEq<Wtf8> for Wtf8Buf {
    #[inline]
    fn eq(&self, other: &Wtf8) -> bool {
        self.bytes == other.as_bytes()
    }
}
impl PartialEq<Wtf8Buf> for str {
    #[inline]
    fn eq(&self, other: &Wtf8Buf) -> bool {
        self.as_bytes() == other.bytes
    }
}

impl fmt::Display for Wtf8Buf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_wtf8(), f)
    }
}
impl fmt::Debug for Wtf8Buf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_wtf8(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn escaped_pair_equals_raw_supplementary() {
        let mut b = Wtf8Buf::new();
        b.push_code_point(0xD835);
        b.push_code_point(0xDCA2);
        assert_eq!(b.as_bytes(), "𝒢".as_bytes());
        assert_eq!(b.as_str(), Some("𝒢"));
        assert_eq!(b, *"𝒢");
    }

    #[test]
    fn lone_halves_stay_lone() {
        let mut b = Wtf8Buf::new();
        b.push_code_point(0xDC00);
        b.push_code_point(0xD800);
        assert_eq!(b.as_bytes(), &[0xED, 0xB0, 0x80, 0xED, 0xA0, 0x80]);
        assert!(b.as_str().is_none());
        assert_eq!(b.code_units().collect::<Vec<_>>(), [0xDC00, 0xD800]);
    }

    #[test]
    fn push_wtf8_joins_across_the_seam() {
        let mut hi = Wtf8Buf::new();
        hi.push_code_point(0xD83D);
        let mut lo = Wtf8Buf::new();
        lo.push_code_point(0xDE00);
        lo.push_str("!");
        hi.push_wtf8(&lo);
        assert_eq!(hi.as_str(), Some("😀!"));

        let mut plain = Wtf8Buf::from("ab");
        plain.push_wtf8(Wtf8::new("cd"));
        assert_eq!(plain, "abcd");
    }

    #[test]
    fn into_string_reports_lone_surrogates() {
        assert_eq!(Wtf8Buf::from("ok").into_string().unwrap(), "ok");
        let mut b = Wtf8Buf::from("x");
        b.push_code_point(0xDFFF);
        let back = b.clone().into_string().unwrap_err();
        assert_eq!(back, b);
        assert_eq!(b.to_string_lossy_owned(), "x\u{FFFD}");
    }

    #[test]
    fn from_bytes_validates_shape_and_pair_rule() {
        assert!(Wtf8Buf::from_bytes(b"ok\xC3\xA9".to_vec()).is_ok());
        assert!(Wtf8Buf::from_bytes(vec![0xED, 0xA0, 0x80]).is_ok());
        assert!(Wtf8Buf::from_bytes(vec![0xC3]).is_err());
        assert!(Wtf8Buf::from_bytes(vec![0xC0, 0x80]).is_err());
        assert!(Wtf8Buf::from_bytes(vec![0xED, 0xA0, 0x80, 0xED, 0xB0, 0x80]).is_err());
        let mut v = Vec::new();
        push_code_point(&mut v, 0xD800);
        push_code_point(&mut v, 0xDC00);
        assert_eq!(Wtf8Buf::from_bytes(v).unwrap().as_str(), Some("\u{10000}"));
    }

    #[test]
    fn hash_and_eq_follow_bytes() {
        use alloc::collections::BTreeSet;
        let mut set = BTreeSet::new();
        set.insert(Wtf8Buf::from("a"));
        assert!(set.contains(Wtf8::new("a")));
    }
}
