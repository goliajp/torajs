//! Unicode property name resolution for `\p{...}` / `\P{...}`
//! escapes (RFC 20260711 chunk B).
//!
//! The range data lives in the CODEGEN tables of
//! [`crate::ucd_tables`] (UCD 16.0.0, produced by
//! `labs/ucd-gen/gen.py`). This module owns the [`UPropRange`]
//! element type, the binary-search membership test, and the ES
//! §22.2.1 name-resolution rules mapping a property expression to
//! the backing table:
//!
//! - `\p{General_Category=V}` / `\p{gc=V}` → [`lookup_gc_value`]
//! - `\p{Script=V}` / `\p{sc=V}` → [`lookup_script`]
//! - `\p{Script_Extensions=V}` / `\p{scx=V}` → [`lookup_scx`]
//! - lone `\p{Name}` → [`lookup_binary`] first, then
//!   [`lookup_gc_value`] (gc values may be written bare)
//!
//! All lookups are exact-match (spec: no loose matching, names are
//! case-sensitive); aliases resolve through the CODEGEN alias
//! tables (`Letter` → `L`, `Adlm` → `Adlam`, `Alpha` →
//! `Alphabetic`, …). A miss returns `None` → the parser reports a
//! SyntaxError, per spec early-error semantics.

use crate::ucd_tables::{
    BINARY_ALIASES, BINARY_TABLES, GC_ALIASES, GC_TABLES, SCRIPT_ALIASES, SCRIPT_TABLES, SCX_TABLES,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UPropRange {
    pub lo: i32,
    pub hi: i32,
}

/// One ES §22.2.1 "property of strings" (RFC 20260712 chunk B3) —
/// single-cp members as sorted ranges + multi-cp sequences as UTF-8
/// strings. Data lives in the CODEGEN [`crate::ucd_emoji_seq`].
pub struct StringProp {
    pub cp_ranges: &'static [UPropRange],
    pub strings: &'static [&'static str],
}

/// Resolve a lone `\p{Name}` against the properties-of-strings
/// whitelist (v-flag only; `\P` of a strings property is the
/// MayContainStrings early error at the call sites). `RGI_Emoji` is
/// the six-part union per UTS #51 — parts are folded by the caller.
pub fn lookup_string_property(name: &str) -> Option<&'static [&'static StringProp]> {
    use crate::ucd_emoji_seq::*;
    static BASIC: [&StringProp; 1] = [&BASIC_EMOJI];
    static KEYCAP: [&StringProp; 1] = [&EMOJI_KEYCAP_SEQUENCE];
    static FLAG: [&StringProp; 1] = [&RGI_EMOJI_FLAG_SEQUENCE];
    static MODIFIER: [&StringProp; 1] = [&RGI_EMOJI_MODIFIER_SEQUENCE];
    static TAG: [&StringProp; 1] = [&RGI_EMOJI_TAG_SEQUENCE];
    static ZWJ: [&StringProp; 1] = [&RGI_EMOJI_ZWJ_SEQUENCE];
    match name {
        "Basic_Emoji" => Some(&BASIC),
        "Emoji_Keycap_Sequence" => Some(&KEYCAP),
        "RGI_Emoji_Flag_Sequence" => Some(&FLAG),
        "RGI_Emoji_Modifier_Sequence" => Some(&MODIFIER),
        "RGI_Emoji_Tag_Sequence" => Some(&TAG),
        "RGI_Emoji_ZWJ_Sequence" => Some(&ZWJ),
        "RGI_Emoji" => Some(&RGI_EMOJI_PARTS),
        _ => None,
    }
}

pub fn uprop_range_contains(t: &[UPropRange], cp: i32) -> bool {
    let mut lo: isize = 0;
    let mut hi: isize = t.len() as isize - 1;
    while lo <= hi {
        let mid = ((lo + hi) >> 1) as usize;
        if cp < t[mid].lo {
            hi = mid as isize - 1;
        } else if cp > t[mid].hi {
            lo = mid as isize + 1;
        } else {
            return true;
        }
    }
    false
}

