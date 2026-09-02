//! Range-based UTF-8 byte expansion for regex classes the matcher
//! cannot step one byte at a time — chunk 10d v2.
//!
//! Classes that can match non-ASCII code points
//! (`negate` / explicit non-ASCII byte bits / `\p{}` property
//! fold-ins) cannot be byte-stepped by the DFA directly: a 2-byte
//! UTF-8 leading byte at the cursor must be paired with the matching
//! trailing continuation byte to decide whether the encoded cp
//! belongs to the class. This module rewrites such classes at compile
//! time into a byte-level [`Node::Alt`] of [`Node::Concat`] over
//! per-byte [`Node::Class`] slots; the existing Thompson compiler
//! emits ordinary `Op::Class` instructions referencing
//! `CharClass { byte_only: true, .. }` leaves, the DFA byte-step
//! walks them verbatim, and the Pike VM second-pass capture
//! recovery honours `byte_only` to step a single byte regardless of
//! `RE_FLAG_U`.
//!
//! ## Algorithm — re2 / regex-syntax `Utf8Sequences`
//!
//! The v1 attempt (commit `5faf4e47`, reverted `c33547d2`) enumerated
//! every cp in the class's matched set and inserted UTF-8 byte
//! sequences into a trie keyed by byte; trie lowering then folded
//! leaves with shape-equal sub-AST via `canonical_form`
//! byte-serialisation. That was O(N²) over the cp count — `[^a]u`
//! (~1.1 M cp) and `\p{L}u` (~50 K cp via curated UCD) SIGABRT'd the
//! cargo test binary. See `.claude/rfcs/20260623-pike-vm-dfa-chunk10d`
//! for the full v1 post-mortem.
//!
//! v2 input is a **cp range list**, not individual cp. The expansion
//! visits at most O(R · L) recursion frames where R is the post-split
//! range count (a few × the length-plane split count, ≤ ~10 for
//! `[^a]`) and L ≤ 4 is the UTF-8 length. Each frame emits at
//! most three sub-Concats (head / middle / tail).
//!
//! ```text
//! 1. cp_ranges_of(cc)             — walk cc.test_cp, emit sorted disjoint ranges
//! 2. split_at_length_planes       — break each range at 0x80 / 0x800 / 0x10000
//! 3. emit_range_of_length(lo,hi,len)
//!      lo_bytes = utf8_encode_cp(lo)
//!      hi_bytes = utf8_encode_cp(hi)
//!      emit_byte_recurse(lo_bytes, hi_bytes, prefix=[], out)
//! ```
//!
//! ### `emit_byte_recurse` (head / middle / tail)
//!
//! ```text
//! if lo.is_empty() → emit Concat from `prefix`; return
//! if lo[0] == hi[0]:
//!    prefix.push(lo[0]); recurse(lo[1..], hi[1..]); prefix.pop()
//!    return
//! // lo[0] < hi[0]
//! 1. head:   prefix.push(lo[0]); recurse(lo[1..], [0xBF]^k); prefix.pop()
//! 2. middle: if hi[0] > lo[0]+1 → emit prefix + (lo[0]+1, hi[0]-1) + (0x80,0xBF)^(len-1)
//! 3. tail:   prefix.push(hi[0]); recurse([0x80]^k, hi[1..]); prefix.pop()
//! ```
//!
//! ## Boundary correctness (overlong / surrogate)
//!
//! Three leading bytes have constrained second-byte ranges:
//! `0xE0 → 0xA0..0xBF` (else overlong < U+0800),
//! `0xF0 → 0x90..0xBF` (else overlong < U+10000),
//! `0xF4 → 0x80..0x8F` (else > U+10FFFF).
//!
//! `0xED → 0xA0..0xBF` is deliberately NOT excluded. A JS string is a
//! sequence of UTF-16 code units (§6.1.4), so a lone surrogate is a
//! value, and the haystack transcoder (`str_helpers::str_units`)
//! spells it as the generalized three-byte form — WTF-8. A negated
//! class must accept it the way `/./u` and a literal `/\uD83D/u`
//! already do: `/[^]/u.exec("\uD83D")` is `["\ud83d"]`, and `/\S/u`
//! matches it too.
//!
//! The recursion handles these *implicitly* because the cp range
//! splits put each constrained lead at the head / tail of a sub-range
//! whose `lo_bytes` / `hi_bytes` already carry the right second-byte
//! constraint. The middle sub-emit's full `[0x80..0xBF]` continuation
//! is only ever used for leads where that's the correct range — the
//! length-plane split puts 0xE0 / 0xED / 0xF0 / 0xF4 at the boundary
//! of the head / tail sub-range, not in the middle.
//!
//! ## Output AST shape
//!
//! - `Node::Alt` over length groups present (unwrap to lone child if
//!   only one length group is present).
//! - Each length group is a `Node::Alt` over its emitted Concats
//!   (unwrap to lone child if only one).
//! - Each Concat is a `Node::Concat` of `length` `Node::Class` slots,
//!   each Class carrying a 256-bit byte set with `byte_only = true`.
//!   A 1-byte single Concat unwraps to a bare `Node::Class`.

