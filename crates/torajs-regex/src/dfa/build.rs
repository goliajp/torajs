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

use crate::dfa::DfaProgram;
use crate::dfa::build_helpers::{
    LeftByteAttr, attr_of_byte, ctx_for, finish_dfa, pc_set_is_accept, pc_set_is_accept_at_end,
    prog_uses_word_boundary,
};
use crate::dfa::byte_step_full;
use crate::dfa::ctx::{PositionCtx, epsilon_closure_full_into, epsilon_closure_with_ctx};
use crate::dfa::pending_class::{KPropertyVerdict, PendingClass, classify_kproperty_shape};
use crate::dfa::search::{DfaState, mask_set};
use crate::program::Program;

/// State-pool key: ready PCs (u_skip=0) + per-u_skip deferred PCs +
/// LeftByteAttr. `deferred[i]` holds PCs that become ready in `i + 1`
/// continuation bytes (chunk 10b).
type StateKey = (BTreeSet<usize>, [BTreeSet<usize>; 3], LeftByteAttr);

/// BFS work-queue entry: same shape as the pool key plus the assigned
/// state index. Carries the original attr so `pc_set_is_accept_at_end`
/// sees the true left-byte class (the pool key may have folded attr
/// onto `TextStart` for dedup when `needs_attr_split` is false).
type WorkItem = (BTreeSet<usize>, [BTreeSet<usize>; 3], LeftByteAttr, u32);

