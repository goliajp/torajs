//! DFA builder — subset-construction BFS that walks the NFA program
//! and materialises the [`super::search::DfaProgram`] consumed by
//! [`super::search::dfa_search`].
//!
//! Owns the BFS state-pool key types ([`StateKey`] / [`WorkItem`]),
//! the [`LeftByteAttr`] enum that distinguishes left-byte cursor
//! classes (chunk 8.6b), the per-state accept analysers
//! ([`pc_set_is_accept`] / [`pc_set_is_accept_at_end`]) and the
//! pool-intern logic ([`intern_state`]). Lives separate from the
//! search executor because the build-time substrate is much larger
//! than the read-only consumer.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use crate::dfa::byte_step_full;
use crate::dfa::ctx::{PositionCtx, epsilon_closure_full, epsilon_closure_with_ctx};
use crate::dfa::search::{DfaProgram, DfaState, mask_set};
use crate::program::{Op, Program};

/// chunk 8.6b — left-byte class carried as part of a DFA state's
/// identity. The just-consumed byte's word/non-word class is the only
/// thing `Op::WBound` / `Op::NWBound` need to see (besides the upcoming
/// right byte, supplied by the BFS step), so a 3-way attr lets us key
/// the state pool by `(PC set, attr)` without exploding into a per-
/// byte map. `TextStart` covers both "no preceding byte" and the
/// `RE_FLAG_M` line-start fold-in (left = `\n` is non-word so WBound
/// sees the same boundary as text-start when right is word-class).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum LeftByteAttr {
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

/// State-pool key: ready PCs (u_skip=0) + per-u_skip deferred PCs +
/// LeftByteAttr. `deferred[i]` holds PCs that become ready in `i + 1`
/// continuation bytes (chunk 10b).
type StateKey = (BTreeSet<usize>, [BTreeSet<usize>; 3], LeftByteAttr);

/// BFS work-queue entry: same shape as the pool key plus the assigned
/// state index. Carries the original attr so `pc_set_is_accept_at_end`
/// sees the true left-byte class (the pool key may have folded attr
/// onto `TextStart` for dedup when `needs_attr_split` is false).
type WorkItem = (BTreeSet<usize>, [BTreeSet<usize>; 3], LeftByteAttr, u32);

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
/// 8.8/9/10b). Seeds four start states (`start` text-start;
/// `start_mid_word` / `start_mid_nonword` per the just-consumed byte's
/// class; `start_mid` = `start_mid_nonword` for back-compat). The wire
/// picks the right entry per cursor position. `flags` threads through
/// [`byte_step_full`] (i-flag case-fold, chunk 8.7; u-flag deferred
/// buckets, chunk 10b) and into `PositionCtx` (`RE_FLAG_M` enables
/// `Op::AnchorB` re-fire after a consumed `\n`, chunk 8.8).
/// `set_to_idx` keys are `(ready, deferred[3], LeftByteAttr)` so
/// `Op::WBound` / `Op::NWBound` resolution can differ across cursor
/// classes (chunk 8.6b) and in-flight u-flag multi-byte sequences
/// don't conflate with ready PCs (chunk 10b). State 0 = dead.
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
    // closure-API role split note at the top of dfa/mod.rs.
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

    let mut cur_seeds: Vec<usize> = Vec::new();
    let mut merged_seeds: Vec<usize> = Vec::new();
    while let Some((cur_ready, cur_deferred, cur_attr, cur_idx)) = work.pop() {
        let mut transitions = [0u32; 256];
        let mut accept_before_byte = [0u32; 8];
        let ctx_at_cursor = ctx_for(cur_attr, mflag);
        // chunk 7.7 v2 step 12 polish — hoist cur_seeds Vec materialise
        // outside the 256-byte loop; cur_ready is invariant within this
        // BFS step so the same Vec contents apply to every byte.
        cur_seeds.clear();
        cur_seeds.extend(cur_ready.iter().copied());
        for byte_u16 in 0u16..=255 {
            let byte = byte_u16 as u8;
            // chunk 8.6b — re-close cur_ready with right=byte to
            // resolve WBound/NWBound at the cursor given left=cur_attr
            // and right=class_of(byte). The cur_ready entered the BFS
            // with right=None, so any WBound PCs are present-but-
            // unresolved; this re-closure resolves them per-byte.
            // Closure is idempotent for already-advanced PCs, so the
            // re-run never shrinks the set.
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
            // chunk 7.7 v2 step 12 polish — reuse hoisted merged_seeds
            // buffer; .clear() drops length to 0 without freeing capacity
            // so subsequent extend reuses the existing allocation.
            merged_seeds.clear();
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
