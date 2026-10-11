//! ECMA-402 IsStructurallyValidLanguageTag (§6.2.1): whether a string
//! is a well-formed `unicode_locale_id` (UTS #35 Part 1 §3.2) that
//! uses none of the BCP 47 backwards-compatibility syntax (§3.3: no
//! `root`, no script-first id, no `_` separator, no irregular
//! grandfathered tag such as `i-klingon`).
//!
//! Pure syntax over ASCII bytes, case-insensitive, no allocation and
//! no registry data: a tag naming a locale nobody ships is still
//! structurally valid. Grammar:
//!
//! ```text
//! unicode_locale_id   = unicode_language_id extensions* pu_extensions?
//! unicode_language_id = language (-script)? (-region)? (-variant)*
//! language            = alpha{2,3} | alpha{5,8}
//! script              = alpha{4}
//! region              = alpha{2} | digit{3}
//! variant             = alphanum{5,8} | digit alphanum{3}
//! extensions          = u_ext | t_ext | other_ext
//! u_ext               = u ((-keyword)+ | (-attribute)+ (-keyword)*)
//! keyword             = key (-type)*      key = alphanum alpha
//! attribute, type     = alphanum{3,8}
//! t_ext               = t ((-tlang (-tfield)*) | (-tfield)+)
//! tlang               = unicode_language_id
//! tfield              = tkey (-alphanum{3,8})+    tkey = alpha digit
//! other_ext           = [singleton other than t u x] (-alphanum{2,8})+
//! pu_extensions       = x (-alphanum{1,8})+
//! ```
//!
//! plus §6.2.1 steps 6, 9.a and 9.c: no duplicate variant in the
//! language id or in `tlang`, no duplicate singleton before `-x-`.

/// ECMA-402 §6.2.1 IsStructurallyValidLanguageTag over the tag's
/// bytes (a non-ASCII byte makes it invalid).
pub fn is_structurally_valid_language_tag(tag: &[u8]) -> bool {
    let mut c = Cursor::new(tag);
    if !language_id(&mut c) {
        return false;
    }
    let mut seen: u64 = 0;
    while let Some(s) = c.bump() {
        if s.len() != 1 || !s[0].is_ascii_alphanumeric() {
            return false;
        }
        let singleton = s[0].to_ascii_lowercase();
        if singleton == b'x' {
            return private_use(&mut c);
        }
        let bit = 1u64 << singleton_index(singleton);
        if seen & bit != 0 {
            return false;
        }
        seen |= bit;
        let ok = match singleton {
            b'u' => unicode_extension(&mut c),
            b't' => transformed_extension(&mut c),
            _ => other_extension(&mut c),
        };
        if !ok {
            return false;
        }
    }
    true
}

/// The `-`-separated subtags of a tag, read front to back. An empty
/// subtag (a leading, doubled or trailing `-`) comes back as `[]`,
/// which no production accepts.
#[derive(Clone)]
struct Cursor<'a> {
    rest: &'a [u8],
    done: bool,
}

impl<'a> Cursor<'a> {
    fn new(tag: &'a [u8]) -> Self {
        Cursor {
            rest: tag,
            done: false,
        }
    }

    fn peek(&self) -> Option<&'a [u8]> {
        self.clone().bump()
    }

    fn bump(&mut self) -> Option<&'a [u8]> {
        if self.done {
            return None;
        }
        match self.rest.iter().position(|&b| b == b'-') {
            Some(p) => {
                let s = &self.rest[..p];
                self.rest = &self.rest[p + 1..];
                Some(s)
            }
            None => {
                self.done = true;
                Some(self.rest)
            }
        }
    }

    /// Consume the next subtag when `pred` accepts it.
    fn eat(&mut self, pred: fn(&[u8]) -> bool) -> bool {
        match self.peek() {
            Some(s) if pred(s) => {
                self.bump();
                true
            }
            _ => false,
        }
    }
}

fn alpha(s: &[u8], min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.iter().all(u8::is_ascii_alphabetic)
}

fn alphanum(s: &[u8], min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.iter().all(u8::is_ascii_alphanumeric)
}

fn is_language(s: &[u8]) -> bool {
    alpha(s, 2, 3) || alpha(s, 5, 8)
}

fn is_script(s: &[u8]) -> bool {
    alpha(s, 4, 4)
}

fn is_region(s: &[u8]) -> bool {
    alpha(s, 2, 2) || (s.len() == 3 && s.iter().all(u8::is_ascii_digit))
}

fn is_variant(s: &[u8]) -> bool {
    alphanum(s, 5, 8) || (s.len() == 4 && s[0].is_ascii_digit() && alphanum(s, 4, 4))
}

fn is_attribute_or_type(s: &[u8]) -> bool {
    alphanum(s, 3, 8)
}

fn is_ukey(s: &[u8]) -> bool {
    s.len() == 2 && s[0].is_ascii_alphanumeric() && s[1].is_ascii_alphabetic()
}

fn is_tkey(s: &[u8]) -> bool {
    s.len() == 2 && s[0].is_ascii_alphabetic() && s[1].is_ascii_digit()
}

