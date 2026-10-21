//! Host-side regex literal bake.
//!
//! [`bake_literal`] compiles each regex literal at `tr build` time with
//! the same pipeline the runtime runs on a pattern it meets at run time
//! (`torajs_regex::compiled::compile_pattern`) and encodes the result
//! as an image (`torajs_regex::image`) the link places next to the
//! string literals. A capture-free, DFA-eligible literal also gets its
//! DFA table serialised, which the link lays out as a `BakedDfaMeta` +
//! `[DfaState; N]` pair in `__DATA_CONST`. The `Expr::Regex` lowering
//! then constructs the RegExp with `__torajs_regex_compile_baked`,
//! which decodes the image instead of parsing, binds the table, and
//! so skips the run-time DFA build as well.

pub(crate) mod matcher;

use crate::ssa::{BakedDfa, BakedRegexEntry};

/// `/pattern/flags` parsed the way the runtime's compile entries parse
/// it, backreferences resolved.
pub(crate) struct Literal {
    pub root: Box<torajs_regex::node::Node>,
    pub flag_bits: u8,
    pub n_captures: usize,
}

/// `None` wherever the runtime would reject the literal.
pub(crate) fn parse_literal(pattern: &str, flags: &str) -> Option<Literal> {
    use torajs_regex::parser::{Parser, RE_FLAG_U, RE_FLAG_V};

    // duplicate or unknown letters, and `u` with `v`, are SyntaxErrors
    let flag_bits = torajs_regex::flags::parse_flags(flags.as_bytes())?;
    if flag_bits & RE_FLAG_U != 0 && flag_bits & RE_FLAG_V != 0 {
        return None;
    }
    // The literal is UTF-8 (code points); without `u` / `v` the
    // pattern is a code-unit sequence, so a supplementary character
    // becomes its two surrogate forms — the same form the runtime
    // haystack transcodes to (§22.2.2.1; `str_helpers::str_units`).
    let pattern_bytes = if torajs_regex::flags::unicode_mode(flag_bits) {
        pattern.as_bytes().to_vec()
    } else {
        torajs_regex::utf8::split_surrogate_pairs(pattern.as_bytes())
    };
    let mut parser = Parser::new(&pattern_bytes, flag_bits);
    let mut root = parser.parse()?;
    if parser.err() {
        return None;
    }
    let names = parser.names.clone();
    let n_captures = parser.n_captures;
    if !torajs_regex::resolve::resolve_backrefs(&mut root, &names, n_captures, flag_bits) {
        return None;
    }
    Some(Literal {
        root,
        flag_bits,
        n_captures,
    })
}

/// How many leading capture groups of the literal every match sets
/// (`0` for a literal the runtime would reject). §22.2.2 leaves a group
/// undefined when the alternative holding it is not taken, when its
/// quantifier ran no iteration, and always inside a negative lookaround.
pub(crate) fn always_set_groups(pattern: &str, flags: &str) -> usize {
    use torajs_regex::node::{Node, NodeKind};
    fn mark(n: &Node, unset: bool, out: &mut [bool]) {
        let unset = unset
            || match n.kind {
                NodeKind::Alt => n.kids.len() > 1,
                NodeKind::Repeat => n.min == 0,
                NodeKind::NegLookahead | NodeKind::NegLookbehind => true,
                _ => false,
            };
        if n.kind == NodeKind::Group
            && let Some(slot) = usize::try_from(n.capture_idx)
                .ok()
                .and_then(|i| out.get_mut(i))
        {
            *slot |= unset;
        }
        for k in &n.kids {
            mark(k, unset, out);
        }
        if let Some(c) = &n.child {
            mark(c, unset, out);
        }
    }
    let Some(lit) = parse_literal(pattern, flags) else {
        return 0;
    };
    let mut maybe_unset = vec![false; lit.n_captures + 1];
    mark(&lit.root, false, &mut maybe_unset);
    (1..=lit.n_captures)
        .take_while(|&i| !maybe_unset[i])
        .count()
}

/// Bake the literal `/pattern/flags`: compile it the way the runtime
/// compiles a pattern, push its image (and its DFA table when it has
/// one) into `baked_regex_buf`, and return the entry's index. A
/// literal the runtime would reject bakes as the same never-match
/// stub the runtime builds.
pub(crate) fn bake_literal(
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    pattern: &str,
    flags: &str,
) -> u32 {
    // The literal is UTF-8 (code points); the runtime reads a pattern
    // in its code-unit form, a supplementary character as its two
    // surrogate forms (§22.2.2.1; `str_helpers::str_units`).
    let units = torajs_regex::utf8::split_surrogate_pairs(pattern.as_bytes());
    let compiled = torajs_regex::compiled::compile_pattern(&units, flags.as_bytes());
    let index = baked_regex_buf.len() as u32;
    baked_regex_buf.push(BakedRegexEntry {
        index,
        image: torajs_regex::image::encode(&compiled),
        dfa: bake_dfa(&compiled),
    });
    index
}