/// Round 3 Phase B sub-batch 4 attack #R-J v3 option A — compute the
/// `PendingClass` triple for a `(ready, deferred)` state being interned.
/// Pure-predicate analysis (does the ready set qualify as K-PROPERTY-
/// shaped?) lives in [`crate::dfa::pending_class::classify_kproperty_shape`];
/// this wrapper handles the side-effecting recursive `intern_state`
/// for the yes_target state and assembles the final `PendingClass`.
/// Returns `PendingClass::INERT` for any ready set the classifier
/// rejects → caller falls back to the ordinary 256-way transitions
/// path.
#[allow(clippy::too_many_arguments)]
fn compute_pending_class_for(
    prog: &Program,
    ready: &BTreeSet<usize>,
    deferred: &[BTreeSet<usize>; 3],
    attr: LeftByteAttr,
    needs_attr_split: bool,
    flags: u8,
    states: &mut Vec<DfaState>,
    set_to_idx: &mut BTreeMap<StateKey, u32>,
    work: &mut Vec<WorkItem>,
    poisoned: &mut bool,
) -> PendingClass {
    let shape = match classify_kproperty_shape(prog, ready, deferred, flags) {
        KPropertyVerdict::NoKProperty => return PendingClass::INERT,
        KPropertyVerdict::Unserveable => {
            // RFC 20260711 chunk B — K-PROPERTY present but not
            // serveable by the single-class pending slot. The state
            // still byte-steps (so the build completes), but the
            // whole DFA is marked poisoned: every consumer
            // (`resolve_dfa` / eager `dfa_runtime` / AOT bake) drops
            // it and the cp-aware Pike VM serves the program.
            *poisoned = true;
            return PendingClass::INERT;
        }
        KPropertyVerdict::Shape(s) => s,
    };
    // Compute yes_target = ε-closure(union of pc + 1 per K-PROPERTY
    // pc) and intern recursively. The recursive intern may push
    // additional states; that's fine — they fill the states vec at
    // indices < the pending state we are about to push.
    let post_ready = epsilon_closure_with_ctx(prog, &shape.yes_seeds, ctx_for(attr));
    let yes_target = intern_state(
        prog,
        post_ready,
        Default::default(),
        attr,
        needs_attr_split,
        flags,
        states,
        set_to_idx,
        work,
        poisoned,
    );
    PendingClass {
        active: 1,
        _pad: 0,
        class_idx: shape.class_idx,
        yes_target,
        no_target: 0, // dead — non-matching cp aborts the scan
    }
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
    needs_attr_split: bool,
    flags: u8,
    states: &mut Vec<DfaState>,
    set_to_idx: &mut BTreeMap<StateKey, u32>,
    work: &mut Vec<WorkItem>,
    poisoned: &mut bool,
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
    // Round 3 Phase B sub-batch 4 attack #R-J v3 option A — reserve
    // the state slot BEFORE computing pending_class so the recursive
    // intern of yes_target can dedup against this very state. v3-A's
    // K-PROPERTY-shaped multi-PC support introduces a self-referential
    // cycle for the `/\p{L}+/u` loop-body state {3, 1, 2, 4} where
    // yes_target = ε-closure({3}) = {3, 1, 2, 4} again. The placeholder
    // INERT pending_class is patched in below after the recursive
    // intern completes; the dedup map insert here ensures the cycle
    // hits the cache instead of recursing forever.
    let i = states.len() as u32;
    states.push(DfaState {
        transitions: [0u32; 256],
        is_accept: pc_set_is_accept(prog, &ready),
        is_accept_at_end: pc_set_is_accept_at_end(prog, &ready, attr),
        // Round 3 Phase B sub-batch 6 attack #R-G — patched by
        // `compute_monotone_accept` after the BFS completes and all
        // `transitions[b]` slots are populated. `false` here keeps
        // the executor's monotone-accept fast path off until then.
        monotone_accept: false,
        _pad: 0,
        // Mask filled in by BFS when the state is dequeued (skipped
        // for pending K-PROPERTY states whose `pending_class.active`
        // short-circuits the per-byte transitions path entirely).
        accept_before_byte: [0u32; 8],
        // Placeholder — patched below once compute_pending_class_for
        // returns. The recursive yes_target intern may dedup back to
        // this state's index `i` while pending_class is still INERT;
        // that's OK because the dedup hit short-circuits before the
        // recursive caller ever reads pending_class, and we patch the
        // field on `states[i]` before this function returns.
        pending_class: PendingClass::INERT,
    });
    set_to_idx.insert((ready.clone(), deferred.clone(), effective_attr), i);

    // Round 3 Phase B sub-batch 4 attack #R-J v3 option A — compute
    // the pending_class triple if the ready set is a "K-PROPERTY
    // shape" set:
    //
    // - singleton {pc} where pc is K-PROPERTY (v2 case, e.g. start
    //   state of `/\p{L}+/u`), or
    // - multi-PC set where every byte-consuming PC is K-PROPERTY with
    //   the SAME class_idx and every other PC is ε-only (Match /
    //   Split / Jmp / Save / Anchor / WBound / NWBound — none consume
    //   a byte under byte_step_full). This is the `+`/`*` Kleene
    //   loop-body case like `{Split, Class[\p{L}], Match}` where all
    //   non-Class PCs are reached only through ε-walks.
    //
    // The mid-match non-ASCII letter regression v2 §3.4.E surfaced
    // was caused by v2 declining to emit pending_class for these
    // multi-PC sets and falling back to byte_step_full's
    // `class.test(byte)` ASCII-only path. v3 closes that gap. See
    // `.claude/rfcs/20260624-uflag-100k-round3-path-a/spec-v2-25E-
    // checkpoint-fail.md` for the full root cause.
    let pending_class = compute_pending_class_for(
        prog,
        &ready,
        &deferred,
        attr,
        needs_attr_split,
        flags,
        states,
        set_to_idx,
        work,
        poisoned,
    );
    // Patch the pending_class field on the reserved slot. Until here
    // `states[i].pending_class == INERT` so any recursive dedup hit
    // already returned `i` safely.
    states[i as usize].pending_class = pending_class;

    // v4 (regex-024 regression fix) — ALWAYS enqueue for BFS, even
    // when pending_class.active != 0. The executor (v4 `dfa_search_
    // from`) now consults `transitions[byte]` FIRST and only falls
    // back to the pending_class handler when `transitions[byte] == 0`
    // (dead). For K-PROPERTY-shaped multi-PC states with a non-K-
    // PROPERTY byte-consumer in the ready set (e.g. `(\p{L})(\d+)
    // (\p{L})/u`'s post-`\d+`-loop state, whose ready set contains
    // both the digit `Op::Class` and the trailing K-PROPERTY `Op::
    // Class`), byte_step populates transitions[] for the digit branch
    // AND the ASCII letter branch; the pending_class handler then
    // covers the non-ASCII letter case (transitions[0xCE]=0 → fire
    // cp decode → yes_target). Pre-v4 these mixed states fell back to
    // chunk-10d byte_step's ASCII-only `class.test(byte)` and dropped
    // every non-ASCII letter mid-match.
    //
    // For the "pure pending" case (state 1 = singleton K-PROPERTY in
    // `/\p{L}+/u`), the byte_step still runs but populates
    // transitions[ASCII-letter] = yes_target (consistent) and
    // transitions[non-ASCII-byte] = 0 (the pending handler will fire
    // there); no behavioural change versus v3-A.
    work.push((ready, deferred, attr, i));
    i
}

