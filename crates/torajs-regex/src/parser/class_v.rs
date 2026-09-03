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
//! operator kinds don't mix at one nesting level.
//!
//! Chunk B2 — `\q{…}` ClassStringDisjunction. A class may contain
//! multi-cp STRINGS alongside code points; sets become
//! [`ClassSetV`] = (cps, strings) with componentwise algebra
//! (length-1 alternatives fold into the cp set at parse time; the
//! empty alternative is a real member matching the empty string).
//! A class whose value may contain strings cannot be complemented
//! (`[^…\q{ab}]` / `[^…]` over a strings-carrying nested class is
//! the spec's MayContainStrings early error). Classes with strings
//! desugar at parse time into an [`Alt`](NodeKind::Alt): string
//! alternatives sorted by descending length (leftmost-first Pike
//! priority == leftmost-longest string preference, matching the
//! DFA), then the cp-set class, then the empty string if present.

use super::class_v_set::{
    ClassSetV, class_to_set, fold_set_into_class, is_class_set_syntax_char,
    is_reserved_double_lead, push_q_alternative, shorthand_set, string_prop_to_set,
};
use super::{Parser, apply_property_name};
use crate::cpset::CpRangeSet;
use crate::node::{Node, NodeKind};
use crate::utf8::{utf8_decode_cp, utf8_encode_cp};
use alloc::boxed::Box;
use alloc::vec::Vec;

/// One parsed operand — a single character keeps its identity so the
/// union level can extend it into a `c1-c2` range.
enum OperandV {
    Single(u32),
    Set(ClassSetV),
}

impl OperandV {
    fn into_set(self) -> ClassSetV {
        match self {
            OperandV::Single(cp) => {
                let mut s = CpRangeSet::new();
                s.insert_cp(cp);
                ClassSetV::from_cps(s)
            }
            OperandV::Set(s) => s,
        }
    }
}

impl<'p> Parser<'p> {
    /// Parse a v-mode class body after `[` has been consumed.
    /// Strings-free classes stay a single `Class` node; classes with
    /// `\q{}` / string members desugar into an `Alt` (see module
    /// doc). Complementing a strings-carrying class is the spec's
    /// MayContainStrings early error.
    pub(super) fn parse_class_v(&mut self) -> Option<Box<Node>> {
        let negate = self.match_byte(b'^');
        let mut set = self.parse_class_set_expression()?;
        if !self.match_byte(b']') {
            self.set_err();
            return None;
        }
        if negate {
            if !set.strings.is_empty() {
                self.set_err();
                return None;
            }
            set.cps = set.cps.complement();
        }
        Some(self.class_set_to_node(set))
    }

    /// Materialise a finished [`ClassSetV`] as an AST node — a plain
    /// `Class` when strings-free, else the string/class `Alt`
    /// desugar. Synthesized leaves carry the current effective i/m/s
    /// scope (they never travel back through `parse_atom_with_repeat`
    /// stamping).
    pub(super) fn class_set_to_node(&self, set: ClassSetV) -> Box<Node> {
        let mut class_node = Node::new(NodeKind::Class);
        fold_set_into_class(&set.cps, &mut class_node);
        class_node.eff_ims = self.eff_ims;
        if set.strings.is_empty() {
            return class_node;
        }
        // Descending length: longer strings win over shorter ones and
        // over single-cp class matches (Pike leftmost-first == the
        // spec's longest-string preference; the DFA is longest-match
        // by construction). Equal lengths are mutually exclusive so
        // their relative order is free.
        let mut strings: Vec<&Vec<u32>> = set.strings.iter().collect();
        strings.sort_by(|a, b| b.len().cmp(&a.len()));
        let mut alt = Node::new(NodeKind::Alt);
        alt.eff_ims = self.eff_ims;
        let mut saw_empty = false;
        for s in strings {
            if s.is_empty() {
                saw_empty = true;
                continue;
            }
            let mut concat = Node::new(NodeKind::Concat);
            concat.eff_ims = self.eff_ims;
            for &cp in s {
                let mut buf = [0u8; 4];
                let blen = utf8_encode_cp(cp as i32, &mut buf);
                for &byte in &buf[..blen] {
                    let mut ch = Node::new(NodeKind::Char);
                    ch.ch = byte;
                    ch.eff_ims = self.eff_ims;
                    concat.push_kid(ch);
                }
            }
            alt.push_kid(concat);
        }
        if !set.cps.is_empty() {
            alt.push_kid(class_node);
        }
        if saw_empty {
            // The empty string matches last (length 0) — an empty
            // Concat is a pure epsilon branch.
            let mut empty = Node::new(NodeKind::Concat);
            empty.eff_ims = self.eff_ims;
            alt.push_kid(empty);
        }
        if alt.kids.len() == 1 {
            return alt.kids.pop().expect("single alt kid");
        }
        alt
    }

