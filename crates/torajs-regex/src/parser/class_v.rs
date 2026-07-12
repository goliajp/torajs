//! v-flag (unicodeSets) class parsing — ES §22.2.1
//! `ClassSetExpression`. RFC 20260712 chunk B1.
//!
//! Under the `v` flag, `[...]` composes operands with union
//! (adjacency), intersection (`&&`) and subtraction (`--`); operands
//! are single characters, `c1-c2` ranges (union level only, per
//! grammar), class escapes (`\d` …), `\p{}` / `\P{}` properties and
//! nested `[...]` classes. The whole expression folds eagerly into a
//! [`CpRangeSet`] — complement (`[^…]`, `\D`, `\P{}`) is computed
//! into the set rather than deferred to a `negate` bit — and lands
//! on the [`Class`](NodeKind::Class) node as byte bitmap (`cp <
//! 0x100`) + [`crate::charclass::CharClass::owned_ranges`].
//!
//! Syntax strictness implemented: unescaped `ClassSetSyntaxCharacter`
//! (`( ) [ ] { } / - \ |`) rejected as literals; reserved double
//! punctuators (`&&` `!!` `##` …) rejected at operand position
//! (single `&&` between operands is the intersection operator);
//! operator kinds don't mix at one nesting level. `\q{…}` string
//! literals land in chunk B2 (parse error until then).

use super::{Parser, apply_property_name};
use crate::cpset::CpRangeSet;
use crate::node::{Node, NodeKind};
use crate::ucd::UPropRange;
use crate::utf8::utf8_decode_cp;
use alloc::boxed::Box;

/// One parsed operand — a single character keeps its identity so the
/// union level can extend it into a `c1-c2` range.
enum OperandV {
    Single(u32),
    Set(CpRangeSet),
}

impl OperandV {
    fn into_set(self) -> CpRangeSet {
        match self {
            OperandV::Single(cp) => {
                let mut s = CpRangeSet::new();
                s.insert_cp(cp);
                s
            }
            OperandV::Set(s) => s,
        }
    }
}

/// `ClassSetReservedDoublePunctuator` leads — two of the same byte at
/// operand position is a SyntaxError (`[&&]`, `[a!!b]`, …).
fn is_reserved_double_lead(b: u8) -> bool {
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
fn is_class_set_syntax_char(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'/' | b'-' | b'\\' | b'|'
    )
}

impl<'p> Parser<'p> {
    /// Parse a v-mode class body after `[` has been consumed.
    pub(super) fn parse_class_v(&mut self) -> Option<Box<Node>> {
        let negate = self.match_byte(b'^');
        let mut set = self.parse_class_set_expression()?;
        if !self.match_byte(b']') {
            self.set_err();
            return None;
        }
        if negate {
            set = set.complement();
        }
        let mut n = Node::new(NodeKind::Class);
        fold_set_into_class(&set, &mut n);
        Some(n)
    }

    /// `ClassSetExpression` — the first operand plus lookahead decide
    /// the level's operator kind: `&&` intersection chain, `--`
    /// subtraction chain, or a union run of operands / ranges.
    /// Ranges (`c1-c2`) only exist at union level per the grammar —
    /// `[a-b&&c]` is a SyntaxError (the chain check runs on the raw
    /// operand, before range extension).
    fn parse_class_set_expression(&mut self) -> Option<CpRangeSet> {
        // Empty class `[]` / fully-negated `[^]`.
        if !self.eof() && self.peek() == b']' {
            return Some(CpRangeSet::new());
        }
        let first = self.parse_class_set_operand()?;
        if self.peek_pair(b'&', b'&') {
            let mut acc = first.into_set();
            while self.peek_pair(b'&', b'&') {
                self.get();
                self.get();
                let rhs = self.parse_class_set_operand()?.into_set();
                acc = acc.intersect(&rhs);
            }
            return self.expect_close(acc);
        }
        if self.peek_pair(b'-', b'-') {
            let mut acc = first.into_set();
            while self.peek_pair(b'-', b'-') {
                self.get();
                self.get();
                let rhs = self.parse_class_set_operand()?.into_set();
                acc = acc.difference(&rhs);
            }
            return self.expect_close(acc);
        }
        // Union run — the first operand may extend into a range.
        let mut acc = self.extend_union_element(first)?;
        while !self.eof() && self.peek() != b']' {
            // A `&&` / `--` after union elements would mix operator
            // kinds at one level — grammar requires nesting.
            if self.peek_pair(b'&', b'&') || self.peek_pair(b'-', b'-') {
                self.set_err();
                return None;
            }
            let op = self.parse_class_set_operand()?;
            let next = self.extend_union_element(op)?;
            acc = acc.union(&next);
        }
        Some(acc)
    }