use alloc::{boxed::Box, vec, vec::Vec};

use crate::charclass::CharClass;
use crate::node::{Node, NodeKind};
use crate::utf8::utf8_encode_cp;

/// Expand `cc` into a byte-level AST under the `u` flag, or `None`
/// to signal the caller should keep the original `Op::Class` emission.
///
/// Returns `None` when:
/// - `uflag == false` (non-u patterns keep ASCII byte-step semantics).
/// - `cc` references property tables (RFC 20260711 chunk B — those
///   classes stay single cp-aware `Op::Class` ops served by the Pike
///   VM; `prog_ops_dfa_safe` gates the DFA off unless the class is
///   pending-serveable).
/// - `cc` is u-safe (no negate, no non-ASCII bits, no property
///   tables). Those classes already work on the DFA byte path
///   verbatim.
///
/// Returns `Some(node)` otherwise.
pub fn expand_unsafe_class(cc: &CharClass, uflag: bool, ci: bool) -> Option<Box<Node>> {
    // chunk 10d invariant: never re-expand a class that is itself a
    // leaf produced by this module. byte_only leaves carry non-ASCII
    // byte values (0xC2..0xF4 leading bytes, 0x80..0xBF continuation
    // bytes) in `bits[16..32]`, so `byte_steppable` would otherwise
    // reject them and we'd recurse forever — the leaf's bits encode
    // the byte-step shape, not a cp set to re-encode.
    if cc.byte_only {
        return None;
    }
    // RFC 20260711 chunk B — property-table-bearing unsafe classes
    // (negated `\P{...}` / property + explicit non-ASCII bits) are
    // NOT expanded: the full UCD tables run to ~800 ranges each, and
    // the byte-level Alt-of-Concat explodes past 20k insts whose DFA
    // subset construction takes minutes. They stay single cp-aware
    // `Op::Class` ops; `prog_ops_dfa_safe` rejects the program from
    // the DFA and the Pike VM (`dispatch_class` cp decode +
    // `test_cp`) serves matches. Suffix-shared byte expansion /
    // negated-pending DFA residency is the L3b follow-up.
    if !cc.u_prop_tables.is_empty() {
        return None;
    }
    if byte_steppable(cc, uflag) {
        return None;
    }
    let ranges = cp_ranges_of(cc, ci);
    if ranges.is_empty() {
        return Some(empty_class_node());
    }
    let mut concats: Vec<Vec<(u8, u8)>> = Vec::new();
    let mut prefix: Vec<u8> = Vec::with_capacity(4);
    for len in 1..=4u8 {
        for (lo, hi) in ranges_in_len(&ranges, len) {
            let mut lo_bytes = [0u8; 4];
            let mut hi_bytes = [0u8; 4];
            let n_lo = utf8_encode_cp(lo as i32, &mut lo_bytes);
            let n_hi = utf8_encode_cp(hi as i32, &mut hi_bytes);
            debug_assert_eq!(n_lo, len as usize);
            debug_assert_eq!(n_hi, len as usize);
            emit_byte_recurse(
                &lo_bytes[..n_lo],
                &hi_bytes[..n_hi],
                &mut prefix,
                &mut concats,
            );
        }
    }
    if concats.is_empty() {
        return Some(empty_class_node());
    }
    Some(concats_to_node(&concats))
}