/// Subset-construction NFA → DFA builder (chunks 8.5/8.6a/8.6b/8.7/
/// 8.8/9/10b). Seeds four start states (`start` text-start;
/// `start_mid_word` / `start_mid_nonword` per the just-consumed byte's
/// class; `start_mid` = `start_mid_nonword` for back-compat). The wire
/// picks the right entry per cursor position. `flags` threads through
/// [`byte_step_full`] (u-flag deferred buckets, chunk 10b); the
/// i / m / s bits are read per instruction from `Inst.pad`
/// (regexp-modifiers) — case-fold in the byte-step, `Op::AnchorB`
/// re-fire after a consumed `\n` in the ε-closure.
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

    let mut work: Vec<WorkItem> = Vec::new();

    let needs_attr_split = prog_uses_word_boundary(prog);

    let mut poisoned = false;

    let mut seed = |attr: LeftByteAttr| {
        seed_start(
            prog,
            attr,
            needs_attr_split,
            flags,
            &mut states,
            &mut set_to_idx,
            &mut work,
            &mut poisoned,
        )
    };
    let start = seed(LeftByteAttr::TextStart);
    let start_mid_word = seed(LeftByteAttr::Word);
    let start_mid_nonword = seed(LeftByteAttr::NonWord);

    let mut cur_seeds: Vec<usize> = Vec::new();
    let mut merged_seeds: Vec<usize> = Vec::new();
    // chunk 7.7 v2 step 12 Round 2 polish — hoist epsilon_closure_full
    // 的 closure + work scratch buffers across the entire BFS so the
    // 256-byte inner loop reuses one allocation instead of allocating
    // fresh BTreeSet / Vec per byte (was 256× per state).
    let mut closed_with_right: BTreeSet<usize> = BTreeSet::new();
    let mut closure_work: Vec<usize> = Vec::new();
    while let Some((cur_ready, cur_deferred, cur_attr, cur_idx)) = work.pop() {
        let mut transitions = [0u32; 256];
        let mut accept_before_byte = [0u32; 8];
        let ctx_at_cursor = ctx_for(cur_attr);
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
            epsilon_closure_full_into(
                prog,
                &cur_seeds,
                ctx_at_cursor,
                Some(byte),
                &mut closed_with_right,
                &mut closure_work,
            );
            // chunk 8.6b — if the per-byte re-closure reaches Match,
            // record a zero-width accept at this cursor before the
            // byte is stepped (else `byte_step` drops Match since
            // it's not byte-consuming). The executor reads
            // `accept_before_byte` ahead of consuming `hay[i]`.
            if pc_set_is_accept(prog, &closed_with_right) {
                mask_set(&mut accept_before_byte, byte);
            }
            let (ready_from_step, deferred_from_step) =
                byte_step_full(prog, &closed_with_right, byte, flags);
            let (next_ready, next_deferred) = advance_deferred(
                prog,
                byte,
                &cur_deferred,
                ready_from_step,
                deferred_from_step,
                &mut merged_seeds,
            );
            let next_attr = attr_of_byte(byte);
            let next_idx = intern_state(
                prog,
                next_ready,
                next_deferred,
                next_attr,
                needs_attr_split,
                flags,
                &mut states,
                &mut set_to_idx,
                &mut work,
                &mut poisoned,
            );
            transitions[byte as usize] = next_idx;
        }
        states[cur_idx as usize].transitions = transitions;
        states[cur_idx as usize].accept_before_byte = accept_before_byte;
    }

    finish_dfa(states, start, start_mid_word, start_mid_nonword, poisoned)
}