    /// Union-level range extension: a single-character operand
    /// followed by `-c2` becomes a range.
    fn extend_union_element(&mut self, op: OperandV) -> Option<CpRangeSet> {
        if let OperandV::Single(lo) = op {
            // Range only when `-` is not `--` (subtraction) and not
            // the closing `-]` position (that dash is itself invalid
            // in v-mode — a lone `-` must be escaped — but `--]`
            // parses as subtraction with a missing operand → err in
            // the operand parser).
            if !self.eof()
                && self.peek() == b'-'
                && self.peek_at(1) != b'-'
                && self.peek_at(1) != b']'
            {
                self.get(); // consume `-`
                let hi = match self.parse_class_set_operand()? {
                    OperandV::Single(hi) => hi,
                    OperandV::Set(_) => {
                        // `a-\d` — range endpoint must be a character.
                        self.set_err();
                        return None;
                    }
                };
                if lo > hi {
                    self.set_err();
                    return None;
                }
                let mut s = CpRangeSet::new();
                s.insert(lo, hi);
                return Some(s);
            }
        }
        Some(op.into_set())
    }

    /// `ClassSetOperand` — nested class, escape, or literal cp.
    fn parse_class_set_operand(&mut self) -> Option<OperandV> {
        if self.eof() {
            self.set_err();
            return None;
        }
        let b = self.peek();
        if b == b'[' {
            self.get();
            let negate = self.match_byte(b'^');
            let mut set = self.parse_class_set_expression()?;
            if !self.match_byte(b']') {
                self.set_err();
                return None;
            }
            if negate {
                set = set.complement();
            }
            return Some(OperandV::Set(set));
        }
        if b == b'\\' {
            self.get();
            return self.parse_class_set_escape();
        }
        if is_reserved_double_lead(b) && self.peek_at(1) == b {
            self.set_err();
            return None;
        }
        if is_class_set_syntax_char(b) {
            self.set_err();
            return None;
        }
        // Literal — decode one UTF-8 code point from the pattern.
        let (cp, len) = utf8_decode_cp(&self.p[self.i..]);
        if cp < 0 || len == 0 {
            self.set_err();
            return None;
        }
        self.i += len;
        Some(OperandV::Single(cp as u32))
    }

    /// Escape at operand position (`\` consumed).
    fn parse_class_set_escape(&mut self) -> Option<OperandV> {
        if self.eof() {
            self.set_err();
            return None;
        }
        let e = self.get();
        let single = match e {
            b'n' => b'\n' as u32,
            b't' => b'\t' as u32,
            b'r' => b'\r' as u32,
            b'f' => 0x0C,
            b'v' => 0x0B,
            b'0' => 0,
            b'b' => 0x08,
            b'd' | b'D' | b'w' | b'W' | b's' | b'S' => {
                return Some(OperandV::Set(shorthand_set(e)));
            }
            b'p' | b'P' => return self.parse_class_set_property(e == b'P'),
            b'q' => {
                // ClassStringDisjunction — chunk B2.
                self.set_err();
                return None;
            }
            b'x' => {
                let h1 = self.read_hex_digit()?;
                let h2 = self.read_hex_digit()?;
                (u32::from(h1) << 4) | u32::from(h2)
            }
            b'u' => self.parse_class_set_u_escape()?,
            b'c' => {
                // `\cX` control escape.
                if self.eof() || !self.peek().is_ascii_alphabetic() {
                    self.set_err();
                    return None;
                }
                u32::from(self.get() % 32)
            }
            // Escaped punctuator / identity escape — any ASCII
            // punctuator (incl. the syntax + reserved sets) stands
            // for itself.
            other if other.is_ascii_punctuation() => u32::from(other),
            _ => {
                self.set_err();
                return None;
            }
        };
        Some(OperandV::Single(single))
    }

