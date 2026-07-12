//! v-flag class set value + fold helpers — sibling of
//! [`super::class_v`] (split at the 500-line HARD limit).
//!
//! [`ClassSetV`] is the (code points, strings) pair a v-mode
//! `ClassSetExpression` evaluates to, with componentwise union /
//! intersection / difference (chunk B2); the free functions fold
//! property lookups and finished sets between [`CpRangeSet`] and
//! [`crate::charclass::CharClass`] shapes.

use crate::cpset::CpRangeSet;
use crate::node::Node;
use crate::ucd::UPropRange;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

/// A v-mode class value — code points plus multi-cp strings (chunk
/// B2). `strings` holds only lengths 0 and ≥ 2: single-cp `\q{}`
/// alternatives fold into `cps` at parse time.
#[derive(Default)]
pub(super) struct ClassSetV {
    pub cps: CpRangeSet,
    pub strings: BTreeSet<Vec<u32>>,
}

impl ClassSetV {
    pub(super) fn from_cps(cps: CpRangeSet) -> Self {
        Self {
            cps,
            strings: BTreeSet::new(),
        }
    }

    pub(super) fn union(mut self, other: Self) -> Self {
        self.cps = self.cps.union(&other.cps);
        self.strings.extend(other.strings);
        self
    }

    pub(super) fn intersect(self, other: Self) -> Self {
        Self {
            cps: self.cps.intersect(&other.cps),
            strings: self
                .strings
                .into_iter()
                .filter(|s| other.strings.contains(s))
                .collect(),
        }
    }

    pub(super) fn difference(mut self, other: Self) -> Self {
        self.cps = self.cps.difference(&other.cps);
        self.strings.retain(|s| !other.strings.contains(s));
        self
    }
}

/// `ClassSetReservedDoublePunctuator` leads — two of the same byte at
/// operand position is a SyntaxError (`[&&]`, `[a!!b]`, …).
pub(super) fn is_reserved_double_lead(b: u8) -> bool {
    matches!(
        b,
        b'&' | b'!'
            | b'#'
            | b'$'
            | b'%'
            | b'*'
            | b'+'
            | b','
            | b'.'
            | b':'
            | b';'
            | b'<'
            | b'='
            | b'>'
            | b'?'
            | b'@'
            | b'^'
            | b'`'
            | b'~'
    )
}

/// `ClassSetSyntaxCharacter` — must be escaped to appear literally.
pub(super) fn is_class_set_syntax_char(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'/' | b'-' | b'\\' | b'|'
    )
}

/// Record one `\q{}` alternative: empty and multi-cp go to
/// `strings`, single-cp folds into the cp set.
pub(super) fn push_q_alternative(set: &mut ClassSetV, alt: Vec<u32>) {
    if alt.len() == 1 {
        set.cps.insert_cp(alt[0]);
    } else {
        set.strings.insert(alt);
    }
}

/// `\d` / `\w` / `\s` and their complements as cp sets. Complements
/// span the full cp domain (v-mode sets are true cp sets, unlike the
/// byte-bitmap complements of the legacy class parser).
pub(super) fn shorthand_set(e: u8) -> CpRangeSet {
    let mut s = CpRangeSet::new();
    match e.to_ascii_lowercase() {
        b'd' => s.insert(u32::from(b'0'), u32::from(b'9')),
        b'w' => {
            s.insert(u32::from(b'0'), u32::from(b'9'));
            s.insert(u32::from(b'A'), u32::from(b'Z'));
            s.insert(u32::from(b'a'), u32::from(b'z'));
            s.insert_cp(u32::from(b'_'));
        }
        b's' => {
            // ECMA WhiteSpace ∪ LineTerminator (mirrors the legacy
            // `add_space` ASCII subset plus the Unicode members the
            // cp domain can now express).
            for cp in [
                0x09u32, 0x0A, 0x0B, 0x0C, 0x0D, 0x20, 0xA0, 0x1680, 0x2028, 0x2029, 0x202F,
                0x205F, 0x3000, 0xFEFF,
            ] {
                s.insert_cp(cp);
            }
            s.insert(0x2000, 0x200A);
        }
        _ => {}
    }
    if e.is_ascii_uppercase() {
        s.complement()
    } else {
        s
    }
}

/// Materialise a property-lookup scratch class (ASCII bits + table
/// refs) into a cp set.
pub(super) fn class_to_set(cc: &crate::charclass::CharClass) -> CpRangeSet {
    let mut s = CpRangeSet::new();
    for cp in 0..128u32 {
        if cc.test_cp(cp as i32) {
            s.insert_cp(cp);
        }
    }
    for t in &cc.u_prop_tables {
        for r in t.iter() {
            if r.hi >= 0x80 {
                s.insert(r.lo.max(0x80) as u32, r.hi as u32);
            }
        }
    }
    s
}

/// Fold a finished cp set onto a Class node: `cp < 0x100` into the
/// byte bitmap, the rest into `owned_ranges`. `negate` stays false —
/// complements were computed eagerly into the set.
pub(super) fn fold_set_into_class(set: &CpRangeSet, n: &mut Node) {
    for &(lo, hi) in set.ranges() {
        if lo < 0x100 {
            let bhi = hi.min(0xFF);
            for b in lo..=bhi {
                n.cc.add(b as u8);
            }
        }
        if hi >= 0x100 {
            n.cc.owned_ranges.push(UPropRange {
                lo: lo.max(0x100) as i32,
                hi: hi as i32,
            });
        }
    }
}