fn find_table(
    family: &[(&str, &'static [UPropRange])],
    name: &str,
) -> Option<&'static [UPropRange]> {
    family
        .binary_search_by(|&(n, _)| n.cmp(name))
        .ok()
        .map(|i| family[i].1)
}

/// Resolve `name` through the alias table (alias → canonical
/// spelling; a name with no alias row may already be canonical),
/// then binary-search the range-table family.
fn lookup_with_aliases(
    aliases: &'static [(&'static str, &'static str)],
    family: &[(&str, &'static [UPropRange])],
    name: &str,
) -> Option<&'static [UPropRange]> {
    let canonical = match aliases.binary_search_by(|&(a, _)| a.cmp(name)) {
        Ok(i) => aliases[i].1,
        Err(_) => name,
    };
    find_table(family, canonical)
}

/// `General_Category` value (canonical short form `Lu` / composite
/// `L` / long alias `Uppercase_Letter` / legacy `digit`).
pub fn lookup_gc_value(name: &str) -> Option<&'static [UPropRange]> {
    lookup_with_aliases(GC_ALIASES, GC_TABLES, name)
}

/// `Script` value (canonical long form `Adlam` / short alias `Adlm`).
pub fn lookup_script(name: &str) -> Option<&'static [UPropRange]> {
    lookup_with_aliases(SCRIPT_ALIASES, SCRIPT_TABLES, name)
}

/// `Script_Extensions` value — same name domain as `Script`,
/// superset range tables.
pub fn lookup_scx(name: &str) -> Option<&'static [UPropRange]> {
    lookup_with_aliases(SCRIPT_ALIASES, SCX_TABLES, name)
}

/// Lone binary property (`Alphabetic` / alias `Alpha` / `White_Space`
/// / alias `space` / `Any` / `Assigned` / `ASCII` / …).
pub fn lookup_binary(name: &str) -> Option<&'static [UPropRange]> {
    lookup_with_aliases(BINARY_ALIASES, BINARY_TABLES, name)
}

/// ES §12.7.1 `IdentifierStartChar` as a code point — `UnicodeIDStart`
/// plus `$` and `_`. §22.2.1 `RegExpIdentifierStart` is that same set
/// (its other two productions are spellings — a `\u` escape and a
/// surrogate pair — which the caller folds to a code point first), so
/// a capture-group name is judged by the rule that judges a JS
/// identifier, out of the tables this crate already carries.
pub fn is_identifier_start_cp(cp: u32) -> bool {
    if cp < 0x80 {
        return cp == u32::from(b'$') || cp == u32::from(b'_') || (cp as u8).is_ascii_alphabetic();
    }
    lookup_binary("ID_Start").is_some_and(|t| uprop_range_contains(t, cp as i32))
}