    /// `\u{HHH…}` / `\uHHHH` (v-mode is always Unicode mode, so the
    /// braced form is available; surrogate halves stay literal cp —
    /// matching them is bounded by the haystack transcode layer).
    fn parse_class_set_u_escape(&mut self) -> Option<u32> {
        if !self.eof() && self.peek() == b'{' {
            self.get();
            let mut cp: u32 = 0;
            let mut n = 0;
            while !self.eof() && self.peek() != b'}' {
                let h = self.read_hex_digit()?;
                cp = (cp << 4) | u32::from(h);
                n += 1;
                if n > 6 || cp > 0x10_FFFF {
                    self.set_err();
                    return None;
                }
            }
            if !self.match_byte(b'}') || n == 0 {
                self.set_err();
                return None;
            }
            return Some(cp);
        }
        let mut cp: u32 = 0;
        for _ in 0..4 {
            let h = self.read_hex_digit()?;
            cp = (cp << 4) | u32::from(h);
        }
        Some(cp)
    }

    /// `\p{…}` / `\P{…}` at operand position — resolves through the
    /// shared UCD lookup; `\P` complements over the full cp domain.
    fn parse_class_set_property(&mut self, complement: bool) -> Option<OperandV> {
        if self.eof() || self.peek() != b'{' {
            self.set_err();
            return None;
        }
        self.get();
        let (name, value) = self.read_property_expr()?;
        // Reuse apply_property_name through a scratch node — it
        // resolves alias/keyed forms against the CODEGEN tables.
        let mut scratch = Node::new(NodeKind::Class);
        if !apply_property_name(&mut scratch, &name, value.as_deref()) {
            self.set_err();
            return None;
        }
        let mut set = class_to_set(&scratch.cc);
        if complement {
            set = set.complement();
        }
        Some(OperandV::Set(set))
    }

    fn peek_pair(&self, a: u8, b: u8) -> bool {
        !self.eof() && self.peek() == a && self.peek_at(1) == b
    }

    fn expect_close(&mut self, acc: CpRangeSet) -> Option<CpRangeSet> {
        if self.eof() || self.peek() != b']' {
            self.set_err();
            return None;
        }
        Some(acc)
    }
}