    /// `ClassSetExpression` — the first operand plus lookahead decide
    /// the level's operator kind: `&&` intersection chain, `--`
    /// subtraction chain, or a union run of operands / ranges.
    /// Ranges (`c1-c2`) only exist at union level per the grammar —
    /// `[a-b&&c]` is a SyntaxError (the chain check runs on the raw
    /// operand, before range extension).
    fn parse_class_set_expression(&mut self) -> Option<ClassSetV> {
        // Empty class `[]` / fully-negated `[^]`.
        if !self.eof() && self.peek() == b']' {
            return Some(ClassSetV::default());
        }
        let first = self.parse_class_set_operand()?;
        if self.peek_pair(b'&', b'&') {
            let mut acc = first.into_set();
            while self.peek_pair(b'&', b'&') {
                self.get();
                self.get();
                let rhs = self.parse_class_set_operand()?.into_set();
                acc = acc.intersect(rhs);
            }
            return self.expect_close(acc);
        }
        if self.peek_pair(b'-', b'-') {
            let mut acc = first.into_set();
            while self.peek_pair(b'-', b'-') {
                self.get();
                self.get();
                let rhs = self.parse_class_set_operand()?.into_set();
                acc = acc.difference(rhs);
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
            acc = acc.union(next);
        }
        Some(acc)
    }

    /// Union-level range extension: a single-character operand
    /// followed by `-c2` becomes a range.
    fn extend_union_element(&mut self, op: OperandV) -> Option<ClassSetV> {
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
                return Some(ClassSetV::from_cps(s));
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
                // MayContainStrings — a strings-carrying class value
                // cannot be complemented.
                if !set.strings.is_empty() {
                    self.set_err();
                    return None;
                }
                set.cps = set.cps.complement();
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
                return Some(OperandV::Set(ClassSetV::from_cps(shorthand_set(e))));
            }
            b'p' | b'P' => return self.parse_class_set_property(e == b'P'),
            b'q' => return self.parse_class_string_disjunction(),
            b'x' => {
                let h1 = self.read_hex_digit()?;
                let h2 = self.read_hex_digit()?;
                (u32::from(h1) << 4) | u32::from(h2)
            }
            b'u' => self.parse_class_set_u_escape()?,
            b'c' => {
                // `\c` + ControlLetter. v-mode is always Unicode mode,
                // so annexB's widened ClassControlLetter never applies
                // here and a non-letter is an early error.
                let Some(v) = (!self.eof())
                    .then(|| super::control_letter_value(self.peek(), false))
                    .flatten()
                else {
                    self.set_err();
                    return None;
                };
                self.get();
                v
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
        // Properties of strings (chunk B3) — lone form only; `\P` of
        // a strings property is the MayContainStrings early error.
        if value.is_none()
            && let Ok(n) = core::str::from_utf8(&name)
            && let Some(parts) = crate::ucd::lookup_string_property(n)
        {
            if complement {
                self.set_err();
                return None;
            }
            return Some(OperandV::Set(string_prop_to_set(parts)));
        }
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
        Some(OperandV::Set(ClassSetV::from_cps(set)))
    }

    /// `\q{Alt|Alt|…}` — ClassStringDisjunction. Each alternative is
    /// a (possibly empty) run of ClassSetCharacters; length-1
    /// alternatives fold into the cp set, the rest join `strings`.
    fn parse_class_string_disjunction(&mut self) -> Option<OperandV> {
        if !self.match_byte(b'{') {
            self.set_err();
            return None;
        }
        let mut set = ClassSetV::default();
        let mut cur: Vec<u32> = Vec::new();
        loop {
            if self.eof() {
                self.set_err();
                return None;
            }
            match self.peek() {
                b'}' => {
                    self.get();
                    push_q_alternative(&mut set, core::mem::take(&mut cur));
                    return Some(OperandV::Set(set));
                }
                b'|' => {
                    self.get();
                    push_q_alternative(&mut set, core::mem::take(&mut cur));
                }
                _ => {
                    let cp = self.parse_q_string_char()?;
                    cur.push(cp);
                }
            }
        }
    }

    /// One character inside `\q{…}` — a literal cp or a character
    /// escape (no sets: `\d` / `\p{}` / nested classes are not
    /// ClassSetCharacters here).
    fn parse_q_string_char(&mut self) -> Option<u32> {
        let b = self.peek();
        if b == b'\\' {
            self.get();
            match self.parse_class_set_escape()? {
                OperandV::Single(cp) => Some(cp),
                OperandV::Set(_) => {
                    self.set_err();
                    None
                }
            }
        } else if is_class_set_syntax_char(b)
            || (is_reserved_double_lead(b) && self.peek_at(1) == b)
        {
            self.set_err();
            None
        } else {
            let (cp, len) = utf8_decode_cp(&self.p[self.i..]);
            if cp < 0 || len == 0 {
                self.set_err();
                return None;
            }
            self.i += len;
            Some(cp as u32)
        }
    }

    fn peek_pair(&self, a: u8, b: u8) -> bool {
        !self.eof() && self.peek() == a && self.peek_at(1) == b
    }

    fn expect_close(&mut self, acc: ClassSetV) -> Option<ClassSetV> {
        if self.eof() || self.peek() != b']' {
            self.set_err();
            return None;
        }
        Some(acc)
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
        // set-operand range endpoint, bad property, malformed \q,
        // MayContainStrings complement, set inside \q.
        for pat in [
            "[(]",
            "[|]",
            "[&&]",
            "[a!!b]",
            "[a&&b--c]",
            "[a-b&&c]",
            "[a-\\d]",
            "[\\p{NotAProp}]",
            "[\\q{a}",
            "[\\qa]",
            "[^\\q{ab}]",
            "[\\q{a-b}]",
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
    fn q_string_disjunction_folds_and_desugars() {
        // Single-cp alternatives fold into the class; multi-cp become
        // Alt branches sorted by descending length; empty matches ε.
        let r = parse_ok("[\\q{a}]");
        assert_eq!(r.kids[0].kind, NodeKind::Class);
        assert!(r.kids[0].cc.test_cp('a' as i32));

        let r = parse_ok("[\\q{ab|c|xyz}]");
        let alt = &r.kids[0];
        assert_eq!(alt.kind, NodeKind::Alt);
        // xyz (3) before ab (2) before the cp class holding 'c'.
        assert_eq!(alt.kids.len(), 3);
        assert_eq!(alt.kids[0].kids.len(), 3);
        assert_eq!(alt.kids[1].kids.len(), 2);
        assert_eq!(alt.kids[2].kind, NodeKind::Class);
        assert!(alt.kids[2].cc.test_cp('c' as i32));

        // Empty alternative → trailing epsilon Concat branch.
        let r = parse_ok("[\\q{ab|}]");
        let alt = &r.kids[0];
        assert_eq!(alt.kind, NodeKind::Alt);
        assert_eq!(alt.kids.len(), 2);
        assert_eq!(alt.kids[1].kind, NodeKind::Concat);
        assert!(alt.kids[1].kids.is_empty());
    }

    #[test]
    fn q_string_set_algebra() {
        use crate::parser::RE_FLAG_V;
        // Intersection keeps only shared strings; difference removes.
        let parse_set = |pat: &str| {
            let mut p = Parser::new(pat.as_bytes(), RE_FLAG_V);
            let r = p.parse().expect("parse failed");
            assert!(!p.err());
            r
        };
        // [\q{ab|cd}&&\q{cd|ef}] → only "cd" survives.
        let r = parse_set("[\\q{ab|cd}&&\\q{cd|ef}]");
        let n = &r.kids[0];
        assert_eq!(
            n.kind,
            NodeKind::Concat,
            "single string folds to its Concat"
        );
        assert_eq!(n.kids.len(), 2);
        assert_eq!(n.kids[0].ch, b'c');
        // [\q{ab|cd}--\q{cd}] → only "ab".
        let r = parse_set("[\\q{ab|cd}--\\q{cd}]");
        let n = &r.kids[0];
        assert_eq!(n.kind, NodeKind::Concat);
        assert_eq!(n.kids[0].ch, b'a');
        // cps and strings mix: union keeps both sides.
        let r = parse_set("[[0-9]\\q{ab}]");
        let alt = &r.kids[0];
        assert_eq!(alt.kind, NodeKind::Alt);
        assert_eq!(alt.kids.len(), 2);
    }

    #[test]
    fn literal_non_ascii_cp_decodes() {
        let r = parse_ok("[π]");
        let cc = class_of(&r);
        assert!(cc.test_cp(0x03C0) && !cc.test_cp(0x03C1));
    }
}