fn is_other_subtag(s: &[u8]) -> bool {
    alphanum(s, 2, 8)
}

/// 0..=9 for a digit, 10..=35 for a (lowercase) letter.
fn singleton_index(b: u8) -> u32 {
    if b.is_ascii_digit() {
        (b - b'0') as u32
    } else {
        (b - b'a') as u32 + 10
    }
}

/// `unicode_language_id`, its variants free of duplicates.
fn language_id(c: &mut Cursor<'_>) -> bool {
    if !c.eat(is_language) {
        return false;
    }
    c.eat(is_script);
    c.eat(is_region);
    let first_variant = c.clone();
    let mut n = 0;
    while let Some(v) = c.peek().filter(|s| is_variant(s)) {
        let mut earlier = first_variant.clone();
        for _ in 0..n {
            if earlier.bump().is_some_and(|e| e.eq_ignore_ascii_case(v)) {
                return false;
            }
        }
        c.bump();
        n += 1;
    }
    true
}

/// After `-u`: keywords, or attributes followed by keywords; at least
/// one of either.
fn unicode_extension(c: &mut Cursor<'_>) -> bool {
    let mut any = false;
    while c.eat(is_attribute_or_type) {
        any = true;
    }
    while c.eat(is_ukey) {
        any = true;
        while c.eat(is_attribute_or_type) {}
    }
    any
}

/// After `-t`: a `tlang` and then fields, or fields alone; each field
/// is a key with at least one value.
fn transformed_extension(c: &mut Cursor<'_>) -> bool {
    let mut any = false;
    if c.peek().is_some_and(is_language) {
        if !language_id(c) {
            return false;
        }
        any = true;
    }
    while c.eat(is_tkey) {
        if !c.eat(is_attribute_or_type) {
            return false;
        }
        while c.eat(is_attribute_or_type) {}
        any = true;
    }
    any
}

fn other_extension(c: &mut Cursor<'_>) -> bool {
    if !c.eat(is_other_subtag) {
        return false;
    }
    while c.eat(is_other_subtag) {}
    true
}

/// After `-x`: one or more subtags of 1 to 8 alphanumerics, to the
/// end of the tag.
fn private_use(c: &mut Cursor<'_>) -> bool {
    let mut any = false;
    while let Some(s) = c.bump() {
        if !alphanum(s, 1, 8) {
            return false;
        }
        any = true;
    }
    any
}

#[cfg(test)]
mod tests {
    use super::is_structurally_valid_language_tag as valid;

    #[test]
    fn accepts_well_formed_tags() {
        for t in [
            "en",
            "EN-us",
            "en-US",
            "en-Latn-US",
            "und",
            "zh-Hans-CN",
            "es-419",
            "ca-ES-valencia",
            "sl-rozaj-biske-1994",
            "de-1996",
            "TR-tr",
            "th-u-nu-thai",
            "en-US-u-nu-arab",
            "en-US-u-ca-gregory-nu-latn",
            "de-DE-u-co-phonebk",
            "en-u-attr-co-phonebk",
            "en-u-attr1-attr2",
            "en-u-kn",
            "ja-t-it",
            "en-t-ja-latn-m0-ungegn-2007",
            "und-t-m0-names",
            "en-a-bbb-x-a-yz",
            "en-a-bbb-b-ccc",
            "en-u-ca-gregory-t-ja",
            "hy-Latn-IT-arevela",
            "abcdefgh-Latn",
            // regular grandfathered tags happen to be well-formed ids
            "art-lojban",
            "cel-gaulish",
            "zh-hakka",
            // private use after an id
            "en-x-a",
            "en-US-x-private1-a",
            "de-x-u-co-phonebk",
        ] {
            assert!(valid(t.as_bytes()), "{t} should be valid");
        }
    }

    #[test]
    fn rejects_malformed_tags() {
        for t in [
            "",
            "-",
            "en-",
            "-en",
            "en--US",
            "en_US",
            "de_DE",
            "a",
            "i",
            "abcd",
            "abcdefghi",
            "root",
            "Latn-US",
            "en-US-US",
            "en-u",
            "en-u-c",
            "en-u-co-",
            "en-u-co-u-nu-latn",
            "de-u-ca-gregory-u-ca-buddhist",
            "en-u-abcdefghi",
            "en-t",
            "en-t-a",
            "en-t-m0",
            "en-t-m0-ab",
            "en-t-en-rozaj-rozaj",
            "en-a",
            "en-a-a",
            "en-a-bb-a-cc",
            "en-rozaj-rozaj",
            "en-ROZAJ-rozaj",
            "de-gregory-gregory",
            "x-priv",
            "en-x",
            "en-x-abcdefghi",
            // irregular grandfathered tags are not unicode_locale_ids
            "i-klingon",
            "i-default",
            "zh-min-nan",
            "en-GB-oed",
            "sgn-BE-FR",
            "en-\u{00FF}",
            "en-US\u{0}",
        ] {
            assert!(!valid(t.as_bytes()), "{t:?} should be invalid");
        }
    }
}