/// `\d` / `\w` / `\s` and their complements as cp sets. Complements
/// span the full cp domain (v-mode sets are true cp sets, unlike the
/// byte-bitmap complements of the legacy class parser).
fn shorthand_set(e: u8) -> CpRangeSet {
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
fn class_to_set(cc: &crate::charclass::CharClass) -> CpRangeSet {
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
fn fold_set_into_class(set: &CpRangeSet, n: &mut Node) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Parser, RE_FLAG_V};

    fn parse_ok(pattern: &str) -> Box<Node> {
        let mut p = Parser::new(pattern.as_bytes(), RE_FLAG_V);
        let r = p.parse().expect("parse failed");
        assert!(!p.err());
        r
    }

    fn parse_err(pattern: &str) {
        let mut p = Parser::new(pattern.as_bytes(), RE_FLAG_V);
        let r = p.parse();
        assert!(
            r.is_none() && p.err(),
            "expected parse error for {pattern:?}"
        );
    }

    fn class_of(root: &Node) -> &crate::charclass::CharClass {
        assert_eq!(root.kids[0].kind, NodeKind::Class);
        &root.kids[0].cc
    }

    #[test]
    fn union_of_nested_class_and_char() {
        let r = parse_ok("[[0-9]_]");
        let cc = class_of(&r);
        assert!(cc.test_cp('0' as i32) && cc.test_cp('9' as i32) && cc.test_cp('_' as i32));
        assert!(!cc.test_cp('a' as i32));
    }

    #[test]
    fn intersection_and_difference() {
        let r = parse_ok("[[0-9]&&[0-7]]");
        let cc = class_of(&r);
        assert!(cc.test_cp('7' as i32) && !cc.test_cp('8' as i32));

        let r = parse_ok("[[0-9]--[4-6]]");
        let cc = class_of(&r);
        assert!(cc.test_cp('3' as i32) && !cc.test_cp('5' as i32) && cc.test_cp('9' as i32));
    }

    #[test]
    fn chained_operators_and_nesting() {
        let r = parse_ok("[[0-9]--[0-3]--[8-9]]");
        let cc = class_of(&r);
        assert!(!cc.test_cp('1' as i32) && cc.test_cp('5' as i32) && !cc.test_cp('9' as i32));

        let r = parse_ok("[[a-z]&&[^aeiou]]");
        let cc = class_of(&r);
        assert!(cc.test_cp('b' as i32) && !cc.test_cp('e' as i32));
    }

    #[test]
    fn negated_v_class_is_eager_complement() {
        let r = parse_ok("[^a]");
        let cc = class_of(&r);
        assert!(!cc.negate, "v-mode complements eagerly");
        assert!(!cc.test_cp('a' as i32));
        assert!(cc.test_cp('b' as i32));
        assert!(cc.test_cp(0x1F600)); // non-ASCII side included
    }

    #[test]
    fn property_and_complement_operands() {
        let r = parse_ok("[\\p{ASCII_Hex_Digit}&&\\p{Nd}]");
        let cc = class_of(&r);
        assert!(cc.test_cp('5' as i32) && !cc.test_cp('a' as i32));

        let r = parse_ok("[\\P{ASCII}]");
        let cc = class_of(&r);
        assert!(!cc.test_cp('a' as i32) && cc.test_cp(0x100));
    }

    #[test]
    fn shorthand_sets_and_ranges() {
        let r = parse_ok("[\\d_]");
        let cc = class_of(&r);
        assert!(cc.test_cp('5' as i32) && cc.test_cp('_' as i32));

        // Raw ranges are union-level only — intersection needs the
        // range nested: `[a-f&&[d-z]]` is a SyntaxError.
        parse_err("[a-f&&[d-z]]");
        let r = parse_ok("[[a-f]&&[d-z]]");
        let cc = class_of(&r);
        assert!(cc.test_cp('d' as i32) && !cc.test_cp('c' as i32));

        let r = parse_ok("[\\u{1F600}-\\u{1F64F}]");
        let cc = class_of(&r);
        assert!(cc.test_cp(0x1F60A) && !cc.test_cp(0x1F650));
        assert!(!cc.owned_ranges.is_empty());
    }

    #[test]
    fn v_syntax_errors() {
        // Unescaped syntax chars, reserved doubles, mixed operators,
        // set-operand range endpoint, bad property, \q (B2).
        for pat in [
            "[(]",
            "[|]",
            "[&&]",
            "[a!!b]",
            "[a&&b--c]",
            "[a-b&&c]",
            "[a-\\d]",
            "[\\p{NotAProp}]",
            "[\\q{a}]",
        ] {
            parse_err(pat);
        }
    }

    #[test]
    fn escaped_punctuators_are_literals() {
        let r = parse_ok("[\\-\\[\\]\\&]");
        let cc = class_of(&r);
        for c in ['-', '[', ']', '&'] {
            assert!(cc.test_cp(c as i32), "expected literal {c}");
        }
    }

    #[test]
    fn literal_non_ascii_cp_decodes() {
        let r = parse_ok("[π]");
        let cc = class_of(&r);
        assert!(cc.test_cp(0x03C0) && !cc.test_cp(0x03C1));
    }
}
