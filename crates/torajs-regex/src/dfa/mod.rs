//! NFA → DFA conversion substrate for backref-free patterns.
//!
//! Layered chunks:
//! - **Eligibility** (`analyze` + `DfaEligibility`) — pre-order AST
//!   walker reporting whether the pattern uses only DFA-representable
//!   opcodes. Result cached on `Program.can_dfa` at compile time.
//! - **ε-closure** (`epsilon_closure`) — given a `Program` and a seed
//!   PC list, return the set of PCs reachable via pure-ε transitions.
//!   Building block for subset construction.
//! - **byte step** (`byte_step`) — given a PC set already closed under
//!   ε, return the PCs reached by consuming one input byte at each
//!   byte-matching op (`Char`, `AnyChar`, `Class`). Feed back into
//!   `epsilon_closure` for the next state.
//!
//! - **subset construction** (`build_dfa`) — BFS subset construction
//!   driver that composes `epsilon_closure` + `byte_step` into a
//!   [`DfaProgram`]: dense 256-way transitions per state, deduped via
//!   `BTreeMap<BTreeSet<usize>, u32>`, state 0 reserved as the dead
//!   state (empty PC set). Builder presumes caller already verified
//!   `prog.can_dfa` (see [`analyze`]); SAVE / Anchor / WBound are
//!   currently treated as terminal in [`epsilon_closure`], so the
//!   resulting DFA does not correctly handle those features yet —
//!   future chunks will revisit them.
//! - **executor** (`dfa_search`) — straight-line byte walk that drives
//!   a built [`DfaProgram`] over a haystack and returns the longest
//!   match-end position seen (per-byte, leftmost-longest semantics).
//!   Single multiply-free index per step; on dead-state (idx 0) the
//!   walk stops since no extension can ever accept.
//! - **per-call build**, no cache yet. The chunk 6 lazy cache
//!   (`Program::dfa_cache: UnsafeCell<Option<DfaProgram>>` +
//!   `get_or_build_dfa`) shipped first but was deleted after the
//!   chunk 7.5 OnceCell audit: both `UnsafeCell` and `OnceCell`
//!   variants triggered a flaky SIGBUS in `regex-021-test-lastindex`
//!   under hot-path consumption. Root cause family (RegExp lifetime
//!   vs cached `&DfaProgram` across the SSA-lower ABI boundary) is
//!   left to a future chunk 7.6 deep audit; chunk 7 v3 wire bypasses
//!   the cache and `build_dfa(prog)` runs per `search_from_with_ws`
//!   call instead.
//!
//! - **position-aware closure** ([`ctx::PositionCtx`] +
//!   [`ctx::epsilon_closure_with_ctx`]) — chunk 8 substrate. Threads
//!   byte-position context (left-byte, text-start, text-end) through
//!   the ε-closure so `Op::AnchorB` / `Op::AnchorE` advance instead of
//!   staying terminal. Subset construction itself still uses the
//!   legacy closure; subsequent chunks thread the ctx through the
//!   builder and executor.
//!
//! Future chunks: thread `PositionCtx` through `build_dfa` and
//! `dfa_search` so the gate stops excluding Anchor-only patterns;
//! `WBound` / `NWBound` via the byte-step also seeing the right byte;
//! per-state save-mask (SAVE); `u`-flag code-point step.
//! Tracking RFC: `.claude/rfcs/20260622-pike-vm-dfa/design.md`.

pub mod ctx;
pub use ctx::{PositionCtx, epsilon_closure_with_ctx};

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use crate::node::{Node, NodeKind};
use crate::program::{Op, Program};

/// Outcome of DFA eligibility analysis.
///
/// `Eligible` means the pattern's AST uses only opcodes representable
/// in a classical Thompson → DFA subset construction. The blocker
/// variants name the specific feature that forces NFA simulation —
/// useful for future logging / heuristics ("how often do we lose DFA
/// fast path to backref vs lookaround?").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DfaEligibility {
    Eligible,
    HasBackref,
    HasLookahead,
    HasNegLookahead,
    HasLookbehind,
    HasNegLookbehind,
}

impl DfaEligibility {
    pub fn is_eligible(self) -> bool {
        matches!(self, DfaEligibility::Eligible)
    }
}

/// Walk `root` recursively, returning the first blocker encountered or
/// `Eligible` if the entire tree is DFA-compatible.
///
/// Order: self node → `child` (used by Repeat / Group / lookaround) →
/// `kids` (used by Concat / Alt). Pre-order so a blocker at the root is
/// reported without descending. Reentrant / stack depth follows the AST
/// nesting depth, which the parser already bounds via standard regex
/// grammar — no separate guard needed.
pub fn analyze(root: &Node) -> DfaEligibility {
    match root.kind {
        NodeKind::Backref => return DfaEligibility::HasBackref,
        NodeKind::Lookahead => return DfaEligibility::HasLookahead,
        NodeKind::NegLookahead => return DfaEligibility::HasNegLookahead,
        NodeKind::Lookbehind => return DfaEligibility::HasLookbehind,
        NodeKind::NegLookbehind => return DfaEligibility::HasNegLookbehind,
        _ => {}
    }
    if let Some(child) = root.child.as_ref() {
        let r = analyze(child);
        if !r.is_eligible() {
            return r;
        }
    }
    for kid in root.kids.iter() {
        let r = analyze(kid);
        if !r.is_eligible() {
            return r;
        }
    }
    DfaEligibility::Eligible
}

