//! Position-aware ε-closure for the DFA substrate (chunk 8).
//!
//! Extends [`crate::dfa::epsilon_closure`] with byte-position context
//! so `Op::AnchorB` and `Op::AnchorE` (and, in later chunks,
//! `Op::WBound` / `Op::NWBound` once the byte-step also threads the
//! right-byte) can resolve correctly instead of being treated as
//! terminal. Chunk 8 leaves the subset-construction builder itself on
//! the legacy ε-closure — later chunks thread the ctx through the
//! builder and executor.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use crate::program::{Op, Program};

/// Position-aware context for [`epsilon_closure_with_ctx`].
///
/// Encodes the byte-position information a DFA state would otherwise
/// need to track on its own. `left_byte` is the byte immediately
/// preceding the current cursor (`None` at the start of the haystack);
/// `is_text_start` is true at byte index 0; `is_text_end` is true at
/// `s.len()`. Chunk 8 supplies this so `Op::AnchorB`, `Op::AnchorE`,
/// `Op::WBound`, and `Op::NWBound` resolve correctly when they would
/// otherwise be terminal under plain [`crate::dfa::epsilon_closure`].
///
/// Multiline `^` (chunk 8.8, per-instruction since the
/// regexp-modifiers chunk) is resolved per `Op::AnchorB` from the
/// instruction's baked `pad` m-bit — see [`anchor_b_advances`]; the
/// ctx only supplies the position facts (`left_byte`).
///
/// `right_byte` (the byte the cursor is about to consume) is *not*
/// included — `WBound` / `NWBound` compare the *left* byte against
/// the upcoming step's *first* byte, which the byte-step itself
/// supplies. The ctx is therefore stable across an entire ε-closure
/// computation at a single cursor position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionCtx {
    pub left_byte: Option<u8>,
    pub is_text_start: bool,
    pub is_text_end: bool,
}

impl PositionCtx {
    /// Convenience: ctx at the very start of a haystack (no preceding
    /// byte, position 0). Used by `dfa_search` callers that anchor at
    /// byte 0; chunks 8.5+ will supply the per-position ctx when the
    /// builder threads it.
    pub fn at_start(is_empty: bool) -> Self {
        Self {
            left_byte: None,
            is_text_start: true,
            is_text_end: is_empty,
        }
    }

    /// Ctx at an arbitrary cursor: `left_byte` is the byte at
    /// `pos - 1` (or `None` at `pos == 0`), `is_text_end` is
    /// `pos == s.len()`.
    pub fn at(pos: usize, s: &[u8]) -> Self {
        Self {
            left_byte: pos.checked_sub(1).and_then(|i| s.get(i).copied()),
            is_text_start: pos == 0,
            is_text_end: pos == s.len(),
        }
    }

