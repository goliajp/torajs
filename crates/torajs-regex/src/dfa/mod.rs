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
//!   the cache and `build_dfa(prog, flags)` runs per `search_from_with_ws`
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
//! Future chunks: `u`-flag code-point step (chunk 10). The full set
//! of zero-width / capture opcodes is now DFA-resident:
//! - `^` (chunk 8.5) — position-aware start states + line-break entry
//!   selection in the wire.
//! - `$` (chunk 8.6a) — per-state `is_accept_at_end` precomputed by
//!   re-closing under an at-end ctx.
//! - `\b` / `\B` (chunk 8.6b) — state key upgraded to `(PC set,
//!   LeftByteAttr)` so the BFS step can re-close with `right_byte =
//!   b` and resolve WBound/NWBound under the cursor's left/right
//!   class pair.
//! - capture group SAVE (chunk 9) — DFA is capture-blind (Save is a
//!   no-op ε in the closure) and `vm::search_from_with_ws` runs a
//!   second-pass Pike VM on the DFA-found `[start..end]` window with
//!   `end_target = end` to extract captures.
//! Tracking RFC: `.claude/rfcs/20260622-pike-vm-dfa/design.md`.

pub mod ctx;
pub use ctx::{PositionCtx, epsilon_closure_full, epsilon_closure_with_ctx};

// chunk 8.6b — closure-API role split:
// - `epsilon_closure_with_ctx` is used for the BFS pre-step / post-
//   step closure (WBound stays terminal here; it'll be resolved by
//   the next BFS step when `right_byte` becomes known).
// - `epsilon_closure_full(.., right_byte = Some(b))` is used inside
//   the per-byte BFS step to resolve WBound/NWBound under the
//   correct left/right class pair.
// - `epsilon_closure_full(.., right_byte = None)` is used only for
//   `pc_set_is_accept_at_end`, where `None` truly means "no upcoming
//   byte" (cursor at haystack end) — WBound resolves against
//   `(left, non-word)`.
// Using `_full(.., None)` for BFS initial/post-step (right unknown,
// not "at end") would let WBound eagerly advance whenever the left
// is word-class, which is wrong.

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
/// One byte-transition step over a state set (presumed ε-closed).
/// `Op::Char` uses [`crate::vm::char_eq`] (case-pair under `RE_FLAG_I`);
/// `Op::AnyChar` advances on any byte except `\n` (0x0A) when
/// `RE_FLAG_S` is unset (chunk 10a — the JS spec says `.` does not
/// match line terminators unless `s` is set; the dot-all branch under
/// `s` is the only way to match `\n` via `.`); `Op::Class` tests the
/// byte then [`class_test_case_fold`] for the i-flag pair. Other ops
/// terminal — ε via [`epsilon_closure`], lookaround/backref filter
/// upstream in [`analyze`]. Out-of-range PCs (defensive) and `pc + 1`
/// past program end are dropped.
///
/// Returns only the *ready* PCs (those whose next dispatch lands at
/// `cursor + 1`). The full step output includes a *deferred* array (PCs
/// from u-flag `.` that consumed a multi-byte code-point first byte and
/// must wait `u_skip` continuation bytes before becoming ready); see
/// [`byte_step_full`].
pub fn byte_step(prog: &Program, states: &BTreeSet<usize>, byte: u8, flags: u8) -> BTreeSet<usize> {
    byte_step_full(prog, states, byte, flags).0
}