/// Compute the ε-closure of `seeds` under `prog` — the set of all PCs
/// reachable from any `seeds[i]` via pure-ε transitions.
///
/// ε ops handled in this chunk: [`Op::Jmp`] (unconditional), [`Op::Split`]
/// (both branches). Everything else (`Save`, the anchors, `WBound` /
/// `NWBound`, the lookaround variants, every byte-consuming op) is
/// terminal — its PC is recorded in the closure if it was reached, but
/// the walk does not follow its successor.
///
/// Save / Anchor / WBound are kept terminal here because they carry
/// state that a flat PC-set cannot represent: `Save` mutates a capture
/// slot (future chunk: track per-state save-mask), the anchors depend on
/// `pos` relative to text start / line boundaries (future chunk:
/// position-context DFA states tracking left-byte / at-start / at-end),
/// `WBound` / `NWBound` depend on the surrounding bytes. The subset
/// construction will revisit these once the per-state context is added.
///
/// PCs are deduped via the returned `BTreeSet`, so a circular `JMP`
/// chain terminates in O(n) where n = `prog.len()`. Out-of-range or
/// unknown-opcode PCs are silently dropped (defensive — mirrors
/// `vm/dispatch.rs::add_thread`).
/// One byte-transition step over a state set.
///
/// `states` is presumed to already be ε-closed under [`epsilon_closure`].
/// For each PC in `states`, inspect the op:
/// - `Op::Char` — emit `pc + 1` iff `ins.ch == byte`
/// - `Op::AnyChar` — emit `pc + 1` unconditionally (dot-all semantics;
///   the JS `s` flag is the AOT-friendly choice. Future chunks can
///   thread flag context if we need spec-exact `.` ≠ '\n' default.)
/// - `Op::Class` — emit `pc + 1` iff `prog.classes[ins.a].test(byte)`.
///   Test is ASCII-only (`u`-flag code-point tests need a separate
///   step-by-codepoint helper — future chunk).
/// - everything else — terminal in the byte step. ε ops are caller's
///   job to handle via [`epsilon_closure`]; lookaround / backref are
///   filtered upstream by [`analyze`].
///
/// Returns a deduped, sorted `BTreeSet<usize>`. Out-of-range PCs in
/// `states` (defensive) and `pc + 1` past program end are silently
/// dropped.
pub fn byte_step(prog: &Program, states: &BTreeSet<usize>, byte: u8) -> BTreeSet<usize> {
    let mut next: BTreeSet<usize> = BTreeSet::new();
    let plen = prog.len();
    for &pc in states.iter() {
        if pc >= plen {
            continue;
        }
        let ins = prog.insts[pc];
        let op = match Op::from_u8(ins.op) {
            Some(o) => o,
            None => continue,
        };
        let advances = match op {
            Op::Char => ins.ch == byte,
            Op::AnyChar => true,
            Op::Class => {
                let cls_idx = ins.a as usize;
                cls_idx < prog.classes.len() && prog.classes[cls_idx].test(byte)
            }
            _ => false,
        };
        if advances {
            let n = pc + 1;
            if n < plen {
                next.insert(n);
            }
        }
    }
    next
}

/// One state of a [`DfaProgram`].
///
/// `transitions[byte]` is the destination state index (0 = dead state).
/// `is_accept` is true iff the PC set this state represents contains an
/// [`Op::Match`] PC (i.e. the NFA can accept at this byte position).
///
/// The 256-way transition table is dense — every byte slot is filled at
/// build time, so the executor is a single `state = states[state].transitions[byte]`
/// step per input byte. Memory cost is `256 * 4 = 1024` bytes per state
/// plus 1 byte for the flag (padded to 4); future sparse map can replace
/// it when state counts blow past a hot-cache budget. Capture groups (i64
/// save slots) are *not* tracked yet — `SAVE` is terminal in
/// [`epsilon_closure`] and a future chunk will add per-state save-masks.
pub struct DfaState {
    /// `transitions[byte]` = destination state index. 0 means dead.
    pub transitions: [u32; 256],
    /// True iff the NFA PC set behind this state contains [`Op::Match`].
    pub is_accept: bool,
}

impl Default for DfaState {
    fn default() -> Self {
        Self {
            transitions: [0u32; 256],
            is_accept: false,
        }
    }
}

/// A built DFA — dense transition table + the start state index.
///
/// `states[0]` is the dead state (empty PC set, all transitions self-loop
/// back to 0, never accepts). `states[start]` is the entry, computed as
/// `epsilon_closure(prog, &[0])`. The total state count is bounded by
/// `2^prog.len()` in theory and tiny in practice for typical patterns.
///
/// **Caller contract**: `build_dfa` does not re-verify [`analyze`]
/// eligibility. Callers must check `prog.can_dfa` first and fall back to
/// the Pike VM for ineligible patterns. Even when eligible, patterns
/// involving `Op::Save` / `Op::AnchorB` / `Op::AnchorE` / `Op::WBound` /
/// `Op::NWBound` will not match correctly until the position-context /
/// save-mask chunks land; callers must additionally gate on absence of
/// those features (or pay the cost of an extra VM fallback). The
/// executor that consumes this struct will land in a follow-up chunk.
pub struct DfaProgram {
    pub states: Vec<DfaState>,
    pub start: u32,
}

/// True iff `set` contains any [`Op::Match`] PC.
fn pc_set_is_accept(prog: &Program, set: &BTreeSet<usize>) -> bool {
    set.iter()
        .any(|&pc| pc < prog.len() && matches!(Op::from_u8(prog.insts[pc].op), Some(Op::Match)))
}