/// True when the matcher can decide this class as it stands, one
/// byte at a time.
///
/// Negation cannot: it has to exclude whole code points, and a
/// multi-byte character's first byte is not the character. Explicit
/// bits in `0x80..=0xFF` cannot either — those are code point values,
/// and no byte of a UTF-8 haystack carries them literally.
///
/// Code points past U+00FF are the one answer that depends on the
/// flag. With `u` the matcher decodes a code point and asks
/// `test_cp`, so the class stands; without it there is no decoder,
/// and the only way to spell a character is its bytes. That
/// asymmetry is why `/[キク]/` answered null for as long as `/u/` did
/// not.
fn byte_steppable(cc: &CharClass, uflag: bool) -> bool {
    if cc.negate || !cc.u_prop_tables.is_empty() {
        return false;
    }
    if cc.bits[16..32].iter().any(|&b| b != 0) {
        return false;
    }
    uflag || cc.owned_ranges.is_empty()
}

/// Walk `cc.test_cp_fold(cp, ci)` over the full cp space; return
/// sorted disjoint inclusive ranges.
///
/// The fold has to happen HERE, before the class-level negate the
/// walk reads through — ES canonicalizes the input and then asks for
/// membership, so `/[^ab]/i` rejects `A`. Asking `test_cp` instead
/// and leaving the fold to match time inverted that: the complement
/// was built over the raw set, `A` was in it, and the later
/// `test_fold` had nothing left to reject. Folding first also leaves
/// the set closed under the ASCII case pair, so that later
/// `test_fold` is a no-op rather than a second, wrong fold.
/// Surrogate cp (`U+D800..U+DFFF`) are walked like any other: no
/// UCD table lists them and `bits` only reaches the first 256 cp, so
/// only `cc.negate` (`[^…]`, `\S`, `\W`, `\D`) admits them — and it
/// must, because the haystack carries a lone surrogate as its WTF-8
/// three-byte form (see the module doc).
fn cp_ranges_of(cc: &CharClass, ci: bool) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut start: Option<u32> = None;
    let mut cp: u32 = 0;
    while cp <= 0x10_FFFF {
        if cc.test_cp_fold(cp as i32, ci) {
            if start.is_none() {
                start = Some(cp);
            }
        } else if let Some(s) = start.take() {
            out.push((s, cp - 1));
        }
        cp += 1;
    }
    if let Some(s) = start {
        out.push((s, 0x10_FFFF));
    }
    out
}

const LEN_BOUNDS: [(u32, u32); 4] = [
    (0x00_0000, 0x00_007F),
    (0x00_0080, 0x00_07FF),
    (0x00_0800, 0x00_FFFF),
    (0x01_0000, 0x10_FFFF),
];

/// Sub-ranges of `ranges` that encode in `len` UTF-8 bytes, clipped
/// to the length window. The surrogate gap is NOT split out of the
/// three-byte window: `0xED 0xA0..0xBF` is how the haystack spells a
/// lone surrogate (WTF-8), so a range that spans the gap must emit it.
fn ranges_in_len(ranges: &[(u32, u32)], len: u8) -> Vec<(u32, u32)> {
    let (wlo, whi) = LEN_BOUNDS[(len - 1) as usize];
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &(lo, hi) in ranges {
        let lo = lo.max(wlo);
        let hi = hi.min(whi);
        if lo > hi {
            continue;
        }
        out.push((lo, hi));
    }
    out
}