/// Seed one BFS start state: ε-close PC 0 under `attr`'s cursor ctx
/// (chunk 8.6b — `closure_with_ctx`, WBound terminal, not
/// `closure_full(.., None)`; see the closure-API role split note at
/// the top of dfa/mod.rs) and intern. The three start entries
/// (text-start / mid-word / mid-nonword) differ only in attr.
#[allow(clippy::too_many_arguments)]
fn seed_start(
    prog: &Program,
    attr: LeftByteAttr,
    needs_attr_split: bool,
    flags: u8,
    states: &mut Vec<DfaState>,
    set_to_idx: &mut BTreeMap<StateKey, u32>,
    work: &mut Vec<WorkItem>,
    poisoned: &mut bool,
) -> u32 {
    let initial = epsilon_closure_with_ctx(prog, &[0], ctx_for(attr));
    intern_state(
        prog,
        initial,
        Default::default(),
        attr,
        needs_attr_split,
        flags,
        states,
        set_to_idx,
        work,
        poisoned,
    )
}

/// chunk 10b — current deferred PCs transition based on the byte's
/// UTF-8 role. Continuation byte 0x80..0xBF decrements every active
/// u_skip by 1 (so u_skip=1 entries promote into the next ready
/// seed); any other byte breaks an in-flight multi-byte sequence and
/// kills the deferred PCs (matches the NFA: an incomplete UTF-8
/// sequence never commits). The promoted PCs merge with the byte-
/// step's fresh ready set (through the hoisted `merged_seeds` buffer
/// — chunk 7.7 v2 step 12 polish: `.clear()` drops length to 0
/// without freeing capacity) and ε-close under the post-consume ctx.
fn advance_deferred(
    prog: &Program,
    byte: u8,
    cur_deferred: &[BTreeSet<usize>; 3],
    ready_from_step: BTreeSet<usize>,
    mut deferred_from_step: [BTreeSet<usize>; 3],
    merged_seeds: &mut Vec<usize>,
) -> (BTreeSet<usize>, [BTreeSet<usize>; 3]) {
    let is_continuation = (0x80..=0xBF).contains(&byte);
    let (promoted, mut next_deferred): (BTreeSet<usize>, [BTreeSet<usize>; 3]) = if is_continuation
    {
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
    merged_seeds.clear();
    merged_seeds.extend(ready_from_step.iter().copied());
    merged_seeds.extend(promoted.iter().copied());
    let next_ready = if merged_seeds.is_empty() {
        BTreeSet::new()
    } else {
        // Post-step ctx (cursor k+1): left = the byte we just
        // consumed. Mid-pattern `Op::AnchorB` re-fires after `\n`
        // when the instruction's baked m-bit is set (per-inst
        // multiline, regexp-modifiers). WBound stays unresolved
        // here — `closure_with_ctx` leaves it terminal so the
        // next BFS step can resolve it under the upcoming
        // right_byte.
        let ctx_after = PositionCtx {
            left_byte: Some(byte),
            is_text_start: false,
            is_text_end: false,
        };
        epsilon_closure_with_ctx(prog, merged_seeds, ctx_after)
    };
    (next_ready, next_deferred)
}