    /// `\b` semantics:`true` iff exactly one side of the cursor is a
    /// word character (or one side is OOR + the other is a word char).
    /// Word := `[A-Za-z0-9_]`.
    pub fn at_word_boundary(self, right_byte: Option<u8>) -> bool {
        let lw = self.left_byte.is_some_and(is_word_byte);
        let rw = right_byte.is_some_and(is_word_byte);
        lw != rw
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Position-aware variant of [`crate::dfa::epsilon_closure`]. In
/// addition to `Op::Jmp` / `Op::Split`, this version resolves the
/// four zero-width position ops against `ctx`:
/// - `Op::AnchorB` advances when `ctx.is_text_start` (or under `m`
///   flag handling, the previous byte is a newline — left to a future
///   `m`-flag chunk; for now ASCII no-`m` semantics).
/// - `Op::AnchorE` advances when `ctx.is_text_end` (same `m`-flag
///   caveat).
/// - `Op::WBound` / `Op::NWBound` are still terminal here because
///   they need *both* the left byte and the upcoming right byte;
///   right-byte awareness lives in the byte-step (chunk 8.5).
///
/// `Op::Save` is transparent here (chunk 9): the DFA byte-step is
/// capture-blind so the closure walks through `Op::Save` as a no-op
/// ε. Real capture-slot writes come from a second-pass Pike VM in
/// [`crate::vm::search_from_with_ws`] (RE2-style two-pass), gated by
/// the DFA's `[start..end]` boundary.
///
/// Chunk 8 keeps the surface minimal — `build_dfa` itself still uses
/// the legacy `epsilon_closure`; subsequent chunks thread the
/// position context through the subset construction.
pub fn epsilon_closure_with_ctx(
    prog: &Program,
    seeds: &[usize],
    ctx: PositionCtx,
) -> BTreeSet<usize> {
    let mut closure: BTreeSet<usize> = BTreeSet::new();
    let mut work: Vec<usize> = Vec::new();
    for &seed in seeds {
        if seed < prog.len() && closure.insert(seed) {
            work.push(seed);
        }
    }
    while let Some(pc) = work.pop() {
        let ins = prog.insts[pc];
        let op = match Op::from_u8(ins.op) {
            Some(o) => o,
            None => continue,
        };
        match op {
            Op::Jmp => {
                let t = ins.a as usize;
                if t < prog.len() && closure.insert(t) {
                    work.push(t);
                }
            }
            Op::Split => {
                let t1 = ins.a as usize;
                let t2 = ins.b as usize;
                if t1 < prog.len() && closure.insert(t1) {
                    work.push(t1);
                }
                if t2 < prog.len() && closure.insert(t2) {
                    work.push(t2);
                }
            }
            Op::AnchorB => {
                if anchor_b_advances(ctx, ins.pad as u8) {
                    let t = pc + 1;
                    if t < prog.len() && closure.insert(t) {
                        work.push(t);
                    }
                }
            }
            Op::AnchorE => {
                if ctx.is_text_end {
                    let t = pc + 1;
                    if t < prog.len() && closure.insert(t) {
                        work.push(t);
                    }
                }
            }
            Op::Save => {
                let t = pc + 1;
                if t < prog.len() && closure.insert(t) {
                    work.push(t);
                }
            }
            _ => {}
        }
    }
    closure
}

/// `Op::AnchorB` advances when at text-start, or — when the
/// instruction's baked `pad` m-bit is set (multiline, merged with
/// `(?m:…)` modifier groups at parse time) — immediately after a
/// newline byte. Shared by [`epsilon_closure_with_ctx`] and
/// [`epsilon_closure_full`].
fn anchor_b_advances(ctx: PositionCtx, ins_pad: u8) -> bool {
    ctx.is_text_start || (ins_pad & crate::parser::RE_FLAG_M != 0 && ctx.left_byte == Some(b'\n'))
}

/// Right-byte-aware variant of [`epsilon_closure_with_ctx`] (chunk 8.3).
///
/// When the byte-step has the upcoming byte in hand, the four
/// zero-width position ops (`Op::AnchorB`, `Op::AnchorE`,
/// `Op::WBound`, `Op::NWBound`) can all be resolved instead of just
/// the two anchors. `right_byte`:
/// - `Some(b)` → the byte the cursor is about to consume (used to
///   compute the word-boundary against `ctx.left_byte`);
/// - `None` → at end-of-haystack (no upcoming byte).
///
/// Behaviour reuses [`epsilon_closure_with_ctx`] for the JMP / SPLIT /
/// AnchorB / AnchorE branches and adds WBound / NWBound resolution.
/// `Op::Save` is transparent (chunk 9): closure walks `pc + 1` as a
/// no-op ε; capture writes come from a second-pass Pike VM in
/// [`crate::vm::search_from_with_ws`].
///
/// Chunk 8.3 keeps the builder/executor unchanged — the helper is the
/// new substrate hooks 8.5+ thread through `build_dfa`.
pub fn epsilon_closure_full(
    prog: &Program,
    seeds: &[usize],
    ctx: PositionCtx,
    right_byte: Option<u8>,
) -> BTreeSet<usize> {
    let mut closure: BTreeSet<usize> = BTreeSet::new();
    let mut work: Vec<usize> = Vec::new();
    epsilon_closure_full_into(prog, seeds, ctx, right_byte, &mut closure, &mut work);
    closure
}

/// chunk 7.7 v2 step 12 Round 2 polish — `epsilon_closure_full` 的
/// fill-into 变体。Caller 提供 `closure` + `work` 两个 scratch buffer
/// 复用,避免 256-byte BFS 内循环每次 allocate。`closure` 在 enter 时
/// 被 `clear()`(保留 capacity),`work` 同理。语义跟
/// [`epsilon_closure_full`] 完全等价 — 后者是 thin wrapper。
pub fn epsilon_closure_full_into(
    prog: &Program,
    seeds: &[usize],
    ctx: PositionCtx,
    right_byte: Option<u8>,
    closure: &mut BTreeSet<usize>,
    work: &mut Vec<usize>,
) {
    closure.clear();
    work.clear();
    for &seed in seeds {
        if seed < prog.len() && closure.insert(seed) {
            work.push(seed);
        }
    }
    let at_wb = ctx.at_word_boundary(right_byte);
    while let Some(pc) = work.pop() {
        let ins = prog.insts[pc];
        let op = match Op::from_u8(ins.op) {
            Some(o) => o,
            None => continue,
        };
        let next = match op {
            Op::Jmp => Some(ins.a as usize),
            Op::Split => {
                let t1 = ins.a as usize;
                let t2 = ins.b as usize;
                if t1 < prog.len() && closure.insert(t1) {
                    work.push(t1);
                }
                if t2 < prog.len() && closure.insert(t2) {
                    work.push(t2);
                }
                None
            }
            Op::AnchorB => {
                if anchor_b_advances(ctx, ins.pad as u8) {
                    Some(pc + 1)
                } else {
                    None
                }
            }
            Op::AnchorE => {
                if ctx.is_text_end {
                    Some(pc + 1)
                } else {
                    None
                }
            }
            Op::WBound => {
                if at_wb {
                    Some(pc + 1)
                } else {
                    None
                }
            }
            Op::NWBound => {
                if !at_wb {
                    Some(pc + 1)
                } else {
                    None
                }
            }
            Op::Save => Some(pc + 1),
            _ => None,
        };
        if let Some(t) = next {
            if t < prog.len() && closure.insert(t) {
                work.push(t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::Inst;

    fn into_set(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn position_ctx_at_start_for_empty_haystack_is_both_endpoints() {
        let ctx = PositionCtx::at_start(true);
        assert!(ctx.is_text_start);
        assert!(ctx.is_text_end);
        assert!(ctx.left_byte.is_none());
    }

    #[test]
    fn position_ctx_at_start_for_nonempty_is_not_end() {
        let ctx = PositionCtx::at_start(false);
        assert!(ctx.is_text_start);
        assert!(!ctx.is_text_end);
    }

    #[test]
    fn position_ctx_at_arbitrary_position() {
        let s = b"abc";
        let ctx0 = PositionCtx::at(0, s);
        assert!(ctx0.is_text_start);
        assert!(!ctx0.is_text_end);
        assert!(ctx0.left_byte.is_none());
        let ctx2 = PositionCtx::at(2, s);
        assert!(!ctx2.is_text_start);
        assert!(!ctx2.is_text_end);
        assert_eq!(ctx2.left_byte, Some(b'b'));
        let ctx3 = PositionCtx::at(3, s);
        assert!(ctx3.is_text_end);
        assert_eq!(ctx3.left_byte, Some(b'c'));
    }

    #[test]
    fn position_ctx_word_boundary_classifies_correctly() {
        // left None (text start) + right 'a' (word) → boundary
        let ctx = PositionCtx::at_start(false);
        assert!(ctx.at_word_boundary(Some(b'a')));
        // left 'a' + right ' ' → boundary
        let ctx = PositionCtx {
            left_byte: Some(b'a'),
            is_text_start: false,
            is_text_end: false,
        };
        assert!(ctx.at_word_boundary(Some(b' ')));
        // left 'a' + right 'b' → not a boundary
        assert!(!ctx.at_word_boundary(Some(b'b')));
        // left ' ' + right ' ' → not a boundary
        let ctx = PositionCtx {
            left_byte: Some(b' '),
            is_text_start: false,
            is_text_end: false,
        };
        assert!(!ctx.at_word_boundary(Some(b' ')));
        // left None + right None → not a boundary
        let ctx = PositionCtx::at_start(true);
        assert!(!ctx.at_word_boundary(None));
    }

    #[test]
    fn epsilon_closure_with_ctx_jmp_split_match_plain_walk() {
        // 0: SPLIT 1, 3; 1: CHAR a; 3: JMP 5; 5: CHAR b — ε walk same as plain
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept()); // filler
        prog.emit(Inst::jmp(5));
        prog.emit(Inst::char_lit(b'x')); // filler
        prog.emit(Inst::char_lit(b'b'));
        let ctx = PositionCtx::at_start(false);
        assert_eq!(
            epsilon_closure_with_ctx(&prog, &[0], ctx),
            into_set(&[0, 1, 3, 5])
        );
    }

    #[test]
    fn epsilon_closure_with_ctx_anchor_b_at_text_start_advances() {
        // 0: ANCHOR_B; 1: CHAR a; 2: MATCH — at text start anchor passes
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let ctx_start = PositionCtx::at_start(false);
        assert_eq!(
            epsilon_closure_with_ctx(&prog, &[0], ctx_start),
            into_set(&[0, 1])
        );
    }

    #[test]
    fn epsilon_closure_with_ctx_anchor_b_mid_text_blocks() {
        // Mid-text: anchor stays terminal so PC 0 stays in closure but
        // PC 1 is unreachable via the anchor.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let s = b"abc";
        let ctx_mid = PositionCtx::at(1, s);
        assert!(!ctx_mid.is_text_start);
        assert_eq!(
            epsilon_closure_with_ctx(&prog, &[0], ctx_mid),
            into_set(&[0])
        );
    }

    #[test]
    fn epsilon_closure_with_ctx_anchor_e_at_text_end_advances() {
        // 0: ANCHOR_E; 1: MATCH — at end, anchor passes → {0, 1}
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let s = b"abc";
        let ctx_end = PositionCtx::at(3, s);
        assert!(ctx_end.is_text_end);
        assert_eq!(
            epsilon_closure_with_ctx(&prog, &[0], ctx_end),
            into_set(&[0, 1])
        );
        // mid-text: anchor blocks → {0}
        let ctx_mid = PositionCtx::at(1, s);
        assert_eq!(
            epsilon_closure_with_ctx(&prog, &[0], ctx_mid),
            into_set(&[0])
        );
    }

    #[test]
    fn epsilon_closure_with_ctx_wbound_still_terminal() {
        // WBound depends on left + right; without the upcoming-byte
        // signal it stays terminal in chunk 8.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let ctx = PositionCtx::at_start(false);
        assert_eq!(epsilon_closure_with_ctx(&prog, &[0], ctx), into_set(&[0]));
    }

    #[test]
    fn epsilon_closure_with_ctx_save_is_transparent_epsilon() {
        // chunk 9: SAVE is a no-op ε in the capture-blind DFA closure.
        // The walk traverses through it to the next PC.
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let ctx = PositionCtx::at_start(false);
        assert_eq!(
            epsilon_closure_with_ctx(&prog, &[0], ctx),
            into_set(&[0, 1])
        );
    }

    #[test]
    fn epsilon_closure_with_ctx_save_chains_to_match() {
        // Multiple SAVEs in a row all walk through.
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::save(2));
        prog.emit(Inst::match_accept());
        let ctx = PositionCtx::at_start(true);
        assert_eq!(
            epsilon_closure_with_ctx(&prog, &[0], ctx),
            into_set(&[0, 1, 2])
        );
    }

    // epsilon_closure_full tests — right-byte-aware variant that
    // resolves WBound / NWBound on top of AnchorB / AnchorE.

    #[test]
    fn epsilon_closure_full_wbound_at_text_start_advances_into_word() {
        // 0: WBOUND; 1: CHAR a; 2: MATCH
        // text-start (left=None) + right='a' (word) → boundary, advance.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let ctx = PositionCtx::at_start(false);
        assert_eq!(
            epsilon_closure_full(&prog, &[0], ctx, Some(b'a')),
            into_set(&[0, 1])
        );
    }

    #[test]
    fn epsilon_closure_full_wbound_between_word_chars_blocks() {
        // left='a' + right='b' → no boundary,WBound blocks.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::char_lit(b'x'));
        prog.emit(Inst::match_accept());
        let ctx = PositionCtx {
            left_byte: Some(b'a'),
            is_text_start: false,
            is_text_end: false,
        };
        assert_eq!(
            epsilon_closure_full(&prog, &[0], ctx, Some(b'b')),
            into_set(&[0])
        );
    }

    #[test]
    fn epsilon_closure_full_nwbound_between_word_chars_advances() {
        // 0: NWBOUND; 1: CHAR a; 2: MATCH
        // left='a' + right='b' (both word) → no boundary,NWBound passes.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::NWBound));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let ctx = PositionCtx {
            left_byte: Some(b'a'),
            is_text_start: false,
            is_text_end: false,
        };
        assert_eq!(
            epsilon_closure_full(&prog, &[0], ctx, Some(b'b')),
            into_set(&[0, 1])
        );
    }

    #[test]
    fn epsilon_closure_full_anchor_b_propagates_same_as_ctx_variant() {
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let ctx_start = PositionCtx::at_start(false);
        assert_eq!(
            epsilon_closure_full(&prog, &[0], ctx_start, Some(b'a')),
            into_set(&[0, 1])
        );
    }

    #[test]
    fn epsilon_closure_full_right_byte_none_at_text_end() {
        // End-of-haystack: right_byte = None → no word char on the right.
        // ctx.is_text_end = true, left='a' → \b boundary.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let s = b"a";
        let ctx_end = PositionCtx::at(1, s);
        assert!(ctx_end.is_text_end);
        assert_eq!(
            epsilon_closure_full(&prog, &[0], ctx_end, None),
            into_set(&[0, 1, 2])
        );
    }

    #[test]
    fn epsilon_closure_full_save_is_transparent_epsilon() {
        // chunk 9: SAVE walks through in the full closure too.
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let ctx = PositionCtx::at_start(false);
        assert_eq!(
            epsilon_closure_full(&prog, &[0], ctx, Some(b'a')),
            into_set(&[0, 1])
        );
    }

    #[test]
    fn epsilon_closure_with_ctx_anchor_chain_walks_through() {
        // 0: ANCHOR_B; 1: SPLIT 2, 4; 2: CHAR a; 3: MATCH; 4: ANCHOR_E; 5: MATCH
        // At an empty-string ctx (start = end), both anchors advance,
        // splitting yields {0,1,2,4,5}.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::split(2, 4));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let ctx = PositionCtx::at_start(true);
        assert_eq!(
            epsilon_closure_with_ctx(&prog, &[0], ctx),
            into_set(&[0, 1, 2, 4, 5])
        );
    }
}