/// Full byte-step: returns `(ready, deferred)` where
/// `deferred[i]` is the set of PCs scheduled to become ready in
/// `i + 1` continuation bytes (chunk 10b — `Op::AnyChar` under
/// `RE_FLAG_U` schedules the post-`.` PC behind the UTF-8 multi-byte
/// tail). For non-u-flag patterns `deferred` is always all-empty so
/// state pool dedup matches the pre-10b BFS shape.
pub fn byte_step_full(
    prog: &Program,
    states: &BTreeSet<usize>,
    byte: u8,
    flags: u8,
) -> (BTreeSet<usize>, [BTreeSet<usize>; 3]) {
    let mut ready: BTreeSet<usize> = BTreeSet::new();
    let mut deferred: [BTreeSet<usize>; 3] = Default::default();
    let plen = prog.len();
    let s_flag = flags & crate::parser::RE_FLAG_S != 0;
    let u_flag = flags & crate::parser::RE_FLAG_U != 0;
    for &pc in states.iter() {
        if pc >= plen {
            continue;
        }
        let ins = prog.insts[pc];
        let op = match Op::from_u8(ins.op) {
            Some(o) => o,
            None => continue,
        };
        match op {
            Op::Char => {
                if crate::vm::char_eq(ins.ch, byte, flags) {
                    let n = pc + 1;
                    if n < plen {
                        ready.insert(n);
                    }
                }
            }
            Op::AnyChar => {
                if !(s_flag || byte != b'\n') {
                    continue;
                }
                let n = pc + 1;
                if n >= plen {
                    continue;
                }
                // Under u-flag, multi-byte first byte parks PC behind
                // the UTF-8 tail (matches NFA `utf8_len_for` defer in
                // `match_at`). ASCII / invalid first bytes (incl.
                // 0xC0/0xC1/0x80..0xBF/0xF5..0xFF) advance 1 byte so
                // the matcher's cursor keeps progressing.
                let u_skip = if u_flag {
                    match byte {
                        0xC2..=0xDF => 1,
                        0xE0..=0xEF => 2,
                        0xF0..=0xF4 => 3,
                        _ => 0,
                    }
                } else {
                    0
                };
                if u_skip == 0 {
                    ready.insert(n);
                } else {
                    deferred[u_skip - 1].insert(n);
                }
            }
            Op::Class => {
                let cls_idx = ins.a as usize;
                if cls_idx < prog.classes.len() {
                    let class = &prog.classes[cls_idx];
                    if class.test(byte) || class_test_case_fold(class, byte, flags) {
                        let n = pc + 1;
                        if n < plen {
                            ready.insert(n);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (ready, deferred)
}

/// `byte_step` helper (chunk 8.7) — true iff `class` contains the
/// ASCII case-pair of `byte` under `RE_FLAG_I`; otherwise false.
fn class_test_case_fold(class: &crate::charclass::CharClass, byte: u8, flags: u8) -> bool {
    if flags & crate::parser::RE_FLAG_I == 0 {
        return false;
    }
    let paired = match byte {
        b'A'..=b'Z' => byte | 0x20,
        b'a'..=b'z' => byte & !0x20,
        _ => return false,
    };
    class.test(paired)
}

/// One state of a [`DfaProgram`].
///
/// `transitions[byte]` is the destination state index (0 = dead state).
/// `is_accept` is true iff the PC set this state represents contains an
/// [`Op::Match`] PC (i.e. the NFA can accept at this byte position).
/// `is_accept_at_end` (chunk 8.6a) is true iff the PC set, when
/// re-closed under an at-end ctx (`is_text_end = true`), reaches an
/// [`Op::Match`] — so the executor can accept a match that depends on
/// `$` (`Op::AnchorE`) firing at the hay end.
///
/// The 256-way transition table is dense — every byte slot is filled at
/// build time, so the executor is a single `state = states[state].transitions[byte]`
/// step per input byte. Memory cost is `256 * 4 = 1024` bytes per state
/// plus 1 byte for the flag (padded to 4); future sparse map can replace
/// it when state counts blow past a hot-cache budget. Capture writes are
/// handled by the wire's second-pass Pike VM, not in the DFA itself
/// (chunk 9).
pub struct DfaState {
    /// `transitions[byte]` = destination state index. 0 means dead.
    pub transitions: [u32; 256],
    /// True iff the NFA PC set behind this state contains [`Op::Match`].
    pub is_accept: bool,
    /// chunk 8.6a — true iff re-closing the PC set under an at-end ctx
    /// (`is_text_end = true`) reaches [`Op::Match`]. Set offline at
    /// build time; consumed by [`dfa_search_from`] after the byte
    /// walk to honour `Op::AnchorE` (`$`) at the haystack end.
    pub is_accept_at_end: bool,
    /// chunk 8.6b — 256-bit packed mask. Bit `b` is set iff re-closing
    /// the PC set at this state with `right_byte = Some(b)` reaches
    /// `Op::Match`. Lets the executor record a zero-width accept at
    /// cursor `i` *before* stepping byte `hay[i]` — required for
    /// patterns like `/\bword\b/` where the trailing `\b` resolves
    /// against the byte the cursor is about to consume, but the
    /// resulting `Op::Match` is zero-width (it never survives a
    /// `byte_step` that doesn't consume).
    pub accept_before_byte: [u32; 8],
}

impl Default for DfaState {
    fn default() -> Self {
        Self {
            transitions: [0u32; 256],
            is_accept: false,
            is_accept_at_end: false,
            accept_before_byte: [0u32; 8],
        }
    }
}

/// Read bit `byte` of a 256-bit packed mask.
#[inline]
fn mask_get(mask: &[u32; 8], byte: u8) -> bool {
    (mask[(byte >> 5) as usize] >> (byte & 31)) & 1 != 0
}

/// Set bit `byte` of a 256-bit packed mask.
#[inline]
fn mask_set(mask: &mut [u32; 8], byte: u8) {
    mask[(byte >> 5) as usize] |= 1u32 << (byte & 31);
}

/// A built DFA — dense transition table + four anchored start state
/// indices. `states[0]` = dead (empty PC set, self-loops, never
/// accepts).
///
/// Start-state selection (chunk 8.6b):
/// - `start` — cursor at text-start (offset 0) or — under `RE_FLAG_M`
///   — immediately after `\n`. `is_text_start = true` so `^`
///   advances; left attr folds in as `TextStart` (no preceding byte
///   for WBound — `at_word_boundary` treats `None` as non-word).
/// - `start_mid_word` — cursor at offset > 0 with the just-consumed
///   byte being a word-class byte (`[A-Za-z0-9_]`). `^` blocks; WBound
///   sees `left = Word`.
/// - `start_mid_nonword` — cursor at offset > 0 with the just-
///   consumed byte being non-word and not a line-start trigger. `^`
///   blocks; WBound sees `left = NonWord`.
/// - `start_mid` — back-compat alias, equals `start_mid_nonword` (the
///   no-WBound case where `left_byte = None` had been the BFS seed).
///
/// Patterns without `Op::AnchorB` / `Op::WBound` / `Op::NWBound` dedup
/// all four start states down. Caller must gate via `prog.can_dfa`
/// + [`prog_ops_dfa_safe`].
pub struct DfaProgram {
    pub states: Vec<DfaState>,
    pub start: u32,
    pub start_mid: u32,
    pub start_mid_word: u32,
    pub start_mid_nonword: u32,
}

/// chunk 8.6b — left-byte class carried as part of a DFA state's
/// identity. The just-consumed byte's word/non-word class is the only
/// thing `Op::WBound` / `Op::NWBound` need to see (besides the upcoming
/// right byte, supplied by the BFS step), so a 3-way attr lets us key
/// the state pool by `(PC set, attr)` without exploding into a per-
/// byte map. `TextStart` covers both "no preceding byte" and the
/// `RE_FLAG_M` line-start fold-in (left = `\n` is non-word so WBound
/// sees the same boundary as text-start when right is word-class).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LeftByteAttr {
    TextStart,
    Word,
    NonWord,
}

/// Classify a byte's left-class for [`LeftByteAttr`]. Word class =
/// ASCII `[A-Za-z0-9_]`, mirroring [`crate::dfa::ctx::PositionCtx::
/// at_word_boundary`]'s `is_word_byte`.
fn attr_of_byte(b: u8) -> LeftByteAttr {
    if b.is_ascii_alphanumeric() || b == b'_' {
        LeftByteAttr::Word
    } else {
        LeftByteAttr::NonWord
    }
}

/// Pick a representative left byte for a [`LeftByteAttr`] so the
/// closure can read `ctx.left_byte` uniformly. The value only matters
/// for `at_word_boundary` (word vs non-word) and `AnchorB`-mflag
/// (`Some(b'\n')`); the wire folds the mflag line-start case into
/// `TextStart` so a representative `None`/`Some(b'a')`/`Some(b' ')`
/// is enough — mflag mid-pattern AnchorB re-fire (left = `\n`) is
/// handled separately when the BFS steps past `\n`, which surfaces in
/// the post-step set under whatever attr the actual byte produced.
fn attr_to_left_byte(attr: LeftByteAttr) -> Option<u8> {
    match attr {
        LeftByteAttr::TextStart => None,
        LeftByteAttr::Word => Some(b'a'),
        LeftByteAttr::NonWord => Some(b' '),
    }
}

/// True iff `set` contains any [`Op::Match`] PC.
fn pc_set_is_accept(prog: &Program, set: &BTreeSet<usize>) -> bool {
    set.iter()
        .any(|&pc| pc < prog.len() && matches!(Op::from_u8(prog.insts[pc].op), Some(Op::Match)))
}

/// chunk 8.6a — true iff re-closing `set` under an at-end ctx
/// (`is_text_end = true`) reaches an [`Op::Match`] PC. Used at build
/// time to populate `DfaState::is_accept_at_end`; executor consults
/// the flag after the byte walk so `Op::AnchorE` (`$`) can fire at
/// the haystack end.
///
/// chunk 8.6b — the at-end ctx now uses the state's left-byte attr
/// (instead of always `None`) and the compiled `mflag`. This lets
/// WBound resolve under `(left = attr, right = None / non-word)` so
/// patterns like `/\w$/` accept iff the live state's incoming byte
/// was word-class. mflag `Op::AnchorB` after `\n` doesn't trigger
/// here (the at-end check is about `$` / AnchorE, not `^`); but mflag
/// `AnchorE` *before* `\n` is folded in by the closure's right=None
/// path (right=None == non-word in `at_word_boundary`).
fn pc_set_is_accept_at_end(
    prog: &Program,
    set: &BTreeSet<usize>,
    attr: LeftByteAttr,
    mflag: bool,
) -> bool {
    let seeds: Vec<usize> = set.iter().copied().collect();
    let at_end_ctx = PositionCtx {
        left_byte: attr_to_left_byte(attr),
        is_text_start: matches!(attr, LeftByteAttr::TextStart),
        is_text_end: true,
        mflag,
    };
    let closed = epsilon_closure_full(prog, &seeds, at_end_ctx, None);
    pc_set_is_accept(prog, &closed)
}

/// Look up `(ready, deferred, attr)` in the BFS interning map; insert
/// + enqueue a new DFA state if absent. States with empty ready and
/// empty deferred map to state 0 (dead). Used by [`build_dfa`] for
/// both initial-seed and successor-state insertion. The attr (chunk
/// 8.6b) is part of the dedup key so states reached via different
/// left-byte classes don't collapse — they have different WBound-
/// resolution behaviour on outgoing transitions. The deferred array
/// (chunk 10b) lets u-flag `.` park PCs behind the UTF-8 multi-byte
/// tail without conflating them with ready PCs at the same cursor.
fn intern_state(
    prog: &Program,
    ready: BTreeSet<usize>,
    deferred: [BTreeSet<usize>; 3],
    attr: LeftByteAttr,
    mflag: bool,
    needs_attr_split: bool,
    states: &mut Vec<DfaState>,
    set_to_idx: &mut BTreeMap<StateKey, u32>,
    work: &mut Vec<WorkItem>,
) -> u32 {
    if ready.is_empty() && deferred.iter().all(|d| d.is_empty()) {
        return 0;
    }
    // chunk 8.6b dedup — when the program emits no `Op::WBound` /
    // `Op::NWBound`, the LeftByteAttr never affects any outgoing
    // transition, so all three attrs collapse onto the same physical
    // state. Pool key folds onto `TextStart` in that case so
    // `dfa.start == start_mid_word == start_mid_nonword` for plain
    // patterns and the state count matches the pre-8.6b BFS shape.
    let effective_attr = if needs_attr_split {
        attr
    } else {
        LeftByteAttr::TextStart
    };
    let key: StateKey = (ready, deferred, effective_attr);
    if let Some(&i) = set_to_idx.get(&key) {
        return i;
    }
    let (ready, deferred, _) = key;
    let i = states.len() as u32;
    states.push(DfaState {
        transitions: [0u32; 256],
        is_accept: pc_set_is_accept(prog, &ready),
        is_accept_at_end: pc_set_is_accept_at_end(prog, &ready, attr, mflag),
        // Mask filled in by BFS when the state is dequeued.
        accept_before_byte: [0u32; 8],
    });
    set_to_idx.insert((ready.clone(), deferred.clone(), effective_attr), i);
    work.push((ready, deferred, attr, i));
    i
}

/// State-pool key: ready PCs (u_skip=0) + per-u_skip deferred PCs +
/// LeftByteAttr. `deferred[i]` holds PCs that become ready in `i + 1`
/// continuation bytes (chunk 10b).
type StateKey = (BTreeSet<usize>, [BTreeSet<usize>; 3], LeftByteAttr);

/// BFS work-queue entry: same shape as the pool key plus the assigned
/// state index. Carries the original attr so `pc_set_is_accept_at_end`
/// sees the true left-byte class (the pool key may have folded attr
/// onto `TextStart` for dedup when `needs_attr_split` is false).
type WorkItem = (BTreeSet<usize>, [BTreeSet<usize>; 3], LeftByteAttr, u32);

/// chunk 8.6b — true iff the program (or any sub-program) emits any
/// `Op::WBound` / `Op::NWBound`. Drives the state-pool key strategy
/// in [`intern_state`]: programs with no `\b` / `\B` collapse
/// LeftByteAttr down to `TextStart`, recovering the pre-8.6b state
/// count (the attr field is a no-op for them).
fn prog_uses_word_boundary(prog: &Program) -> bool {
    fn scan(insts: &[crate::program::Inst]) -> bool {
        insts
            .iter()
            .any(|ins| matches!(Op::from_u8(ins.op), Some(Op::WBound | Op::NWBound)))
    }
    if scan(&prog.insts) {
        return true;
    }
    for sub in prog.sub_progs.iter() {
        if scan(&sub.insts) {
            return true;
        }
    }
    false
}

/// Subset-construction NFA → DFA builder (chunks 8.5/8.6a/8.6b/8.7/
/// 8.8). Seeds four start states (`start` text-start; `start_mid_word`
/// / `start_mid_nonword` per the just-consumed byte's class;
/// `start_mid` = `start_mid_nonword` for back-compat). The wire picks
/// the right entry per cursor position. `flags` threads through
/// [`byte_step`] (i-flag case-fold, chunk 8.7) and into `PositionCtx`
/// (`RE_FLAG_M` enables `Op::AnchorB` re-fire after a consumed `\n`,
/// chunk 8.8). `set_to_idx` keys are `(PC set, LeftByteAttr)` so
/// `Op::WBound` / `Op::NWBound` resolution can differ across cursor
/// classes (chunk 8.6b). State 0 = dead.
pub fn build_dfa(prog: &Program, flags: u8) -> DfaProgram {
    let mut states: Vec<DfaState> = Vec::new();
    // state 0: dead state, all transitions self-loop to 0.
    states.push(DfaState::default());

    let mut set_to_idx: BTreeMap<StateKey, u32> = BTreeMap::new();
    let empty_def: [BTreeSet<usize>; 3] = Default::default();
    for attr in [
        LeftByteAttr::TextStart,
        LeftByteAttr::Word,
        LeftByteAttr::NonWord,
    ] {
        set_to_idx.insert((BTreeSet::new(), empty_def.clone(), attr), 0);
    }

    let mflag = flags & crate::parser::RE_FLAG_M != 0;

    fn ctx_for(attr: LeftByteAttr, mflag: bool) -> PositionCtx {
        PositionCtx {
            left_byte: attr_to_left_byte(attr),
            is_text_start: matches!(attr, LeftByteAttr::TextStart),
            is_text_end: false,
            mflag,
        }
    }

    let mut work: Vec<WorkItem> = Vec::new();

    let needs_attr_split = prog_uses_word_boundary(prog);

    // chunk 8.6b — use `closure_with_ctx` (WBound terminal) for
    // initial states, not `closure_full(.., None)`. See the
    // closure-API role split note at the top of the file.
    let initial_anchored =
        epsilon_closure_with_ctx(prog, &[0], ctx_for(LeftByteAttr::TextStart, mflag));
    let start = intern_state(
        prog,
        initial_anchored,
        empty_def.clone(),
        LeftByteAttr::TextStart,
        mflag,
        needs_attr_split,
        &mut states,
        &mut set_to_idx,
        &mut work,
    );

    let initial_mid_word = epsilon_closure_with_ctx(prog, &[0], ctx_for(LeftByteAttr::Word, mflag));
    let start_mid_word = intern_state(
        prog,
        initial_mid_word,
        empty_def.clone(),
        LeftByteAttr::Word,
        mflag,
        needs_attr_split,
        &mut states,
        &mut set_to_idx,
        &mut work,
    );

    let initial_mid_nonword =
        epsilon_closure_with_ctx(prog, &[0], ctx_for(LeftByteAttr::NonWord, mflag));
    let start_mid_nonword = intern_state(
        prog,
        initial_mid_nonword,
        empty_def.clone(),
        LeftByteAttr::NonWord,
        mflag,
        needs_attr_split,
        &mut states,
        &mut set_to_idx,
        &mut work,
    );

    while let Some((cur_ready, cur_deferred, cur_attr, cur_idx)) = work.pop() {
        let mut transitions = [0u32; 256];
        let mut accept_before_byte = [0u32; 8];
        let ctx_at_cursor = ctx_for(cur_attr, mflag);
        for byte_u16 in 0u16..=255 {
            let byte = byte_u16 as u8;
            // chunk 8.6b — re-close cur_ready with right=byte to
            // resolve WBound/NWBound at the cursor given left=cur_attr
            // and right=class_of(byte). The cur_ready entered the BFS
            // with right=None, so any WBound PCs are present-but-
            // unresolved; this re-closure resolves them per-byte.
            // Closure is idempotent for already-advanced PCs, so the
            // re-run never shrinks the set.
            let cur_seeds: Vec<usize> = cur_ready.iter().copied().collect();
            let closed_with_right =
                epsilon_closure_full(prog, &cur_seeds, ctx_at_cursor, Some(byte));
            // chunk 8.6b — if the per-byte re-closure reaches Match,
            // record a zero-width accept at this cursor before the
            // byte is stepped (else `byte_step` drops Match since
            // it's not byte-consuming). The executor reads
            // `accept_before_byte` ahead of consuming `hay[i]`.
            if pc_set_is_accept(prog, &closed_with_right) {
                mask_set(&mut accept_before_byte, byte);
            }
            let (ready_from_step, mut deferred_from_step) =
                byte_step_full(prog, &closed_with_right, byte, flags);
            // chunk 10b — current deferred PCs transition based on the
            // byte's UTF-8 role. Continuation byte 0x80..0xBF
            // decrements every active u_skip by 1 (so u_skip=1 entries
            // promote into the next ready seed); any other byte breaks
            // an in-flight multi-byte sequence and kills the deferred
            // PCs (matches the NFA: an incomplete UTF-8 sequence never
            // commits).
            let is_continuation = (0x80..=0xBF).contains(&byte);
            let (promoted, mut next_deferred): (BTreeSet<usize>, [BTreeSet<usize>; 3]) =
                if is_continuation {
                    let [d0, d1, d2] = cur_deferred.clone();
                    (d0, [d1, d2, BTreeSet::new()])
                } else {
                    (BTreeSet::new(), Default::default())
                };
            // Merge the byte-step's fresh deferred PCs into the carry-
            // forward array.
            for i in 0..3 {
                if !deferred_from_step[i].is_empty() {
                    let extra = core::mem::take(&mut deferred_from_step[i]);
                    next_deferred[i].extend(extra);
                }
            }
            let mut merged_seeds: Vec<usize> = Vec::new();
            merged_seeds.extend(ready_from_step.iter().copied());
            merged_seeds.extend(promoted.iter().copied());
            let next_attr = attr_of_byte(byte);
            let next_ready = if merged_seeds.is_empty() {
                BTreeSet::new()
            } else {
                // Post-step ctx (cursor k+1): left = the byte we just
                // consumed. Under mflag this lets mid-pattern AnchorB
                // re-fire when byte == `\n`. WBound stays unresolved
                // here — `closure_with_ctx` leaves it terminal so the
                // next BFS step can resolve it under the upcoming
                // right_byte.
                let ctx_after = PositionCtx {
                    left_byte: Some(byte),
                    is_text_start: false,
                    is_text_end: false,
                    mflag,
                };
                epsilon_closure_with_ctx(prog, &merged_seeds, ctx_after)
            };
            let next_idx = intern_state(
                prog,
                next_ready,
                next_deferred,
                next_attr,
                mflag,
                needs_attr_split,
                &mut states,
                &mut set_to_idx,
                &mut work,
            );
            transitions[byte as usize] = next_idx;
        }
        states[cur_idx as usize].transitions = transitions;
        states[cur_idx as usize].accept_before_byte = accept_before_byte;
    }

    DfaProgram {
        states,
        start,
        start_mid: start_mid_nonword,
        start_mid_word,
        start_mid_nonword,
    }
}

/// Stricter than [`crate::program::Program::can_dfa`]: chunks 8.5
/// (`^`), 8.6a (`$`), 8.6b (`\b` / `\B`), and 9 (capture group SAVE)
/// have all cleared their respective opcodes from the blocker list.
/// What remains rejected here is just byte-step / op-set combinations
/// the DFA substrate doesn't model — sub-programs (lookaround) are
/// already rejected upstream by `can_dfa`.
///
/// Cleared blockers (full chain):
/// - `Op::AnchorB` (`^`, chunk 8.5) — position-aware builder resolves
///   it via `start` / `start_mid*` start states; the executor picks
///   per the cursor position (byte 0 — or a line break under
///   `RE_FLAG_M`, chunk 8.8 — selects `start`).
/// - `Op::AnchorE` (`$`, chunk 8.6a) — every DFA state precomputes
///   `is_accept_at_end` (PC set re-closed under an at-end ctx); the
///   executor consults the flag after the byte walk so `$` can fire
///   at the haystack end.
/// - `Op::WBound` / `Op::NWBound` (chunk 8.6b) — state key upgraded
///   to `(PC set, LeftByteAttr)`; BFS re-closes the set with
///   `right_byte = b` on each step, so `\b` / `\B` resolve with the
///   true left/right class pair at the cursor.
/// - `Op::Save` (chunk 9) — DFA is capture-blind (Save is a no-op ε in
///   the closure) and [`crate::vm::search_from_with_ws`] runs a
///   second-pass Pike VM on the DFA-found `[start..end]` window with
///   `end_target = end` so the winning thread's saves come out.
///
/// Sub-programs (lookaround bodies) cannot appear when `can_dfa` is
/// true (lookaround is itself a blocker in [`analyze`]) — the loop
/// over `sub_progs` is a belt-and-suspenders defensive check (it
/// always returns true for `can_dfa`-eligible programs but stays so
/// future opcode additions don't quietly slip past).
pub fn prog_ops_dfa_safe(_prog: &Program) -> bool {
    // No opcode is rejected at this layer any more — `can_dfa`
    // upstream rules out backref / lookaround which are the only
    // truly DFA-incompatible ops the AST can emit. Function stays as
    // a safety net so future opcode additions can wire a quick
    // blocker here without touching every call site.
    true
}

/// True iff the program (or any sub-program) emits any `Op::Class`.
///
/// chunk 10b — under `RE_FLAG_U` the DFA's byte-stream `byte_step`
/// can't honour code-point class semantics for non-ASCII bytes
/// (negation + `u_props` cover multi-byte code points the byte-test
/// over-/under-reports for). The hot-path gate blocks u-flag programs
/// containing any `Op::Class`; future chunks can split this into a
/// "u-safe class" analyser (ASCII-only, no negate, no `u_props`) once
/// the class encoder exposes the relevant bits cheaply.
pub fn prog_uses_class(prog: &Program) -> bool {
    fn scan(insts: &[crate::program::Inst]) -> bool {
        insts
            .iter()
            .any(|ins| matches!(Op::from_u8(ins.op), Some(Op::Class)))
    }
    if scan(&prog.insts) {
        return true;
    }
    prog.sub_progs.iter().any(|sub| scan(&sub.insts))
}

/// Drive a built [`DfaProgram`] over `hay`, returning the longest
/// match-end byte offset reachable from `dfa.start` — anchored
/// leftmost-longest semantics, starting at byte index 0 of `hay` *which
/// is also byte 0 of the original haystack*. Use this when the wire
/// is searching from `st == 0` so `Op::AnchorB` (`^`) advances through
/// the start closure.
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
/// `0..=hay.len()`.
///
/// **Cost**: one indexed table load + one branch per byte, no per-step
/// allocation. Dead-state early-out skips the haystack tail when no
/// further match is reachable.
pub fn dfa_search(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, hay, dfa.start)
}

/// Like [`dfa_search`] but enters at `dfa.start_mid_nonword` — back-
/// compat alias for `dfa_search_mid_nonword`. Used when the wire only
/// needs `^`-blocking without WBound class awareness.
pub fn dfa_search_mid(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, hay, dfa.start_mid)
}

/// chunk 8.6b — enter at `dfa.start_mid_word` (cursor `st > 0` and
/// `s[st-1]` is a word-class byte). WBound on the first step sees
/// `left = Word`. For patterns without `\b` / `\B` this dedups to
/// `dfa.start_mid_nonword`.
pub fn dfa_search_mid_word(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, hay, dfa.start_mid_word)
}

/// chunk 8.6b — enter at `dfa.start_mid_nonword` (cursor `st > 0` and
/// `s[st-1]` is non-word and not a `\n` line-start under `RE_FLAG_M`).
/// WBound on the first step sees `left = NonWord`.
pub fn dfa_search_mid_nonword(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, hay, dfa.start_mid_nonword)
}

fn dfa_search_from(dfa: &DfaProgram, hay: &[u8], start: u32) -> Option<usize> {
    let mut state = start;
    let mut last_accept: Option<usize> = None;
    if dfa.states[state as usize].is_accept {
        last_accept = Some(0);
    }
    let mut alive = true;
    for (i, &byte) in hay.iter().enumerate() {
        // chunk 8.6b — zero-width accept before stepping byte `i`.
        // Patterns like `/\bword\b/`: the trailing `\b` resolves
        // against the byte the cursor is about to consume (right =
        // byte) plus the just-consumed left byte; the resulting
        // Op::Match is zero-width so it never survives a byte_step.
        // The BFS precomputes `accept_before_byte` per state per
        // byte so the executor can record the accept here at cursor
        // `i` (not `i + 1`).
        if mask_get(&dfa.states[state as usize].accept_before_byte, byte) {
            last_accept = Some(i);
        }
        state = dfa.states[state as usize].transitions[byte as usize];
        if state == 0 {
            alive = false;
            break;
        }
        if dfa.states[state as usize].is_accept {
            last_accept = Some(i + 1);
        }
    }
    // chunk 8.6a — after consuming the haystack, if the live state's
    // PC set reaches `Op::Match` under an at-end ε-closure
    // (`Op::AnchorE` advances), record an accept at `hay.len()`. This
    // beats any earlier mid-byte accept since leftmost-longest wants
    // the longest match seen, and the at-end accept is by definition
    // the last `is_accept`-ish observation.
    if alive && dfa.states[state as usize].is_accept_at_end {
        last_accept = Some(hay.len());
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
        assert!(byte_step(&prog, &BTreeSet::new(), b'a', 0).is_empty());
    }

    #[test]
    fn byte_step_char_hit_advances_to_next_pc() {
        // 0: CHAR a; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert_eq!(byte_step(&prog, &set(&[0]), b'a', 0), set(&[1]));
    }

    #[test]
    fn byte_step_char_miss_drops_pc() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0]), b'b', 0).is_empty());
    }

    #[test]
    fn byte_step_anychar_advances_on_any_byte_under_s_flag() {
        // chunk 10a — under `s`, `.` matches every byte (dot-all).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        for b in [b'a', b'\n', 0u8, 0xff] {
            assert_eq!(
                byte_step(&prog, &set(&[0]), b, crate::parser::RE_FLAG_S),
                set(&[1]),
                "byte 0x{b:02x}"
            );
        }
    }

    #[test]
    fn byte_step_anychar_skips_newline_without_s_flag() {
        // chunk 10a — without `s`, `.` advances on every byte except
        // `\n` (0x0A). This honours the JS spec at the DFA level so the
        // hot-path gate no longer needs to require the `s` flag.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        for b in [b'a', 0u8, 0xff, b'\r'] {
            assert_eq!(
                byte_step(&prog, &set(&[0]), b, 0),
                set(&[1]),
                "byte 0x{b:02x}"
            );
        }
        assert!(byte_step(&prog, &set(&[0]), b'\n', 0).is_empty());
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
        assert_eq!(byte_step(&prog, &set(&[0]), b'a', 0), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0]), b'c', 0), set(&[1]));
        assert!(byte_step(&prog, &set(&[0]), b'd', 0).is_empty());
    }

    #[test]
    fn byte_step_jmp_and_split_are_inert() {
        // 0: JMP 2; 1: SPLIT 0,2; 2: MATCH — none of these consume bytes.
        let mut prog = Program::new();
        prog.emit(Inst::jmp(2));
        prog.emit(Inst::split(0, 2));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0, 1]), b'a', 0).is_empty());
    }

    #[test]
    fn byte_step_save_anchor_are_inert() {
        // 0: SAVE 0; 1: ANCHOR_B; 2: WBOUND; 3: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::match_accept());
        assert!(byte_step(&prog, &set(&[0, 1, 2]), b'a', 0).is_empty());
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
        assert_eq!(byte_step(&prog, &set(&[0, 2]), b'a', 0), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0, 2]), b'b', 0), set(&[3]));
    }

    #[test]
    fn byte_step_next_pc_past_end_is_dropped() {
        // 0: CHAR a — pc 0's successor 1 is past end of (1-inst) program
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        // No MATCH terminator: pc 1 is out of range, must be dropped.
        assert!(byte_step(&prog, &set(&[0]), b'a', 0).is_empty());
    }

    // build_dfa tests — composes epsilon_closure + byte_step into a
    // deterministic table. Each test asserts state count + key
    // transitions rather than the full 256-byte table for readability.

    #[test]
    fn build_dfa_empty_program_has_only_dead_state() {
        let prog = Program::new();
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa.states.len(), 1);
        assert_eq!(dfa.start, 0);
        assert!(!dfa.states[0].is_accept);
    }

    #[test]
    fn build_dfa_dead_state_self_loops_to_zero() {
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
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
        let dfa = build_dfa(&prog, 0);
        assert!(dfa.states[dfa.start as usize].is_accept);
    }

    #[test]
    fn build_dfa_single_char_literal() {
        // 0: CHAR a; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
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
        let dfa = build_dfa(&prog, 0);
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
    fn build_dfa_anychar_routes_every_byte_to_accept_under_s_flag() {
        // 0: ANY; 1: MATCH — under `s`, `.` matches every byte.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_S);
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
    fn build_dfa_anychar_skips_newline_without_s_flag() {
        // chunk 10a — without `s`, `.` routes `\n` to dead state.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        let start = dfa.start as usize;
        let target = dfa.states[start].transitions[b'a' as usize];
        assert_ne!(target, 0);
        assert!(dfa.states[target as usize].is_accept);
        assert_eq!(dfa.states[start].transitions[b'\n' as usize], 0);
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
        let dfa = build_dfa(&prog, 0);
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
        let dfa = build_dfa(&prog, 0);
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
        let dfa = build_dfa(&prog, 0);
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
        let dfa = build_dfa(&prog, 0);
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
        let dfa = build_dfa(&prog, 0);
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
        let dfa = build_dfa(&prog, 0);
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
        let dfa = build_dfa(&prog, 0);
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
        build_dfa(&prog, 0)
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
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search(&dfa, b"42"), Some(2));
        assert_eq!(dfa_search(&dfa, b"42x"), Some(2));
        assert_eq!(dfa_search(&dfa, b"x"), None);
    }

    #[test]
    fn dfa_search_anychar_consumes_exactly_one_byte() {
        // 0: ANY; 1: MATCH — chunk 10a `.` excludes `\n` without `s`.
        let dfa = build_dfa_for(&[Inst::simple(Op::AnyChar), Inst::match_accept()]);
        // Start does not accept (no MATCH in {0}); first non-`\n` byte
        // takes us to an accepting state — match length 1.
        assert_eq!(dfa_search(&dfa, b""), None);
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        // `\n` is the one byte `.` rejects without the `s` flag.
        assert_eq!(dfa_search(&dfa, b"\n"), None);
        // After the accept, the DFA stays in an accepting state for one
        // more byte (the build queues the post-accept set), so longer
        // input still reports the longest accepting position — which is
        // length 1 because the post-accept set has no further MATCH.
        assert_eq!(dfa_search(&dfa, b"ab"), Some(1));
    }

    #[test]
    fn dfa_search_anychar_matches_newline_under_s_flag() {
        // chunk 10a — with `s`, `.` covers `\n` too.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_S);
        assert_eq!(dfa_search(&dfa, b"\n"), Some(1));
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
    }

    // chunk 8.5 — position-aware build_dfa with `start` / `start_mid`.
    // `^` (AnchorB) advances through `start` (text_start=true closure)
    // but stays terminal in `start_mid` (text_start=false closure).

    #[test]
    fn build_dfa_pattern_without_anchor_dedups_start_states() {
        // Plain `a` — no AnchorB, so the text_start=true and
        // text_start=false closures coincide; the dedup map collapses
        // them to the same DFA state.
        let dfa = build_dfa_for(&[Inst::char_lit(b'a'), Inst::match_accept()]);
        assert_eq!(dfa.start, dfa.start_mid);
        assert_ne!(dfa.start, 0);
    }

    #[test]
    fn build_dfa_anchor_b_distinct_start_states() {
        // `^a`: 0: ANCHOR_B; 1: CHAR a; 2: MATCH
        // start (text_start=true): closure = {0, 1} (ANCHOR_B advances)
        // start_mid (text_start=false): closure = {0} (ANCHOR_B blocks),
        // with no byte-step out of pc 0 (it's an inert ε op).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_ne!(dfa.start, dfa.start_mid);
        // From `start`, byte 'a' advances to an accepting state.
        let via_a = dfa.states[dfa.start as usize].transitions[b'a' as usize];
        assert_ne!(via_a, 0);
        assert!(dfa.states[via_a as usize].is_accept);
        // From `start_mid`, byte 'a' dead-ends — `^` blocked the closure.
        assert_eq!(
            dfa.states[dfa.start_mid as usize].transitions[b'a' as usize],
            0
        );
    }

    #[test]
    fn dfa_search_anchor_b_matches_at_text_start() {
        // `^abc`: 0: ANCHOR_B; 1: CHAR a; 2: CHAR b; 3: CHAR c; 4: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        // anchored entry: `^abc` matches the start of "abc..." and
        // returns the match length (3 bytes consumed past anchor).
        assert_eq!(dfa_search(&dfa, b"abc"), Some(3));
        assert_eq!(dfa_search(&dfa, b"abcdef"), Some(3));
        assert_eq!(dfa_search(&dfa, b"xabc"), None);
    }

    #[test]
    fn dfa_search_mid_anchor_b_always_misses() {
        // Same `^abc`: from start_mid the AnchorB closure blocks, so
        // no input ever matches via dfa_search_mid.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search_mid(&dfa, b"abc"), None);
        assert_eq!(dfa_search_mid(&dfa, b"abcdef"), None);
        assert_eq!(dfa_search_mid(&dfa, b""), None);
    }

    #[test]
    fn dfa_search_pattern_without_anchor_both_entries_equivalent() {
        // Plain `abc` — start and start_mid coincide, so both entries
        // return identical results.
        let dfa = build_dfa_for(&[
            Inst::char_lit(b'a'),
            Inst::char_lit(b'b'),
            Inst::char_lit(b'c'),
            Inst::match_accept(),
        ]);
        for hay in [&b""[..], b"abc", b"abcd", b"axc"] {
            assert_eq!(
                dfa_search(&dfa, hay),
                dfa_search_mid(&dfa, hay),
                "hay={hay:?}"
            );
        }
    }

    #[test]
    fn dfa_search_anchor_b_alternation_anchored_branch_only() {
        // `^a|b`: SPLIT 1, 4; 1: ANCHOR_B; 2: CHAR a; 3: MATCH;
        //         4: CHAR b; 5: MATCH
        // start (text_start=true): both branches alive — accepts "a" or "b".
        // start_mid (text_start=false): anchored branch dies, only "b" path
        // remains.
        let mut prog = Program::new();
        prog.emit(Inst::split(1, 4));
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_ne!(dfa.start, dfa.start_mid);
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, b"b"), Some(1));
        // From start_mid, 'a' branch is dead — only 'b' matches.
        assert_eq!(dfa_search_mid(&dfa, b"a"), None);
        assert_eq!(dfa_search_mid(&dfa, b"b"), Some(1));
    }

    #[test]
    fn build_dfa_anchor_b_then_match_accepts_empty_at_start() {
        // `^`: 0: ANCHOR_B; 1: MATCH
        // start: closure = {0, 1} (anchor advances, match in set) →
        // start state already accepts (empty match at text-start).
        // start_mid: closure = {0} (anchor blocks) → start_mid does
        // not accept.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert!(dfa.states[dfa.start as usize].is_accept);
        assert!(!dfa.states[dfa.start_mid as usize].is_accept);
        assert_eq!(dfa_search(&dfa, b""), Some(0));
        assert_eq!(dfa_search(&dfa, b"x"), Some(0));
        assert_eq!(dfa_search_mid(&dfa, b""), None);
        assert_eq!(dfa_search_mid(&dfa, b"x"), None);
    }

    #[test]
    fn prog_ops_dfa_safe_accepts_every_can_dfa_op_after_chunk_8_6b() {
        // All zero-width and capture opcodes are DFA-resident now:
        // - AnchorB (chunk 8.5) via dual start states
        // - Save (chunk 9) via second-pass NFA at DFA boundary
        // - AnchorE (chunk 8.6a) via per-state is_accept_at_end
        // - WBound / NWBound (chunk 8.6b) via LeftByteAttr state key
        for op in [Op::AnchorB, Op::AnchorE, Op::WBound, Op::NWBound, Op::Save] {
            let mut p = Program::new();
            if matches!(op, Op::Save) {
                p.emit(Inst::save(0));
            } else {
                p.emit(Inst::simple(op));
            }
            p.emit(Inst::char_lit(b'a'));
            p.emit(Inst::match_accept());
            assert!(prog_ops_dfa_safe(&p), "{op:?} should now be safe");
        }
    }

    #[test]
    fn build_dfa_anchor_e_at_haystack_end_accepts() {
        // chunk 8.6a: `a$` — DFA must accept "a" only when the
        // haystack ends right after the `a`. Mid-byte `is_accept`
        // never fires on its own (Match PC is behind AnchorE).
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        // Standalone "a": consume one byte, then at-end accept.
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        // "ab": consume `a`, then byte `b` lives in a state whose
        // PC set has the Match-after-AnchorE reachable only at end —
        // but the haystack has not ended, so no accept here either.
        assert_eq!(dfa_search(&dfa, b"ab"), None);
        // Empty hay: start state has no path to Match via at-end
        // closure (the `Op::Char` is still required).
        assert_eq!(dfa_search(&dfa, b""), None);
    }

    #[test]
    fn build_dfa_wbound_then_word_only_matches_at_boundary() {
        // chunk 8.6b — `/\bfoo/` style: WBound at cursor=0 needs
        // left=None|non-word + right=word. Standalone "foo" or
        // " foo" hits; "xfoo" misses at offset 0 (word-word, no
        // boundary). We test directly via dfa_search/dfa_search_mid*.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::WBound));
        prog.emit(Inst::char_lit(b'f'));
        prog.emit(Inst::char_lit(b'o'));
        prog.emit(Inst::char_lit(b'o'));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        // Anchored at text-start: left=None (non-word), right='f'
        // (word) → boundary → match "foo".
        assert_eq!(dfa_search(&dfa, b"foo"), Some(3));
        // Mid-nonword entry: left=NonWord, right='f' (word) →
        // boundary → match.
        assert_eq!(dfa_search_mid_nonword(&dfa, b"foo"), Some(3));
        // Mid-word entry: left=Word, right='f' (word) → no boundary
        // → no match at offset 0.
        assert_eq!(dfa_search_mid_word(&dfa, b"foo"), None);
    }

    #[test]
    fn build_dfa_nwbound_only_matches_inside_word_run() {
        // chunk 8.6b — `/\Bfoo/`: NWBound advances when there is no
        // boundary. So "foo" at offset 0 (left=None / non-word, right
        // ='f' / word — boundary) does *not* match here.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::NWBound));
        prog.emit(Inst::char_lit(b'f'));
        prog.emit(Inst::char_lit(b'o'));
        prog.emit(Inst::char_lit(b'o'));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search(&dfa, b"foo"), None);
        assert_eq!(dfa_search_mid_nonword(&dfa, b"foo"), None);
        // Mid-word entry: left=Word, right='f' (word) — no boundary,
        // NWBound advances → match.
        assert_eq!(dfa_search_mid_word(&dfa, b"foo"), Some(3));
    }

    #[test]
    fn build_dfa_anchor_e_only_pattern_accepts_empty_hay_at_zero() {
        // `$` alone — anchored DFA at offset 0 accepts only when
        // the haystack is already empty. Mid-haystack ends are
        // handled by the outer `search_from` loop (it advances `st`
        // and re-anchors); a single `dfa_search(b"x")` call walks
        // the byte 'x', finds no consumer (start state has no
        // byte-step out), goes dead, and returns None.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        // Empty hay: start state's at-end closure reaches Match.
        assert_eq!(dfa_search(&dfa, b""), Some(0));
        // Non-empty hay: anchored DFA can't `consume` the byte (no
        // Char/AnyChar in the program), state goes dead → None.
        // The outer `search_from` is what runs dfa_search again at
        // `st=hay.len()` to find the zero-width end accept.
        assert_eq!(dfa_search(&dfa, b"x"), None);
    }

    #[test]
    fn build_dfa_save_walks_through_to_accept() {
        // chunk 9: build_dfa walks `Op::Save` as a no-op ε. The
        // capture group `(a)` compiles to `Save(0) Char(a) Save(1)
        // Match` (after the implicit whole-match Save 0/1 in
        // production; here we test the closure shape directly): the
        // resulting DFA must reach `Match` after consuming one `a`,
        // exactly like a non-captured `/a/` would.
        let mut prog = Program::new();
        prog.emit(Inst::save(0));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::save(1));
        prog.emit(Inst::match_accept());
        assert!(prog_ops_dfa_safe(&prog));
        let dfa = build_dfa(&prog, 0);
        // Start state is not accept (need to consume `a` first).
        assert!(!dfa.states[dfa.start as usize].is_accept);
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, b"axyz"), Some(1));
        assert_eq!(dfa_search(&dfa, b""), None);
        assert_eq!(dfa_search(&dfa, b"x"), None);
    }

    // chunk 8.7 — ASCII case-fold under `RE_FLAG_I`. `byte_step` and
    // `build_dfa` now thread the flag and resolve `Op::Char` /
    // `Op::Class` against the case-paired byte when `i` is set.

    use crate::parser::RE_FLAG_I;

    #[test]
    fn byte_step_i_flag_char_advances_on_both_cases() {
        // 0: CHAR 'a'; 1: MATCH — under i flag both 'a' and 'A' advance.
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        // Plain (no flag): only 'a' advances.
        assert_eq!(byte_step(&prog, &set(&[0]), b'a', 0), set(&[1]));
        assert!(byte_step(&prog, &set(&[0]), b'A', 0).is_empty());
        // Under i: both 'a' and 'A' advance.
        assert_eq!(byte_step(&prog, &set(&[0]), b'a', RE_FLAG_I), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0]), b'A', RE_FLAG_I), set(&[1]));
        // Non-alpha bytes still respect literal compare.
        assert!(byte_step(&prog, &set(&[0]), b'0', RE_FLAG_I).is_empty());
    }

    #[test]
    fn byte_step_i_flag_class_matches_case_paired_byte() {
        // 0: CLASS [a-c]; 1: MATCH — under i flag 'A' / 'B' / 'C' also
        // match.
        let mut prog = Program::new();
        let mut cc = CharClass::new();
        cc.add_range(b'a', b'c');
        let idx = prog.intern_class(&cc);
        prog.emit(Inst::class_ref(idx));
        prog.emit(Inst::match_accept());
        // Plain: only lowercase.
        assert_eq!(byte_step(&prog, &set(&[0]), b'a', 0), set(&[1]));
        assert!(byte_step(&prog, &set(&[0]), b'A', 0).is_empty());
        // i flag: uppercase pair matches via class_test_case_fold.
        assert_eq!(byte_step(&prog, &set(&[0]), b'A', RE_FLAG_I), set(&[1]));
        assert_eq!(byte_step(&prog, &set(&[0]), b'C', RE_FLAG_I), set(&[1]));
        // Out-of-class bytes still miss.
        assert!(byte_step(&prog, &set(&[0]), b'D', RE_FLAG_I).is_empty());
        assert!(byte_step(&prog, &set(&[0]), b'1', RE_FLAG_I).is_empty());
    }

    #[test]
    fn build_dfa_i_flag_literal_accepts_both_cases() {
        // /abc/i: 0: CHAR a; 1: CHAR b; 2: CHAR c; 3: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, RE_FLAG_I);
        // All eight case combinations of "abc" should accept under i.
        for hay in [
            &b"abc"[..],
            b"ABC",
            b"Abc",
            b"aBc",
            b"abC",
            b"AbC",
            b"aBC",
            b"ABc",
        ] {
            assert_eq!(dfa_search(&dfa, hay), Some(3), "hay={hay:?}");
        }
        // Non-alpha mismatch still misses.
        assert_eq!(dfa_search(&dfa, b"axc"), None);
        assert_eq!(dfa_search(&dfa, b"abd"), None);
    }

    #[test]
    fn build_dfa_no_i_flag_does_not_case_fold() {
        // Same /abc/ without flag — only "abc" matches.
        let mut prog = Program::new();
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::char_lit(b'b'));
        prog.emit(Inst::char_lit(b'c'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, 0);
        assert_eq!(dfa_search(&dfa, b"abc"), Some(3));
        assert_eq!(dfa_search(&dfa, b"ABC"), None);
        assert_eq!(dfa_search(&dfa, b"Abc"), None);
    }

    #[test]
    fn build_dfa_i_flag_class_range_case_folds() {
        // /[a-z]/i: under i flag also matches A-Z.
        let mut prog = Program::new();
        let mut lower = CharClass::new();
        lower.add_range(b'a', b'z');
        let li = prog.intern_class(&lower);
        prog.emit(Inst::class_ref(li));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, RE_FLAG_I);
        for hay in [&b"a"[..], b"z", b"M", b"Z"] {
            assert_eq!(dfa_search(&dfa, hay), Some(1), "hay={hay:?}");
        }
        // Digits stay outside the (folded) class.
        assert_eq!(dfa_search(&dfa, b"7"), None);
    }

    // chunk 8.8 — `RE_FLAG_M` multiline `^`. `build_dfa` threads
    // `mflag` into `PositionCtx`; `Op::AnchorB` advances when the ctx
    // is at text-start *or* `left_byte == Some(b'\n')`. The
    // `vm::search_from_with_ws` wire picks `dfa.start` at line-start
    // cursor positions and `dfa.start_mid` elsewhere.

    use crate::parser::RE_FLAG_M;

    #[test]
    fn build_dfa_mflag_for_anchor_b_pattern_compiles() {
        // `^a`: 0: ANCHOR_B; 1: CHAR a; 2: MATCH
        // Multiline `^` resolution is wire-level (see
        // `vm::search_from_with_ws`: line-start positions re-enter via
        // `dfa.start`). The BFS just has to produce a valid DFA — the
        // `start` state mirrors the no-flag DFA (AnchorB advances under
        // `is_text_start = true` regardless of mflag) and `start_mid`
        // still blocks AnchorB (left_byte = None in ctx_mid_default).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, RE_FLAG_M);
        // start (text_start=true closure) accepts 'a'.
        let via_a = dfa.states[dfa.start as usize].transitions[b'a' as usize];
        assert!(
            dfa.states[via_a as usize].is_accept,
            "start + 'a' must accept under mflag (text_start ctx)"
        );
        // start_mid (no left_byte) blocks AnchorB — all bytes dead.
        for b in [b'a', b'\n', b'x'] {
            assert_eq!(
                dfa.states[dfa.start_mid as usize].transitions[b as usize], 0,
                "byte {b:02x} from start_mid must be dead (wire re-enters at line-start)"
            );
        }
    }

    #[test]
    fn epsilon_closure_full_anchor_b_advances_under_mflag_after_newline() {
        // Direct ε-closure test against the new mflag semantic.
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::char_lit(b'a'));
        prog.emit(Inst::match_accept());
        // mflag = true, left = '\n': AnchorB advances.
        let ctx_after_nl = PositionCtx {
            left_byte: Some(b'\n'),
            is_text_start: false,
            is_text_end: false,
            mflag: true,
        };
        let cl = crate::dfa::epsilon_closure_full(&prog, &[0], ctx_after_nl, Some(b'a'));
        assert!(
            cl.contains(&1),
            "AnchorB must advance after \\n under mflag"
        );
        // mflag = true, left = 'a': AnchorB stays terminal.
        let ctx_after_a = PositionCtx {
            left_byte: Some(b'a'),
            is_text_start: false,
            is_text_end: false,
            mflag: true,
        };
        let cl = crate::dfa::epsilon_closure_full(&prog, &[0], ctx_after_a, Some(b'b'));
        assert!(!cl.contains(&1));
        // mflag = false, left = '\n': AnchorB stays terminal (legacy
        // semantic).
        let ctx_no_mflag = PositionCtx {
            left_byte: Some(b'\n'),
            is_text_start: false,
            is_text_end: false,
            mflag: false,
        };
        let cl = crate::dfa::epsilon_closure_full(&prog, &[0], ctx_no_mflag, Some(b'a'));
        assert!(!cl.contains(&1));
    }

    // chunk 10b — u-flag `.` matches one code point (1-4 UTF-8 bytes)
    // via the BFS deferred[u_skip] buckets. byte_step_full splits the
    // PC into the right bucket on the first byte; subsequent
    // continuation bytes promote it forward until it lands in `ready`
    // exactly `cp_len` bytes after the first.

    #[test]
    fn byte_step_full_u_flag_anychar_routes_multi_byte_to_deferred() {
        // 0: ANY; 1: MATCH
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        let u_s = crate::parser::RE_FLAG_U | crate::parser::RE_FLAG_S;
        // ASCII first byte → ready advance (u_skip = 0).
        let (ready, def) = byte_step_full(&prog, &set(&[0]), b'a', u_s);
        assert_eq!(ready, set(&[1]));
        assert!(def[0].is_empty() && def[1].is_empty() && def[2].is_empty());
        // 2-byte first (0xCE) → deferred[0] (u_skip = 1).
        let (ready, def) = byte_step_full(&prog, &set(&[0]), 0xCE, u_s);
        assert!(ready.is_empty());
        assert_eq!(def[0], set(&[1]));
        // 3-byte first (0xE6) → deferred[1] (u_skip = 2).
        let (ready, def) = byte_step_full(&prog, &set(&[0]), 0xE6, u_s);
        assert!(ready.is_empty());
        assert_eq!(def[1], set(&[1]));
        // 4-byte first (0xF0) → deferred[2] (u_skip = 3).
        let (ready, def) = byte_step_full(&prog, &set(&[0]), 0xF0, u_s);
        assert!(ready.is_empty());
        assert_eq!(def[2], set(&[1]));
        // Continuation byte alone (no prior multi-byte context) is an
        // unpaired tail under u-flag — defensive 1-byte advance per
        // `utf8_len_for`.
        let (ready, def) = byte_step_full(&prog, &set(&[0]), 0x80, u_s);
        assert_eq!(ready, set(&[1]));
        assert!(def.iter().all(|d| d.is_empty()));
    }

    #[test]
    fn byte_step_full_without_u_flag_never_defers() {
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::match_accept());
        for b in [b'a', 0xCEu8, 0xE6, 0xF0, 0x80] {
            let (_, def) = byte_step_full(&prog, &set(&[0]), b, crate::parser::RE_FLAG_S);
            assert!(
                def.iter().all(|d| d.is_empty()),
                "byte 0x{b:02x} unexpectedly deferred without u-flag"
            );
        }
    }

    #[test]
    fn dfa_search_u_flag_dot_consumes_four_byte_code_point() {
        // /^.$/u — under u-flag `.` matches one code point; the
        // anchored DFA walks 4 bytes and accepts at the haystack end
        // via `is_accept_at_end` (AnchorE closes through Match).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let u = crate::parser::RE_FLAG_U;
        let dfa = build_dfa(&prog, u);
        // 😀 = U+1F600 = F0 9F 98 80 (4 bytes).
        let smile: &[u8] = b"\xF0\x9F\x98\x80";
        assert_eq!(dfa_search(&dfa, smile), Some(4));
        // ASCII "a" — 1 byte cp, also matches.
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        // Two cps — `^.$` doesn't match.
        assert_eq!(dfa_search(&dfa, b"ab"), None);
        // 4-byte cp followed by extra ASCII — `$` fails after cp end.
        let mut smile_plus = smile.to_vec();
        smile_plus.push(b'x');
        assert_eq!(dfa_search(&dfa, &smile_plus), None);
    }

    #[test]
    fn dfa_search_u_flag_dot_two_byte_code_point_round_trip() {
        // /^.$/u over "Ω" = U+03A9 = CE A9 (2 bytes).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        let omega: &[u8] = b"\xCE\xA9";
        assert_eq!(dfa_search(&dfa, omega), Some(2));
    }

    #[test]
    fn dfa_search_u_flag_dot_rejects_truncated_multi_byte_at_end() {
        // /^.$/u over a 3-byte first byte with only 1 continuation —
        // incomplete cp at hay end must not accept (deferred PCs are
        // lost; `is_accept_at_end` operates on ready only).
        let mut prog = Program::new();
        prog.emit(Inst::simple(Op::AnchorB));
        prog.emit(Inst::simple(Op::AnyChar));
        prog.emit(Inst::simple(Op::AnchorE));
        prog.emit(Inst::match_accept());
        let dfa = build_dfa(&prog, crate::parser::RE_FLAG_U);
        let truncated: &[u8] = b"\xE6\x88"; // first 2 of 3 bytes
        assert_eq!(dfa_search(&dfa, truncated), None);
    }

    #[test]
    fn prog_uses_class_scans_inner_and_subprograms() {
        // Plain class — flagged.
        let mut prog = Program::new();
        let mut cc = CharClass::new();
        cc.add_range(b'a', b'z');
        let idx = prog.intern_class(&cc);
        prog.emit(Inst::class_ref(idx));
        prog.emit(Inst::match_accept());
        assert!(prog_uses_class(&prog));
        // No class — clear.
        let mut prog2 = Program::new();
        prog2.emit(Inst::char_lit(b'a'));
        prog2.emit(Inst::match_accept());
        assert!(!prog_uses_class(&prog2));
    }
}