/// Subset-construction NFA → DFA builder. BFS over PC-set states.
///
/// Walk:
/// 1. State 0 is reserved as the dead state (empty PC set). Any
///    transition that lands in an empty set points back to 0; the dead
///    state's own transitions all self-loop to 0 (set at construction).
/// 2. State 1 is `epsilon_closure(prog, &[0])` — the NFA entry's
///    ε-closure.
/// 3. For each unvisited state, compute the ε-closed successor set for
///    each of the 256 input bytes. Look the set up in
///    `set_to_idx: BTreeMap<BTreeSet<usize>, u32>`; reuse the existing
///    index or push a new state and enqueue it.
/// 4. Mark a state accepting iff its PC set contains [`Op::Match`].
///
/// The BTreeMap key is the canonical PC-set representation (already
/// sorted + deduped), so equivalent NFA configurations collapse to a
/// single DFA state. Build time is O(|states| · 256 · ε-closure cost);
/// for typical AOT patterns total states stay in the low hundreds.
pub fn build_dfa(prog: &Program) -> DfaProgram {
    let mut states: Vec<DfaState> = Vec::new();
    // state 0: dead state, all transitions self-loop to 0.
    states.push(DfaState::default());

    let mut set_to_idx: BTreeMap<BTreeSet<usize>, u32> = BTreeMap::new();
    set_to_idx.insert(BTreeSet::new(), 0);

    let initial = epsilon_closure(prog, &[0]);
    if initial.is_empty() {
        return DfaProgram { states, start: 0 };
    }

    let start_idx = states.len() as u32;
    states.push(DfaState {
        transitions: [0u32; 256],
        is_accept: pc_set_is_accept(prog, &initial),
    });
    set_to_idx.insert(initial.clone(), start_idx);

    // Work queue of (PC-set, state idx) pairs whose transitions are not
    // yet filled. Pop-from-back is fine — order doesn't affect the DFA
    // (BFS vs DFS just changes the state numbering of equivalent sets).
    let mut work: Vec<(BTreeSet<usize>, u32)> = Vec::new();
    work.push((initial, start_idx));

    while let Some((cur_set, cur_idx)) = work.pop() {
        let mut transitions = [0u32; 256];
        for byte_u16 in 0u16..=255 {
            let byte = byte_u16 as u8;
            let stepped = byte_step(prog, &cur_set, byte);
            let closed = if stepped.is_empty() {
                BTreeSet::new()
            } else {
                let seeds: Vec<usize> = stepped.iter().copied().collect();
                epsilon_closure(prog, &seeds)
            };

            let next_idx = if let Some(&i) = set_to_idx.get(&closed) {
                i
            } else {
                let i = states.len() as u32;
                states.push(DfaState {
                    transitions: [0u32; 256],
                    is_accept: pc_set_is_accept(prog, &closed),
                });
                set_to_idx.insert(closed.clone(), i);
                work.push((closed, i));
                i
            };
            transitions[byte as usize] = next_idx;
        }
        states[cur_idx as usize].transitions = transitions;
    }

    DfaProgram {
        states,
        start: start_idx,
    }
}

/// Stricter than [`crate::program::Program::can_dfa`]: also checks that
/// no `Op::Save` / `Op::AnchorB` / `Op::AnchorE` / `Op::WBound` /
/// `Op::NWBound` opcodes appear in the program's bytecode (or any
/// sub-program). Those ops are still terminal in [`epsilon_closure`],
/// so the built DFA silently mis-matches them; the Pike VM fallback
/// consumes any program that fails this check.
///
/// Sub-programs (lookaround bodies) cannot appear when `can_dfa` is
/// true (lookaround is itself a blocker in [`analyze`]) — the loop
/// over `sub_progs` is a belt-and-suspenders defensive check.
pub fn prog_ops_dfa_safe(prog: &Program) -> bool {
    fn scan(insts: &[crate::program::Inst]) -> bool {
        !insts.iter().any(|ins| {
            matches!(
                Op::from_u8(ins.op),
                Some(Op::Save | Op::AnchorB | Op::AnchorE | Op::WBound | Op::NWBound)
            )
        })
    }
    if !scan(&prog.insts) {
        return false;
    }
    for sub in prog.sub_progs.iter() {
        if !scan(&sub.insts) {
            return false;
        }
    }
    true
}

/// True iff the program (or any sub-program) emits an [`Op::AnyChar`]
/// instruction (i.e. the source pattern contains `.`).
///
/// The DFA's [`byte_step`] always advances on any byte at an `AnyChar`
/// PC, matching the JS `s` (dotall) flag semantics. Without the `s`
/// flag, JS `.` must *not* match `\n`. The hot-path gate uses this to
/// require either no `AnyChar` ops or the `s` flag set; otherwise the
/// pattern falls back to Pike VM where `match_at` consults flags.
pub fn prog_uses_anychar(prog: &Program) -> bool {
    fn scan(insts: &[crate::program::Inst]) -> bool {
        insts
            .iter()
            .any(|ins| matches!(Op::from_u8(ins.op), Some(Op::AnyChar)))
    }
    if scan(&prog.insts) {
        return true;
    }
    prog.sub_progs.iter().any(|sub| scan(&sub.insts))
}