/// The DFA table of a capture-free literal whose RegExp would build one
/// eagerly at construction. Capture-using literals keep the runtime
/// DFA-then-Pike pair (a table carries no save information); past the
/// cap the table is filled lazily at run time.
fn bake_dfa(c: &torajs_regex::compiled::Compiled) -> Option<BakedDfa> {
    use torajs_regex::dfa::{CappedDfa, DfaState, build_dfa_capped, prog_ops_dfa_safe};

    if c.rejected != 0 || c.n_captures > 0 || !c.prog.can_dfa || !prog_ops_dfa_safe(&c.prog) {
        return None;
    }
    let CappedDfa::Table(dfa) = build_dfa_capped(&c.prog, c.flags) else {
        return None;
    };
    // a poisoned build (unserveable K-PROPERTY shape, e.g.
    // `\p{L}|\p{N}`) is discarded and the Pike VM serves the literal
    if dfa.poisoned {
        return None;
    }
    let states_slice: &[DfaState] = &dfa.states;
    let state_size = core::mem::size_of::<DfaState>();
    let mut payload: Vec<u8> = Vec::with_capacity(std::mem::size_of_val(states_slice));
    for state in states_slice {
        // SAFETY: `DfaState` is `#[repr(C)]` POD (no pointers, no
        // drop) and has no implicit padding — every byte between
        // `transitions` and `pending_class` belongs to a declared
        // field, `_pad` included. That is what makes reading the
        // struct's byte representation through `*const u8` sound:
        // every byte read is initialised, and the sequence matches
        // the layout the link emitter writes into __DATA_CONST and
        // the runtime reader walks.
        //
        // The padding must stay declared. `Default` initialises
        // fields, not holes, so an implicit padding byte read here is
        // uninitialised memory — it took whatever the allocator last
        // left there and carried it into the binary, which is how the
        // same source built twice produced two different artifacts.
        let bytes = unsafe {
            core::slice::from_raw_parts(state as *const DfaState as *const u8, state_size)
        };
        payload.extend_from_slice(bytes);
    }
    Some(BakedDfa {
        states_payload: payload,
        states_len: states_slice.len() as u32,
        start: dfa.start,
        start_mid: dfa.start_mid,
        start_mid_word: dfa.start_mid_word,
        start_mid_nonword: dfa.start_mid_nonword,
        any_accept_before_byte: dfa.any_accept_before_byte,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dfa_of(pattern: &str, flags: &str) -> Option<BakedDfa> {
        let mut buf = Vec::new();
        assert_eq!(bake_literal(&mut buf, pattern, flags), 0);
        buf.pop().unwrap().dfa
    }

    #[test]
    fn capture_free_eligible_literal_bakes_a_table() {
        let d = dfa_of(r"hello\d+", "").expect("table");
        assert!(d.states_len > 0);
        // `DfaState` is 1072 bytes on aarch64-apple-darwin
        assert_eq!(d.states_payload.len(), d.states_len as usize * 1072);
    }

    #[test]
    fn every_literal_gets_an_image_and_its_own_index() {
        let mut buf = Vec::new();
        let idx: Vec<u32> = [("foo", ""), (r"(hello)\d+", ""), ("[", "")]
            .iter()
            .map(|(p, f)| bake_literal(&mut buf, p, f))
            .collect();
        assert_eq!(idx, [0, 1, 2]);
        for (i, e) in buf.iter().enumerate() {
            assert_eq!(e.index, i as u32);
            assert!(!e.image.is_empty());
        }
        assert!(buf[0].dfa.is_some());
    }

    #[test]
    fn captures_backrefs_lookaround_and_rejects_bake_no_table() {
        for p in [r"(hello)\d+", r"(\w+)\1", r"hello(?=world)", r"["] {
            assert!(dfa_of(p, "").is_none(), "{p}");
        }
    }

    #[test]
    fn eligible_shapes_bake_a_table() {
        for (p, f) in [(r"^\w+", ""), (r"hello", "i"), (r"^hello", "m")] {
            assert!(dfa_of(p, f).is_some_and(|d| d.states_len > 0), "/{p}/{f}");
        }
    }

    #[test]
    fn uflag_pclass_eligible_small_payload() {
        // K-PROPERTY skips the Alt-of-Concat byte expansion: one
        // pending K-PROPERTY state plus the `+` loop body
        let d = dfa_of(r"\p{L}+", "u").expect("table");
        assert!(
            d.states_len > 0 && d.states_len <= 10,
            "uflag \\p{{L}}+ should collapse to ≤ 10 DFA states, got {}",
            d.states_len
        );
    }

    /// DFA state counts of the regex-dfa-* bench literals.
    #[test]
    fn ground_truth_states_len_per_fixture() {
        let fixtures = [
            (r"^\w+", "", "anchored"),
            (r"hello", "i", "iflag"),
            (r"\w+\b", "", "wbound"),
            (r"hello$", "", "eflag"),
            (r".+", "s", "dotall"),
            (r"[abc]+", "", "safeclass"),
            (r"\p{L}+", "u", "uflag"),
        ];
        let counts: Vec<(&str, u32)> = fixtures
            .iter()
            .map(|(p, f, name)| {
                let d = dfa_of(p, f).unwrap_or_else(|| panic!("{name} ({p}, {f}) eligible"));
                (*name, d.states_len)
            })
            .collect();
        // a multiline `$` (global m or `(?m:…)`) is VM-only at run
        // time, and so are mixed `^` m-bits: neither bakes a table
        assert!(dfa_of(r"hello$", "m").is_none());
        assert!(dfa_of(r"(?m:^a)|^b", "").is_none());
        let uflag_count = counts.last().unwrap().1;
        let anchored_count = counts[0].1;
        assert!(
            uflag_count <= anchored_count * 3,
            "uflag K-PROPERTY state count stays within 3x of anchored: \
             uflag={uflag_count}, anchored={anchored_count}",
        );
    }
}