/// ES §12.7.1 `IdentifierPartChar` — `UnicodeIDContinue` plus `$`,
/// <ZWNJ> and <ZWJ>. `_` needs no special case: it is in ID_Continue.
pub fn is_identifier_part_cp(cp: u32) -> bool {
    if cp < 0x80 {
        return cp == u32::from(b'$')
            || cp == u32::from(b'_')
            || (cp as u8).is_ascii_alphanumeric();
    }
    if cp == 0x200C || cp == 0x200D {
        return true;
    }
    lookup_binary("ID_Continue").is_some_and(|t| uprop_range_contains(t, cp as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_contains_hits_and_misses() {
        let t = lookup_gc_value("L").unwrap();
        assert!(uprop_range_contains(t, 0x03B1)); // α (Greek)
        assert!(uprop_range_contains(t, 0x0451)); // ё (Cyrillic)
        assert!(uprop_range_contains(t, 0x4E2D)); // 中
        assert!(uprop_range_contains(t, 0xAC00)); // 가
        assert!(uprop_range_contains(t, b'A' as i32)); // full table covers ASCII
        assert!(!uprop_range_contains(t, 0x0030)); // '0'
        assert!(!uprop_range_contains(t, 0xD7A4)); // just past Hangul syllables
    }

    #[test]
    fn gc_value_aliases_resolve() {
        assert_eq!(lookup_gc_value("Letter"), lookup_gc_value("L"));
        assert_eq!(lookup_gc_value("Uppercase_Letter"), lookup_gc_value("Lu"));
        assert_eq!(lookup_gc_value("digit"), lookup_gc_value("Nd"));
        assert!(lookup_gc_value("L").is_some());
        assert!(lookup_gc_value("Foo").is_none());
        // case-sensitive: no loose matching per spec
        assert!(lookup_gc_value("letter").is_none());
        assert!(lookup_gc_value("lu").is_none());
    }

    #[test]
    fn script_and_scx_aliases_resolve() {
        assert_eq!(lookup_script("Adlm"), lookup_script("Adlam"));
        assert_eq!(lookup_script("Latn"), lookup_script("Latin"));
        assert!(lookup_script("Adlam").is_some());
        assert!(lookup_script("NotAScript").is_none());
        // scx superset: U+0640 ARABIC TATWEEL lists Adlam in
        // ScriptExtensions.txt but its Script is Arabic.
        assert!(uprop_range_contains(lookup_scx("Adlam").unwrap(), 0x0640));
        assert!(!uprop_range_contains(
            lookup_script("Adlam").unwrap(),
            0x0640
        ));
    }

    #[test]
    fn binary_names_and_aliases_resolve() {
        assert_eq!(lookup_binary("Alpha"), lookup_binary("Alphabetic"));
        assert_eq!(lookup_binary("space"), lookup_binary("White_Space"));
        assert_eq!(lookup_binary("AHex"), lookup_binary("ASCII_Hex_Digit"));
        assert!(lookup_binary("Alphabetic").is_some());
        assert!(lookup_binary("Any").is_some());
        assert!(lookup_binary("Assigned").is_some());
        assert!(lookup_binary("ASCII").is_some());
        // gc values are NOT binary names (parser falls back separately)
        assert!(lookup_binary("Lu").is_none());
        assert!(lookup_binary("alpha").is_none());
    }

    /// CODEGEN table invariants (RFC 20260711 chunk A) — every
    /// generated table is sorted + disjoint (the binary-search
    /// contract) and name-sorted (the name binary search); spot hits
    /// pin the generator's range coalescing.
    #[test]
    fn codegen_tables_sorted_and_disjoint() {
        use crate::ucd_tables::BINARY_TABLES;
        for family in [GC_TABLES, SCRIPT_TABLES, SCX_TABLES, BINARY_TABLES] {
            for pair in family.windows(2) {
                assert!(pair[0].0 < pair[1].0, "table names must be sorted");
            }
            for (name, table) in family {
                for w in table.windows(2) {
                    assert!(
                        w[0].hi < w[1].lo,
                        "{name}: ranges must be sorted + disjoint"
                    );
                }
            }
        }
    }

    #[test]
    fn alias_tables_sorted() {
        for aliases in [GC_ALIASES, SCRIPT_ALIASES, BINARY_ALIASES] {
            for pair in aliases.windows(2) {
                assert!(pair[0].0 < pair[1].0, "alias names must be sorted");
            }
        }
    }

    #[test]
    fn codegen_spot_hits() {
        // Adlam script: 1E900..1E94B coalesced from three UCD rows.
        assert!(uprop_range_contains(
            lookup_script("Adlam").unwrap(),
            0x1E94B
        ));
        assert!(!uprop_range_contains(
            lookup_script("Adlam").unwrap(),
            0x1E94C
        ));
        // gc composite L covers CJK; not digits.
        assert!(uprop_range_contains(lookup_gc_value("L").unwrap(), 0x4E2D));
        assert!(!uprop_range_contains(lookup_gc_value("L").unwrap(), 0x0030));
        // alias rows are (alias, canonical).
        assert!(
            GC_ALIASES
                .iter()
                .any(|&(a, c)| a == "Uppercase_Letter" && c == "Lu")
        );
    }
}