/// Drive a built [`DfaProgram`] over `hay`, returning the longest
/// match-end byte offset reachable from `dfa.start` — anchored
/// leftmost-longest semantics, starting at byte index 0.
///
/// Walk:
/// 1. If `dfa.states[dfa.start]` already accepts (empty-match patterns
///    like `a*`), seed `last_accept = Some(0)`.
/// 2. For each byte at index `i`, advance `state = states[state]
///    .transitions[byte]`. If `state == 0` (dead), break — no
///    suffix can ever accept past this point.
/// 3. If the new `state` accepts, update `last_accept = Some(i + 1)`.
/// 4. Return the last accept position seen (or `None` if start didn't
///    accept and no transition reached an accepting state).
///
/// The returned offset is the byte-end of the match (exclusive), in
/// `0..=hay.len()`. Match length is the offset itself for this
/// anchored-at-0 entry point; the search-from-position variant lands
/// in a later chunk that wires the executor to `vm::search_from_with_ws`.
///
/// **Cost**: one indexed table load + one branch per byte, no per-step
/// allocation. Dead-state early-out skips the haystack tail when no
/// further match is reachable.
pub fn dfa_search(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    let mut state = dfa.start;
    let mut last_accept: Option<usize> = None;
    if dfa.states[state as usize].is_accept {
        last_accept = Some(0);
    }
    for (i, &byte) in hay.iter().enumerate() {
        state = dfa.states[state as usize].transitions[byte as usize];
        if state == 0 {
            break;
        }
        if dfa.states[state as usize].is_accept {
            last_accept = Some(i + 1);
        }
    }
    last_accept
}