fn emit_byte_recurse(lo: &[u8], hi: &[u8], prefix: &mut Vec<u8>, out: &mut Vec<Vec<(u8, u8)>>) {
    debug_assert_eq!(lo.len(), hi.len());
    let n = lo.len();
    if n == 0 {
        let concat: Vec<(u8, u8)> = prefix.iter().map(|&b| (b, b)).collect();
        out.push(concat);
        return;
    }
    if lo[0] == hi[0] {
        prefix.push(lo[0]);
        emit_byte_recurse(&lo[1..], &hi[1..], prefix, out);
        prefix.pop();
        return;
    }
    // lo[0] < hi[0]
    // 1. head: leading = lo[0]; tail = [lo[1..], 0xBF^(n-1)]
    prefix.push(lo[0]);
    let suffix_max: Vec<u8> = vec![0xBFu8; n - 1];
    emit_byte_recurse(&lo[1..], &suffix_max, prefix, out);
    prefix.pop();

    // 2. middle: leading ∈ (lo[0], hi[0]); tail = [0x80..0xBF]^(n-1)
    if hi[0] > lo[0] + 1 {
        let mut concat: Vec<(u8, u8)> = Vec::with_capacity(n + prefix.len());
        for &b in prefix.iter() {
            concat.push((b, b));
        }
        concat.push((lo[0] + 1, hi[0] - 1));
        for _ in 0..(n - 1) {
            concat.push((0x80, 0xBF));
        }
        out.push(concat);
    }

    // 3. tail: leading = hi[0]; tail = [0x80^(n-1), hi[1..]]
    prefix.push(hi[0]);
    let suffix_min: Vec<u8> = vec![0x80u8; n - 1];
    emit_byte_recurse(&suffix_min, &hi[1..], prefix, out);
    prefix.pop();
}

fn empty_class_node() -> Box<Node> {
    let mut n = Node::new(NodeKind::Class);
    n.cc = CharClass::new();
    n.cc.byte_only = true;
    n
}

fn class_node_for_range(lo: u8, hi: u8) -> Box<Node> {
    let mut cc = CharClass::new();
    cc.byte_only = true;
    cc.add_range(lo, hi);
    let mut n = Node::new(NodeKind::Class);
    n.cc = cc;
    n
}

fn concat_to_node(concat: &[(u8, u8)]) -> Box<Node> {
    debug_assert!(!concat.is_empty());
    if concat.len() == 1 {
        let (lo, hi) = concat[0];
        return class_node_for_range(lo, hi);
    }
    let mut node = Node::new(NodeKind::Concat);
    for &(lo, hi) in concat {
        node.push_kid(class_node_for_range(lo, hi));
    }
    node
}

fn concats_to_node(concats: &[Vec<(u8, u8)>]) -> Box<Node> {
    if concats.len() == 1 {
        return concat_to_node(&concats[0]);
    }
    let mut alt = Node::new(NodeKind::Alt);
    for c in concats {
        alt.push_kid(concat_to_node(c));
    }
    alt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utf8::{utf8_decode_cp, utf8_len_for};
    use alloc::collections::BTreeSet;

    /// Walk the expanded AST and collect the cp set it accepts, by
    /// enumerating every byte sequence the byte ranges admit. Only
    /// safe to call on small AST (test fixtures); we use `accept_cp`
    /// for the property tests instead.
    fn enumerate_cp_set(node: &Node) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();
        let mut prefix: Vec<u8> = Vec::with_capacity(4);
        walk(node, &mut prefix, &mut out);
        out
    }

    fn walk(node: &Node, prefix: &mut Vec<u8>, out: &mut BTreeSet<u32>) {
        match node.kind {
            NodeKind::Class => {
                for b in 0..=255u8 {
                    if node.cc.test(b) {
                        prefix.push(b);
                        commit(prefix, out);
                        prefix.pop();
                    }
                }
            }
            NodeKind::Concat => walk_concat(&node.kids, 0, prefix, out),
            NodeKind::Alt => {
                for kid in &node.kids {
                    walk(kid, prefix, out);
                }
            }
            _ => unreachable!("unexpected node kind"),
        }
    }

    fn walk_concat(kids: &[Box<Node>], i: usize, prefix: &mut Vec<u8>, out: &mut BTreeSet<u32>) {
        if i == kids.len() {
            commit(prefix, out);
            return;
        }
        let kid = &kids[i];
        if kid.kind == NodeKind::Class {
            for b in 0..=255u8 {
                if kid.cc.test(b) {
                    prefix.push(b);
                    walk_concat(kids, i + 1, prefix, out);
                    prefix.pop();
                }
            }
        } else {
            walk(kid, prefix, out);
        }
    }

    fn commit(seq: &[u8], out: &mut BTreeSet<u32>) {
        let n = utf8_len_for(seq[0]);
        if seq.len() != n {
            return;
        }
        let (cp, m) = utf8_decode_cp(seq);
        if m != n || cp < 0 {
            return;
        }
        let cp = cp as u32;
        let mut buf = [0u8; 4];
        let nn = utf8_encode_cp(cp as i32, &mut buf);
        if nn != n || &buf[..nn] != seq {
            return;
        }
        out.insert(cp);
    }

    /// Check whether `cp` is accepted by the expansion AST by walking
    /// it directly — O(L · branch_factor) per cp, suitable for spot
    /// checks across the full cp space.
    fn accept_cp(node: &Node, cp: u32) -> bool {
        let mut buf = [0u8; 4];
        let n = utf8_encode_cp(cp as i32, &mut buf);
        if n == 0 {
            return false;
        }
        accept_bytes(node, &buf[..n])
    }

    fn accept_bytes(node: &Node, bytes: &[u8]) -> bool {
        match node.kind {
            NodeKind::Class => bytes.len() == 1 && node.cc.test(bytes[0]),
            NodeKind::Concat => {
                if bytes.len() != node.kids.len() {
                    return false;
                }
                for (kid, &b) in node.kids.iter().zip(bytes.iter()) {
                    if kid.kind != NodeKind::Class || !kid.cc.test(b) {
                        return false;
                    }
                }
                true
            }
            NodeKind::Alt => node.kids.iter().any(|k| accept_bytes(k, bytes)),
            _ => false,
        }
    }

    fn count_concats(node: &Node) -> usize {
        match node.kind {
            NodeKind::Class | NodeKind::Concat => 1,
            NodeKind::Alt => node.kids.iter().map(|k| count_concats(k)).sum(),
            _ => 0,
        }
    }

    /// A negated class excludes whole code points, so it needs the
    /// byte expansion whether or not the pattern carries `u` —
    /// `"日X".match(/[^X]/)` used to stop after 日's first byte and
    /// take the string layer down with it.
    #[test]
    fn negated_class_expands_without_the_u_flag_too() {
        let mut cc = CharClass::new();
        cc.add(b'a');
        cc.negate = true;
        assert!(expand_unsafe_class(&cc, false, false).is_some());
        assert!(expand_unsafe_class(&cc, true, false).is_some());
    }

    /// Code points past U+00FF are the one answer that depends on
    /// the flag: with `u` the matcher decodes and asks `test_cp`, so
    /// the class stands; without it the only way to spell a
    /// character is its bytes.
    #[test]
    fn cp_range_class_expands_only_without_the_u_flag() {
        let mut cc = CharClass::new();
        cc.owned_ranges.push(crate::ucd::UPropRange {
            lo: 0x30AD,
            hi: 0x30AF,
        });
        assert!(expand_unsafe_class(&cc, true, false).is_none());
        assert!(expand_unsafe_class(&cc, false, false).is_some());
    }

    #[test]
    fn no_op_for_uflag_safe_class() {
        let mut cc = CharClass::new();
        cc.add_digit();
        assert!(expand_unsafe_class(&cc, true, false).is_none());
    }

    #[test]
    fn negate_single_cp_accepts_full_minus_one() {
        // `[^a]u` — accept set = all cp except `a`, lone surrogates
        // included (the haystack spells them as WTF-8).
        let mut cc = CharClass::new();
        cc.add(b'a');
        cc.negate = true;
        let node = expand_unsafe_class(&cc, true, false).expect("expansion");
        // ASCII spot checks
        assert!(!accept_cp(&node, b'a' as u32));
        for cp in [b'b' as u32, b'A' as u32, b'0' as u32, 0u32, 0x7F] {
            assert!(accept_cp(&node, cp), "ASCII cp 0x{cp:X} should accept");
        }
        // Non-ASCII spot checks
        for cp in [0x00A9u32, 0x4E2D, 0x10000, 0x1F600] {
            assert!(accept_cp(&node, cp), "cp 0x{cp:X} should accept");
        }
        // A lone surrogate is a code unit the haystack can carry, so
        // the negated class accepts its WTF-8 spelling.
        for cp in [0xD800u32, 0xD83D, 0xDC38, 0xDFFF] {
            assert!(
                accept_cp(&node, cp),
                "lone surrogate 0x{cp:X} should accept"
            );
        }
        // Concat count stays bounded — `[^a]u` expands to ~70 Concats
        // across the four length planes (1-byte split into two sub-
        // ranges, 2-byte / 3-byte / 4-byte each emit head + middle +
        // tail per leading-byte boundary).
        let count = count_concats(&node);
        assert!(count < 200, "expected < 200 Concats, got {count}");
    }

    #[test]
    fn explicit_non_ascii_byte_in_bits() {
        // `cc.bits[0xE6 / 8] = 1 << (0xE6 % 8)` — cc.test_cp(0xE6)
        // returns true via the bitmap path; cp 0x00E6 (LATIN AE)
        // encodes to bytes [0xC3, 0xA6]. The expansion's 2-byte
        // length group should accept exactly that sequence.
        let mut cc = CharClass::new();
        let byte = 0xE6u8;
        cc.bits[(byte >> 3) as usize] |= 1u8 << (byte & 7);
        // Sanity: cc.test_cp(0xE6) accepts; cc.test_cp(0xE7) doesn't.
        assert!(cc.test_cp(0xE6));
        assert!(!cc.test_cp(0xE7));
        let node = expand_unsafe_class(&cc, true, false).expect("expansion");
        assert!(accept_cp(&node, 0x00E6));
        assert!(!accept_cp(&node, 0x00E7));
        assert!(!accept_cp(&node, b'a' as u32));
    }

    #[test]
    fn table_bearing_classes_decline_expansion() {
        // RFC 20260711 chunk B — property-table-bearing unsafe classes
        // (negated / mixed) are NOT expanded: full UCD tables explode
        // the byte-level Alt-of-Concat. They stay single cp-aware
        // `Op::Class` ops served by the Pike VM (`prog_ops_dfa_safe`
        // gates the DFA off).
        let mut neg = CharClass::new();
        neg.add_property_table(crate::ucd::lookup_gc_value("L").unwrap());
        neg.negate = true;
        assert!(expand_unsafe_class(&neg, true, false).is_none());
        // Sanity: the cp-aware membership the Pike VM consults is the
        // inverted table union.
        for cp in [b'A' as i32, 0x03B1, 0x4E2D] {
            assert!(!neg.test_cp(cp));
        }
        for cp in [b'0' as i32, b' ' as i32, 0x0664, 0x1F600] {
            assert!(neg.test_cp(cp));
        }
        // Mixed shape (table + explicit non-ASCII byte bit) declines
        // expansion the same way.
        let mut mixed = CharClass::new();
        mixed.add_property_table(crate::ucd::lookup_gc_value("L").unwrap());
        mixed.bits[24] = 0x01;
        assert!(expand_unsafe_class(&mixed, true, false).is_none());
        // Small negated bitmap classes (no tables) keep the chunk-10d
        // expansion path.
        let mut small_neg = CharClass::new();
        small_neg.add(b'a');
        small_neg.negate = true;
        assert!(expand_unsafe_class(&small_neg, true, false).is_some());
    }

    /// `[^]` is every code unit — the surrogate gap included, since
    /// a lone surrogate is a value the haystack can carry.
    #[test]
    fn cp_ranges_of_empty_negated_class_is_the_whole_cp_space() {
        let mut cc = CharClass::new();
        cc.negate = true;
        assert_eq!(cp_ranges_of(&cc, false), vec![(0u32, 0x10_FFFF)]);
    }

    #[test]
    fn ranges_in_len_clips_to_window() {
        let ranges = vec![(0u32, 0x10_FFFF)];
        let one_byte = ranges_in_len(&ranges, 1);
        assert_eq!(one_byte, vec![(0u32, 0x7F)]);
        let two_byte = ranges_in_len(&ranges, 2);
        assert_eq!(two_byte, vec![(0x80u32, 0x7FF)]);
        let three_byte = ranges_in_len(&ranges, 3);
        // The surrogate gap stays inside the window (WTF-8 haystack)
        assert_eq!(three_byte, vec![(0x800u32, 0xFFFF)]);
        let four_byte = ranges_in_len(&ranges, 4);
        assert_eq!(four_byte, vec![(0x1_0000u32, 0x10_FFFF)]);
    }

    #[test]
    fn small_unsafe_class_enumerates_exact_cp_set() {
        // `[^A]u` over the ASCII-only domain — small enough to
        // enumerate via the walker and compare against the spec.
        let mut cc = CharClass::new();
        cc.add(b'A');
        cc.negate = true;
        let node = expand_unsafe_class(&cc, true, false).expect("expansion");
        let cp_set = enumerate_cp_set(&node);
        // Spot-verify ASCII portion: every byte 0..0x7F except 'A'.
        for cp in 0u32..=0x7F {
            let want = cp != b'A' as u32;
            assert_eq!(
                cp_set.contains(&cp),
                want,
                "ASCII cp 0x{cp:X} membership mismatch"
            );
        }
        // 2-byte plane: spot-check a few representative cp.
        for cp in [0x80u32, 0xA9, 0x7FF] {
            assert!(cp_set.contains(&cp), "2-byte cp 0x{cp:X} missing");
        }
    }

    /// ES canonicalizes the input and then asks for membership, so a
    /// negated class under `i` rejects both cases of what it lists.
    /// Building the complement over the unfolded set let `A` through.
    #[test]
    fn negated_class_folds_before_it_complements() {
        let mut cc = CharClass::new();
        cc.add(b'a');
        cc.add(b'b');
        cc.negate = true;
        let folded = cp_ranges_of(&cc, true);
        for cp in [b'a', b'b', b'A', b'B'] {
            assert!(
                !folded
                    .iter()
                    .any(|&(lo, hi)| (lo..=hi).contains(&(cp as u32))),
                "0x{cp:02x} should be excluded under i"
            );
        }
        assert!(
            folded
                .iter()
                .any(|&(lo, hi)| (lo..=hi).contains(&(b'c' as u32)))
        );
        // Without `i` only the listed pair is excluded.
        let raw = cp_ranges_of(&cc, false);
        assert!(
            raw.iter()
                .any(|&(lo, hi)| (lo..=hi).contains(&(b'A' as u32)))
        );
    }
}