pub fn epsilon_closure(prog: &Program, seeds: &[usize]) -> BTreeSet<usize> {
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
            _ => {}
        }
    }
    closure
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn char_node(b: u8) -> alloc::boxed::Box<Node> {
        let mut n = Node::new(NodeKind::Char);
        n.ch = b;
        n
    }

    #[test]
    fn single_char_is_eligible() {
        let n = char_node(b'a');
        assert_eq!(analyze(&n), DfaEligibility::Eligible);
        assert!(analyze(&n).is_eligible());
    }

    #[test]
    fn anychar_class_anchors_are_eligible() {
        for k in [
            NodeKind::Any,
            NodeKind::Class,
            NodeKind::AnchorBeg,
            NodeKind::AnchorEnd,
            NodeKind::WBound,
            NodeKind::NWBound,
        ] {
            let n = Node::new(k);
            assert_eq!(analyze(&n), DfaEligibility::Eligible, "{k:?}");
        }
    }

    #[test]
    fn concat_of_chars_is_eligible() {
        let mut concat = Node::new(NodeKind::Concat);
        concat.push_kid(char_node(b'a'));
        concat.push_kid(char_node(b'b'));
        concat.push_kid(char_node(b'c'));
        assert_eq!(analyze(&concat), DfaEligibility::Eligible);
    }

    #[test]
    fn alt_of_concats_is_eligible() {
        let mut alt = Node::new(NodeKind::Alt);
        for c in [b'a', b'b'] {
            let mut concat = Node::new(NodeKind::Concat);
            concat.push_kid(char_node(c));
            alt.push_kid(concat);
        }
        assert_eq!(analyze(&alt), DfaEligibility::Eligible);
    }

    #[test]
    fn repeat_over_class_is_eligible() {
        let mut rep = Node::new(NodeKind::Repeat);
        rep.min = 0;
        rep.max = -1;
        rep.child = Some(Node::new(NodeKind::Class));
        assert_eq!(analyze(&rep), DfaEligibility::Eligible);
    }

    #[test]
    fn group_around_char_is_eligible() {
        let mut g = Node::new(NodeKind::Group);
        g.capture_idx = 1;
        g.child = Some(char_node(b'x'));
        assert_eq!(analyze(&g), DfaEligibility::Eligible);
    }

    #[test]
    fn root_backref_is_blocker() {
        let n = Node::new(NodeKind::Backref);
        assert_eq!(analyze(&n), DfaEligibility::HasBackref);
        assert!(!analyze(&n).is_eligible());
    }

    #[test]
    fn nested_backref_is_blocker() {
        let mut concat = Node::new(NodeKind::Concat);
        concat.push_kid(char_node(b'a'));
        concat.push_kid(Node::new(NodeKind::Backref));
        assert_eq!(analyze(&concat), DfaEligibility::HasBackref);
    }

    #[test]
    fn lookahead_variants_are_blockers() {
        for (k, want) in [
            (NodeKind::Lookahead, DfaEligibility::HasLookahead),
            (NodeKind::NegLookahead, DfaEligibility::HasNegLookahead),
            (NodeKind::Lookbehind, DfaEligibility::HasLookbehind),
            (NodeKind::NegLookbehind, DfaEligibility::HasNegLookbehind),
        ] {
            let mut n = Node::new(k);
            n.child = Some(char_node(b'x'));
            assert_eq!(analyze(&n), want, "{k:?}");
        }
    }

    #[test]
    fn lookahead_deeply_nested_in_group_is_blocker() {
        let mut outer = Node::new(NodeKind::Group);
        outer.capture_idx = 1;
        let mut rep = Node::new(NodeKind::Repeat);
        rep.min = 1;
        rep.max = -1;
        let mut la = Node::new(NodeKind::Lookahead);
        la.child = Some(char_node(b'z'));
        rep.child = Some(la);
        outer.child = Some(rep);
        assert_eq!(analyze(&outer), DfaEligibility::HasLookahead);
    }

    #[test]
    fn first_blocker_wins_pre_order() {
        let mut alt = Node::new(NodeKind::Alt);
        alt.push_kid(Node::new(NodeKind::Backref));
        alt.push_kid(Node::new(NodeKind::Lookahead));
        assert_eq!(analyze(&alt), DfaEligibility::HasBackref);
    }

    #[test]
    fn empty_concat_is_eligible() {
        let n = Node::new(NodeKind::Concat);
        assert_eq!(analyze(&n), DfaEligibility::Eligible);
    }

    // Integration: parse real regex source through the parser pipeline
    // (including `resolve_backrefs` for named `\k<name>`) and check the
    // eligibility verdict against a curated truth table.
    fn parse_and_analyze(pattern: &[u8], flags: u8) -> DfaEligibility {
        use crate::parser::Parser;
        use crate::resolve::resolve_backrefs;
        let mut parser = Parser::new(pattern, flags);
        let mut root = parser.parse().expect("parse should succeed");
        let names = parser.names.clone();
        let n_captures = parser.n_captures;
        assert!(
            resolve_backrefs(&mut root, &names, n_captures),
            "resolve_backrefs should succeed for {pattern:?}"
        );
        analyze(&root)
    }

    #[test]
    fn parsed_literal_pattern_is_eligible() {
        assert_eq!(parse_and_analyze(b"abc", 0), DfaEligibility::Eligible);
    }

    #[test]
    fn parsed_alt_repeat_class_pattern_is_eligible() {
        assert_eq!(
            parse_and_analyze(b"(foo|bar)+[0-9]*\\b", 0),
            DfaEligibility::Eligible
        );
    }

    #[test]
    fn parsed_anchor_dotall_pattern_is_eligible() {
        assert_eq!(parse_and_analyze(b"^.*$", 0), DfaEligibility::Eligible);
    }

    #[test]
    fn parsed_backref_pattern_is_blocked() {
        // `(a)\1` — capture + decimal backref to it.
        assert_eq!(parse_and_analyze(b"(a)\\1", 0), DfaEligibility::HasBackref);
    }

    #[test]
    fn parsed_named_backref_pattern_is_blocked() {
        // `(?<x>a)\k<x>` — named capture + `\k<name>` backref.
        // `resolve_backrefs` rewrites this into a `Backref` node with
        // the resolved capture index; without it the walker would not
        // see the blocker.
        assert_eq!(
            parse_and_analyze(b"(?<x>a)\\k<x>", 0),
            DfaEligibility::HasBackref
        );
    }

    #[test]
    fn parsed_lookahead_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"foo(?=bar)", 0),
            DfaEligibility::HasLookahead
        );
    }

    #[test]
    fn parsed_neg_lookahead_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"foo(?!bar)", 0),
            DfaEligibility::HasNegLookahead
        );
    }

    #[test]
    fn parsed_lookbehind_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"(?<=foo)bar", 0),
            DfaEligibility::HasLookbehind
        );
    }

    #[test]
    fn parsed_neg_lookbehind_pattern_is_blocked() {
        assert_eq!(
            parse_and_analyze(b"(?<!foo)bar", 0),
            DfaEligibility::HasNegLookbehind
        );
    }

    // ε-closure tests — synthetic mini-Programs that exercise the JMP /
    // SPLIT walk plus the "terminal" treatment of every other op.

    use crate::program::Inst;

    fn into_set(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn epsilon_closure_of_empty_seed_list_is_empty() {
        let prog = Program::new();
        assert!(epsilon_closure(&prog, &[]).is_empty());
    }

    #[test]
    fn seed_out_of_range_is_dropped() {
        let prog = Program::new();
        assert!(epsilon_closure(&prog, &[0, 999]).is_empty());
    }

    #[test]
    fn seed_at_char_op_only_contains_self() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn jmp_chain_walks_to_terminal_op() {
        // 0: JMP 2 → 2: JMP 4 → 4: CHAR a → 5: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::char_lit(b'x')); // unreachable filler
        prog.emit(Inst::jmp(4));
        prog.emit(Inst::char_lit(b'y')); // unreachable filler
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 2, 4]));
    }

    #[test]
    fn split_forks_both_targets() {
        // 0: SPLIT 1, 3 — both targets are char ops (terminal)
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept()); // unreachable filler
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1, 3]));
    }

    #[test]
    fn split_to_split_walks_transitively() {
        // 0: SPLIT 1,2; 1: SPLIT 3,4; 2: char; 3: char; 4: char
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 2));
        prog.emit(Inst::split(3, 4));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1, 2, 3, 4]));
    }

    #[test]
    fn circular_jmp_terminates() {
        // 0: JMP 1; 1: JMP 0 — must not loop forever
        let mut prog = Program::new();
        prog.emit(Inst::jmp(1));
        prog.emit(Inst::jmp(0));
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0, 1]));
    }

    #[test]
    fn save_op_is_terminal_in_this_chunk() {
        // 0: SAVE 0; 1: CHAR a — closure of {0} = {0} only (not {0, 1});
        // SAVE walk is deferred to a future save-mask-aware chunk.
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn anchor_b_is_terminal_in_this_chunk() {
        // 0: ANCHOR_B; 1: CHAR a — closure of {0} = {0}; anchor walk
        // deferred to a future position-context-aware chunk.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0]), into_set(&[0]));
    }

    #[test]
    fn multi_seed_unions_per_seed_closures() {
        // 0: JMP 2; 2: CHAR a; 3: JMP 5; 5: CHAR b
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::char_lit(b'x')); // filler
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::jmp(5));
        prog.emit(Inst::char_lit(b'y')); // filler
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        assert_eq!(epsilon_closure(&prog, &[0, 3]), into_set(&[0, 2, 3, 5]));
    }

    // byte_step tests — single-byte transition over an already-ε-closed
    // PC set.

    use crate::charclass::CharClass;

    fn set(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn byte_step_empty_states_returns_empty() {
        let prog = Program::new();
        assert!(byte_step(&prog, &BTreeSet::new(), b'a').is_empty());
    }

    #[test]
    fn byte_step_char_hit_advances_to_next_pc() {
        // 0: CHAR a; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(byte_step(&prog, &set(&[0]), b'a'), set(&[1]));
    }

    #[test]
    fn byte_step_char_miss_drops_pc() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0]), b'b').is_empty());
    }

    #[test]
    fn byte_step_anychar_always_advances() {
        // 0: ANY; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        for b in [b'a', b'\n', 0u8, 0xff] {
            assert_eq!(byte_step(&prog, &set(&[0]), b), set(&[1]), "byte 0x{b:02x}");
        }
    }

    #[test]
    fn byte_step_class_hit_and_miss() {
        // 0: CLASS [a-c]; 1: MATCH
        let mut prog = Program::new();
        let mut cc = CharClass::new();
        cc.add_range(b'a', b'c');
        let idx = prog.intern_class(&cc);
        prog.emit(Inst::class_ref(idx));
        prog.emit(Inst::match_accept());
        assert_eq!(byte_step(&prog, &set(&[0]), b'a'), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0]), b'c'), set(&[1]));
        assert!(byte_step(&prog, &set(&[0]), b'd').is_empty());
    }

    #[test]
    fn byte_step_jmp_and_split_are_inert() {
        // 0: JMP 2; 1: SPLIT 0,2; 2: MATCH — none of these consume bytes.
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::split(0, 2));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0, 1]), b'a').is_empty());
    }

    #[test]
    fn byte_step_save_anchor_are_inert() {
        // 0: SAVE 0; 1: ANCHOR_B; 2: WBOUND; 3: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0, 1, 2]), b'a').is_empty());
    }

    #[test]
    fn byte_step_unions_advances_across_state_set() {
        // 0: CHAR a; 1: MATCH; 2: CHAR b; 3: MATCH
        // states {0, 2}, byte 'a' → only pc 0 advances → {1}
        // states {0, 2}, byte 'b' → only pc 2 advances → {3}
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        assert_eq!(byte_step(&prog, &set(&[0, 2]), b'a'), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0, 2]), b'b'), set(&[3]));
    }

    #[test]
    fn byte_step_next_pc_past_end_is_dropped() {
        // 0: CHAR a — pc 0's successor 1 is past end of (1-inst) program
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        // No MATCH terminator: pc 1 is out of range, must be dropped.
        assert!(byte_step(&prog, &set(&[0]), b'a').is_empty());
    }

    // build_dfa tests — composes epsilon_closure + byte_step into a
    // deterministic table. Each test asserts state count + key
    // transitions rather than the full 256-byte table for readability.

    #[test]
    fn build_dfa_empty_program_has_only_dead_state() {
        let prog = Program::new();
        let dfa = build_dfa(&prog);
        assert_eq!(dfa.states.len(), 1);
        assert_eq!(dfa.start, 0);
        assert!(!dfa.states[0].is_accept);
    }

    #[test]
    fn build_dfa_dead_state_self_loops_to_zero() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        for b in 0u16..=255 {
            assert_eq!(dfa.states[0].transitions[b as usize], 0);
        }
        assert!(!dfa.states[0].is_accept);
    }

    #[test]
    fn build_dfa_match_only_is_accept_immediately() {
        // 0: MATCH — accepts empty string. ε-closure of {0} = {0}, has MATCH.
        let mut prog = Program::new();
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        assert!(dfa.states[dfa.start as usize].is_accept);
    }

    #[test]
    fn build_dfa_single_char_literal() {
        // 0: CHAR a; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        // dead + start({0}) + accept({1}) = 3 states.
        assert_eq!(dfa.states.len(), 3);
        assert_eq!(dfa.start, 1);
        assert!(!dfa.states[dfa.start as usize].is_accept);
        let accept = dfa.states[dfa.start as usize].transitions[b'a' as usize];
        assert_ne!(accept, 0);
        assert!(dfa.states[accept as usize].is_accept);
        assert_eq!(dfa.states[dfa.start as usize].transitions[b'b' as usize], 0);
    }

    #[test]
    fn build_dfa_literal_abc_walks_to_accept() {
        // 0: CHAR a; 1: CHAR b; 2: CHAR c; 3: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        let s1 = dfa.start as usize;
        let s2 = dfa.states[s1].transitions[b'a' as usize] as usize;
        assert_ne!(s2, 0);
        let s3 = dfa.states[s2].transitions[b'b' as usize] as usize;
        assert_ne!(s3, 0);
        let s4 = dfa.states[s3].transitions[b'c' as usize] as usize;
        assert_ne!(s4, 0);
        assert!(dfa.states[s4].is_accept);
        assert!(!dfa.states[s2].is_accept);
        assert!(!dfa.states[s3].is_accept);
        // wrong-byte transitions all route to dead.
        assert_eq!(dfa.states[s1].transitions[b'z' as usize], 0);
        assert_eq!(dfa.states[s2].transitions[b'z' as usize], 0);
        assert_eq!(dfa.states[s3].transitions[b'z' as usize], 0);
    }

    #[test]
    fn build_dfa_anychar_routes_every_byte_to_accept() {
        // 0: ANY; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        let start = dfa.start as usize;
        let target = dfa.states[start].transitions[0];
        assert_ne!(target, 0);
        assert!(dfa.states[target as usize].is_accept);
        for b in 0u16..=255 {
            assert_eq!(
                dfa.states[start].transitions[b as usize], target,
                "byte 0x{b:02x}"
            );
        }
    }

    #[test]
    fn build_dfa_class_only_in_range_advances() {
        // 0: CLASS [a-c]; 1: MATCH
        let mut prog = Program::new();
        let mut cc = CharClass::new();
        cc.add_range(b'a', b'c');
        let idx = prog.intern_class(&cc);
        prog.emit(Inst::class_ref(idx));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        let start = dfa.start as usize;
        let target = dfa.states[start].transitions[b'a' as usize];
        assert_ne!(target, 0);
        assert!(dfa.states[target as usize].is_accept);
        // all in-range bytes route to the same accept state.
        assert_eq!(dfa.states[start].transitions[b'b' as usize], target);
        assert_eq!(dfa.states[start].transitions[b'c' as usize], target);
        // out-of-range bytes route to dead.
        assert_eq!(dfa.states[start].transitions[b'd' as usize], 0);
        assert_eq!(dfa.states[start].transitions[b'`' as usize], 0);
    }

    #[test]
    fn build_dfa_alternation_both_branches_accept() {
        // 0: SPLIT 1, 3; 1: CHAR a; 2: MATCH; 3: CHAR b; 4: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        let start = dfa.start as usize;
        let via_a = dfa.states[start].transitions[b'a' as usize];
        let via_b = dfa.states[start].transitions[b'b' as usize];
        assert_ne!(via_a, 0);
        assert_ne!(via_b, 0);
        assert!(dfa.states[via_a as usize].is_accept);
        assert!(dfa.states[via_b as usize].is_accept);
        // 'c' from start routes to dead.
        assert_eq!(dfa.states[start].transitions[b'c' as usize], 0);
    }

    #[test]
    fn build_dfa_kleene_star_self_loops_and_accepts_empty() {
        // a*: 0: SPLIT 1, 3; 1: CHAR a; 2: JMP 0; 3: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::jmp(0));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        // ε-closure({0}) walks SPLIT to {1, 3}; 3 = MATCH → start accepts.
        assert!(dfa.states[dfa.start as usize].is_accept);
        let via_a = dfa.states[dfa.start as usize].transitions[b'a' as usize];
        assert_ne!(via_a, 0);
        // After consuming 'a' we are back at an equivalent ε-closed set.
        assert!(dfa.states[via_a as usize].is_accept);
        assert_eq!(dfa.states[dfa.start as usize].transitions[b'b' as usize], 0);
    }

    #[test]
    fn build_dfa_kleene_plus_self_loops_but_not_empty() {
        // a+: 0: CHAR a; 1: SPLIT 0, 2; 2: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::split(0, 2));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        // Start ε-closure = {0} — no MATCH yet, so start does NOT accept.
        assert!(!dfa.states[dfa.start as usize].is_accept);
        let via_a = dfa.states[dfa.start as usize].transitions[b'a' as usize];
        assert_ne!(via_a, 0);
        assert!(dfa.states[via_a as usize].is_accept);
        // Repeated 'a' from via_a must land on the same state (dedup).
        let via_aa = dfa.states[via_a as usize].transitions[b'a' as usize];
        assert_eq!(via_aa, via_a, "kleene-plus loops back to itself");
        assert_eq!(dfa.states[via_a as usize].transitions[b'b' as usize], 0);
    }

    #[test]
    fn build_dfa_dedup_equivalent_pc_sets() {
        // (a|a)b — both alternatives converge on the same PC set after 'a';
        // dedup must collapse them so the total state count stays minimal.
        // 0: SPLIT 1, 3; 1: CHAR a; 2: JMP 4; 3: CHAR a; 4: CHAR b; 5: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 3));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::jmp(4));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        let start = dfa.start as usize;
        let after_a = dfa.states[start].transitions[b'a' as usize];
        assert_ne!(after_a, 0);
        let accept = dfa.states[after_a as usize].transitions[b'b' as usize];
        assert_ne!(accept, 0);
        assert!(dfa.states[accept as usize].is_accept);
        // dead + start + after_a + accept = 4 states (no duplicate after_a).
        assert_eq!(dfa.states.len(), 4);
    }

    #[test]
    fn build_dfa_class_after_class_routes_only_via_lower_then_digit() {
        // [a-z][0-9]: CLASS lower; CLASS digit; MATCH
        let mut prog = Program::new();
        let mut lower = CharClass::new();
        lower.add_range(b'a', b'z');
        let li = prog.intern_class(&lower);
        let mut digit = CharClass::new();
        digit.add_range(b'0', b'9');
        let di = prog.intern_class(&digit);
        prog.emit(Inst::class_ref(li));
        prog.emit(Inst::class_ref(di));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        let start = dfa.start as usize;
        let s1 = dfa.states[start].transitions[b'a' as usize];
        assert_ne!(s1, 0);
        let s2 = dfa.states[s1 as usize].transitions[b'5' as usize];
        assert_ne!(s2, 0);
        assert!(dfa.states[s2 as usize].is_accept);
        // Uppercase from start = dead.
        assert_eq!(dfa.states[start].transitions[b'A' as usize], 0);
        // Digit from start = dead (must consume lower first).
        assert_eq!(dfa.states[start].transitions[b'5' as usize], 0);
        // Letter from s1 = dead (need a digit, not another letter).
        assert_eq!(dfa.states[s1 as usize].transitions[b'x' as usize], 0);
    }

    #[test]
    fn build_dfa_miss_routes_to_dead_then_self_loops() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        // miss from start lands on dead (idx 0)…
        let dead = dfa.states[dfa.start as usize].transitions[b'b' as usize];
        assert_eq!(dead, 0);
        // …and dead self-loops on every byte.
        for b in 0u16..=255 {
            assert_eq!(dfa.states[0].transitions[b as usize], 0);
        }
    }

    // dfa_search tests — anchored leftmost-longest driver.

    fn build_dfa_for(insts: &[Inst]) -> DfaProgram {
        let mut prog = Program::new();
        for ins in insts {
            prog.emit(*ins);
        }
        build_dfa(&prog)
    }

    #[test]
    fn dfa_search_literal_matches_at_exact_length() {
        // 0: CHAR a; 1: CHAR b; 2: CHAR c; 3: MATCH
        let dfa = build_dfa_for(&[
            Inst::char_lit(b'a'),
            Inst::char_lit(b'b'),
            Inst::char_lit(b'c'),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, b"abc"), Some(3));
        assert_eq!(dfa_search(&dfa, b"abcd"), Some(3)); // trailing byte ignored
    }

    #[test]
    fn dfa_search_literal_misses_on_first_byte_mismatch() {
        let dfa = build_dfa_for(&[Inst::char_lit(b'a'), Inst::match_accept()]);
        assert_eq!(dfa_search(&dfa, b"b"), None);
        assert_eq!(dfa_search(&dfa, b""), None);
    }

    #[test]
    fn dfa_search_match_only_accepts_empty() {
        let dfa = build_dfa_for(&[Inst::match_accept()]);
        assert_eq!(dfa_search(&dfa, b""), Some(0));
        // accepts empty prefix even with trailing bytes — dead-state
        // halts the walk but the seeded `Some(0)` survives.
        assert_eq!(dfa_search(&dfa, b"anything"), Some(0));
    }

    #[test]
    fn dfa_search_kleene_star_matches_empty_and_extends() {
        // a*: 0: SPLIT 1, 3; 1: CHAR a; 2: JMP 0; 3: MATCH
        let dfa = build_dfa_for(&[
            Inst::split(1, 3),
            Inst::char_lit(b'a'),
            Inst::jmp(0),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, b""), Some(0));
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, b"aaaa"), Some(4));
        // hits dead on 'b' after consuming a's, returns longest seen.
        assert_eq!(dfa_search(&dfa, b"aab"), Some(2));
        assert_eq!(dfa_search(&dfa, b"b"), Some(0));
    }

    #[test]
    fn dfa_search_kleene_plus_requires_one_byte() {
        // a+: 0: CHAR a; 1: SPLIT 0, 2; 2: MATCH
        let dfa = build_dfa_for(&[
            Inst::char_lit(b'a'),
            Inst::split(0, 2),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, b""), None);
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, b"aaaa"), Some(4));
        assert_eq!(dfa_search(&dfa, b"b"), None);
    }

    #[test]
    fn dfa_search_alternation_takes_either_branch() {
        // 0: SPLIT 1, 3; 1: CHAR a; 2: MATCH; 3: CHAR b; 4: MATCH
        let dfa = build_dfa_for(&[
            Inst::split(1, 3),
            Inst::char_lit(b'a'),
            Inst::match_accept(),
            Inst::char_lit(b'b'),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, b"b"), Some(1));
        assert_eq!(dfa_search(&dfa, b"c"), None);
    }

    #[test]
    fn dfa_search_leftmost_longest_prefers_extended_accept() {
        // a*b: 0: SPLIT 1, 3; 1: CHAR a; 2: JMP 0; 3: CHAR b; 4: MATCH
        // Start does not accept (no MATCH in ε-closure of {0,1,3}).
        // 'aaab' should match 4 bytes.
        let dfa = build_dfa_for(&[
            Inst::split(1, 3),
            Inst::char_lit(b'a'),
            Inst::jmp(0),
            Inst::char_lit(b'b'),
            Inst::match_accept(),
        ]);
        assert_eq!(dfa_search(&dfa, b"b"), Some(1));
        assert_eq!(dfa_search(&dfa, b"ab"), Some(2));
        assert_eq!(dfa_search(&dfa, b"aaab"), Some(4));
        // No 'b' tail → no match (the executor is anchored leftmost-longest,
        // doesn't yet retry suffixes — that's the search-from-position chunk).
        assert_eq!(dfa_search(&dfa, b"aaa"), None);
    }

    #[test]
    fn dfa_search_dead_state_short_circuits_after_miss() {
        // Pattern `ab`: walking "ac…" should stop at index 1 (state hits
        // dead) and return None regardless of tail content.
        let dfa = build_dfa_for(&[
            Inst::char_lit(b'a'),
            Inst::char_lit(b'b'),
            Inst::match_accept(),
        ]);
        // Even with a megabyte of garbage, the executor only reads up to
        // the first mismatch — but we just sanity-check the answer.
        assert_eq!(dfa_search(&dfa, b"ac"), None);
        assert_eq!(dfa_search(&dfa, b"axxxxxxxxxx"), None);
    }

    #[test]
    fn dfa_search_class_range_matches_in_range_bytes() {
        // [0-9]+: CLASS [0-9]; SPLIT 0, 2; MATCH
        let mut prog = Program::new();
        let mut digit = CharClass::new();
        digit.add_range(b'0', b'9');
        let di = prog.intern_class(&digit);
        prog.emit(Inst::class_ref(di));
        prog.emit(Inst::split(0, 2));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog);
        assert_eq!(dfa_search(&dfa, b"42"), Some(2));
        assert_eq!(dfa_search(&dfa, b"42x"), Some(2));
        assert_eq!(dfa_search(&dfa, b"x"), None);
    }

    #[test]
    fn dfa_search_anychar_consumes_exactly_one_byte() {
        // 0: ANY; 1: MATCH
        let dfa = build_dfa_for(&[Inst::simple(Op::AnyChar), Inst::match_accept()]);
        // Start does not accept (no MATCH in {0}); first byte takes us
        // to an accepting state — match length 1 regardless of byte.
        assert_eq!(dfa_search(&dfa, b""), None);
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, b"\n"), Some(1));
        // After the accept, the DFA stays in an accepting state for one
        // more byte (the build queues the post-accept set), so longer
        // input still reports the longest accepting position — which is
        // length 1 because the post-accept set has no further MATCH.
        assert_eq!(dfa_search(&dfa, b"ab"), Some(1));
    }
}
