//! DFA builder — subset-construction BFS that walks the NFA program
//! and materialises the [`super::search::DfaProgram`] consumed by
//! [`super::search::dfa_search`].
//!
//! Owns the BFS driver over the pool in [`super::pool`],
//! the [`LeftByteAttr`] enum that distinguishes left-byte cursor
//! classes (chunk 8.6b), the per-state accept analysers
//! ([`pc_set_is_accept`] / [`pc_set_is_accept_at_end`]) and the
//! pool-intern logic ([`intern_state`]). Lives separate from the
//! search executor because the build-time substrate is much larger
//! than the read-only consumer.

use alloc::vec::Vec;

use crate::dfa::DfaProgram;
use crate::dfa::build_helpers::{
    LeftByteAttr, attr_of_byte, ctx_for, finish_dfa, pc_set_is_accept, pc_set_is_accept_at_end,
    prog_uses_word_boundary,
};
use crate::dfa::ctx::{PositionCtx, closure_into};
use crate::dfa::pending_class::{KPropertyVerdict, PendingClass, classify_kproperty_shape};
use crate::dfa::pool::{PcList, Pool, flatten_key};
use crate::dfa::search::{DfaState, mask_set};
use crate::dfa::sparse::SparseSet;
use crate::dfa::step::byte_step_into;
use crate::program::Program;

/// Round 3 Phase B sub-batch 4 attack #R-J v3 option A — compute the
/// `PendingClass` triple for a `(ready, deferred)` state being interned.
/// Pure-predicate analysis (does the ready set qualify as K-PROPERTY-
/// shaped?) lives in [`crate::dfa::pending_class::classify_kproperty_shape`];
/// this wrapper handles the side-effecting recursive `intern_state`
/// for the yes_target state and assembles the final `PendingClass`.
/// Returns `PendingClass::INERT` for any ready set the classifier
/// rejects → caller falls back to the ordinary 256-way transitions
/// path.
fn compute_pending_class_for(
    prog: &Program,
    ready: &[u32],
    deferred: &[PcList; 3],
    attr: LeftByteAttr,
    needs_attr_split: bool,
    flags: u8,
    pool: &mut Pool,
) -> PendingClass {
    let shape = match classify_kproperty_shape(prog, ready, deferred, flags) {
        KPropertyVerdict::NoKProperty => return PendingClass::INERT,
        KPropertyVerdict::Unserveable => {
            // RFC 20260711 chunk B — K-PROPERTY present but not
            // serveable by the single-class pending slot. The whole
            // DFA is marked poisoned: every consumer (`resolve_dfa` /
            // eager `dfa_runtime` / AOT bake) drops it and the
            // cp-aware Pike VM serves the program, so `build_dfa`
            // stops the BFS at the first poisoned intern.
            pool.poisoned = true;
            return PendingClass::INERT;
        }
        KPropertyVerdict::Shape(s) => s,
    };
    // Compute yes_target = ε-closure(union of pc + 1 per K-PROPERTY
    // pc) and intern recursively. The recursive intern may push
    // additional states; that's fine — they fill the states vec at
    // indices < the pending state we are about to push.
    let yes_seeds: PcList = shape.yes_seeds.iter().map(|&pc| pc as u32).collect();
    let post_ready = pool.closure_sorted(prog, &yes_seeds, ctx_for(attr));
    let yes_target = intern_state(
        prog,
        post_ready,
        Default::default(),
        attr,
        needs_attr_split,
        flags,
        pool,
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
/// and enqueue a new DFA state if absent. States with empty ready and
/// empty deferred map to state 0 (dead). Used by [`build_dfa`] for
/// both initial-seed and successor-state insertion. The attr (chunk
/// 8.6b) is part of the dedup key so states reached via different
/// left-byte classes don't collapse — they have different WBound-
/// resolution behaviour on outgoing transitions. The deferred array
/// (chunk 10b) lets u-flag `.` park PCs behind the UTF-8 multi-byte
/// tail without conflating them with ready PCs at the same cursor.
fn intern_state(
    prog: &Program,
    ready: PcList,
    deferred: [PcList; 3],
    attr: LeftByteAttr,
    needs_attr_split: bool,
    flags: u8,
    pool: &mut Pool,
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
    flatten_key(&mut pool.key_scratch, &ready, &deferred, effective_attr);
    let slot = match pool.set_to_idx.find(&pool.keys, &pool.key_scratch) {
        Ok(i) => return i,
        Err(slot) => slot,
    };
    // Round 3 Phase B sub-batch 4 attack #R-J v3 option A — reserve
    // the state slot BEFORE computing pending_class so the recursive
    // intern of yes_target can dedup against this very state. v3-A's
    // K-PROPERTY-shaped multi-PC support introduces a self-referential
    // cycle for the `/\p{L}+/u` loop-body state {3, 1, 2, 4} where
    // yes_target = ε-closure({3}) = {3, 1, 2, 4} again. The placeholder
    // INERT pending_class is patched in below after the recursive
    // intern completes; the dedup map insert here ensures the cycle
    // hits the cache instead of recursing forever.
    let i = pool.states.len() as u32;
    pool.states.push(DfaState {
        transitions: [0u32; 256],
        is_accept: pc_set_is_accept(prog, &ready),
        is_accept_at_end: pc_set_is_accept_at_end(
            prog,
            &ready,
            attr,
            &mut pool.closed,
            &mut pool.closure_work,
        ),
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
    pool.keys.push(pool.key_scratch.clone());
    pool.set_to_idx.insert_at(slot, i);

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
    // `class.test(byte)` ASCII-only path. v3 closes that gap.
    let pending_class =
        compute_pending_class_for(prog, &ready, &deferred, attr, needs_attr_split, flags, pool);
    // Patch the pending_class field on the reserved slot. Until here
    // `states[i].pending_class == INERT` so any recursive dedup hit
    // already returned `i` safely.
    pool.states[i as usize].pending_class = pending_class;

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
    pool.work.push((ready, deferred, attr, i));
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
    let mut pool = Pool::new(prog);
    // state 0: dead state, all transitions self-loop to 0. Its key is
    // never looked up — `intern_state` answers empty sets directly.
    pool.states.push(DfaState::default());
    pool.keys.push(PcList::new());

    let needs_attr_split = prog_uses_word_boundary(prog);

    let mut seed = |attr: LeftByteAttr| seed_start(prog, attr, needs_attr_split, flags, &mut pool);
    let start = seed(LeftByteAttr::TextStart);
    let start_mid_word = seed(LeftByteAttr::Word);
    let start_mid_nonword = seed(LeftByteAttr::NonWord);
    // A poisoned table is discarded by every consumer, so nothing
    // past the first unserveable intern is worth building: the
    // strings-property patterns (`\p{RGI_Emoji}`, ~10k states) poison
    // on their very first state.
    if pool.poisoned {
        return finish_dfa(pool.states, start, start_mid_word, start_mid_nonword, true);
    }

    // Scratch for the 256-byte loop: the right-byte closure, the
    // byte-step output and the merged seeds, each allocated once.
    let mut closed_with_right = SparseSet::new(prog.len());
    let mut closure_work: Vec<usize> = Vec::new();
    let mut ready_from_step: PcList = PcList::new();
    let mut deferred_from_step: [PcList; 3] = Default::default();
    let mut merged_seeds: PcList = PcList::new();
    'bfs: while let Some((cur_ready, cur_deferred, cur_attr, cur_idx)) = pool.work.pop() {
        let mut transitions = [0u32; 256];
        let mut accept_before_byte = [0u32; 8];
        let ctx_at_cursor = ctx_for(cur_attr);
        let cur_seeds = || cur_ready.iter().map(|&pc| pc as usize);
        // The right byte only reaches the closure through WBound /
        // NWBound, so without them one closure per state serves all
        // 256 bytes and the accept-before-byte mask is all or nothing.
        if !needs_attr_split {
            closed_with_right.clear();
            let at_wb = ctx_at_cursor.at_word_boundary(None);
            closure_into(
                prog,
                cur_seeds(),
                ctx_at_cursor,
                Some(at_wb),
                &mut closed_with_right,
                &mut closure_work,
            );
            if pc_set_is_accept(prog, closed_with_right.as_slice()) {
                accept_before_byte = [u32::MAX; 8];
            }
        }
        for byte_u16 in 0u16..=255 {
            let byte = byte_u16 as u8;
            if needs_attr_split {
                // chunk 8.6b — re-close cur_ready with right=byte to
                // resolve WBound/NWBound at the cursor given left=cur_attr
                // and right=class_of(byte). The cur_ready entered the BFS
                // with right=None, so any WBound PCs are present-but-
                // unresolved; this re-closure resolves them per-byte.
                // Closure is idempotent for already-advanced PCs, so the
                // re-run never shrinks the set.
                closed_with_right.clear();
                let at_wb = ctx_at_cursor.at_word_boundary(Some(byte));
                closure_into(
                    prog,
                    cur_seeds(),
                    ctx_at_cursor,
                    Some(at_wb),
                    &mut closed_with_right,
                    &mut closure_work,
                );
                // chunk 8.6b — if the per-byte re-closure reaches Match,
                // record a zero-width accept at this cursor before the
                // byte is stepped (else `byte_step` drops Match since
                // it's not byte-consuming). The executor reads
                // `accept_before_byte` ahead of consuming `hay[i]`.
                if pc_set_is_accept(prog, closed_with_right.as_slice()) {
                    mask_set(&mut accept_before_byte, byte);
                }
            }
            ready_from_step.clear();
            for d in deferred_from_step.iter_mut() {
                d.clear();
            }
            byte_step_into(
                prog,
                closed_with_right.as_slice().iter().map(|&pc| pc as usize),
                byte,
                &mut ready_from_step,
                &mut deferred_from_step,
            );
            let (next_ready, next_deferred) = advance_deferred(
                prog,
                byte,
                &cur_deferred,
                &ready_from_step,
                &deferred_from_step,
                &mut merged_seeds,
                &mut pool,
            );
            let next_attr = attr_of_byte(byte);
            let next_idx = intern_state(
                prog,
                next_ready,
                next_deferred,
                next_attr,
                needs_attr_split,
                flags,
                &mut pool,
            );
            transitions[byte as usize] = next_idx;
            if pool.poisoned {
                break 'bfs;
            }
        }
        pool.states[cur_idx as usize].transitions = transitions;
        // The executor reads this mask to answer "is there a zero-width
        // accept sitting at this cursor, before the byte is stepped".
        // On a state that ALREADY accepts the answer is yes for every
        // byte — and `last_accept` is already this cursor (the step
        // that arrived here wrote it, or the pre-loop seed did), so
        // every one of those 256 bits would only re-store the value
        // that is there. Writing them anyway left
        // `any_accept_before_byte` true for every pattern with a
        // reachable accepting state, i.e. all of them, so attack #R-E's
        // hoist has never once fired. Only non-accepting states carry a
        // mask the executor cannot already infer.
        if !pool.states[cur_idx as usize].is_accept {
            pool.states[cur_idx as usize].accept_before_byte = accept_before_byte;
        }
    }

    finish_dfa(
        pool.states,
        start,
        start_mid_word,
        start_mid_nonword,
        pool.poisoned,
    )
}

/// Seed one BFS start state: ε-close PC 0 under `attr`'s cursor ctx
/// (chunk 8.6b — `closure_with_ctx`, WBound terminal, not
/// `closure_full(.., None)`; see the closure-API role split note at
/// the top of dfa/mod.rs) and intern. The three start entries
/// (text-start / mid-word / mid-nonword) differ only in attr.
fn seed_start(
    prog: &Program,
    attr: LeftByteAttr,
    needs_attr_split: bool,
    flags: u8,
    pool: &mut Pool,
) -> u32 {
    let initial = pool.closure_sorted(prog, &[0], ctx_for(attr));
    intern_state(
        prog,
        initial,
        Default::default(),
        attr,
        needs_attr_split,
        flags,
        pool,
    )
}

/// chunk 10b — current deferred PCs transition based on the byte's
/// UTF-8 role. Continuation byte 0x80..0xBF decrements every active
/// u_skip by 1 (so u_skip=1 entries promote into the next ready
/// seed); any other byte breaks an in-flight multi-byte sequence and
/// kills the deferred PCs (matches the NFA: an incomplete UTF-8
/// sequence never commits). The promoted PCs merge with the byte-
/// step's fresh ready set and ε-close under the post-consume ctx.
fn advance_deferred(
    prog: &Program,
    byte: u8,
    cur_deferred: &[PcList; 3],
    ready_from_step: &[u32],
    deferred_from_step: &[PcList; 3],
    merged_seeds: &mut PcList,
    pool: &mut Pool,
) -> (PcList, [PcList; 3]) {
    let is_continuation = (0x80..=0xBF).contains(&byte);
    let mut next_deferred: [PcList; 3] = if is_continuation {
        [
            cur_deferred[1].clone(),
            cur_deferred[2].clone(),
            PcList::new(),
        ]
    } else {
        Default::default()
    };
    // Merge the byte-step's fresh deferred PCs into the carry-
    // forward array; the lists stay sorted and duplicate-free.
    for i in 0..3 {
        if !deferred_from_step[i].is_empty() {
            next_deferred[i].extend_from_slice(&deferred_from_step[i]);
            next_deferred[i].sort_unstable();
            next_deferred[i].dedup();
        }
    }
    merged_seeds.clear();
    merged_seeds.extend_from_slice(ready_from_step);
    if is_continuation {
        merged_seeds.extend_from_slice(&cur_deferred[0]);
    }
    let next_ready = if merged_seeds.is_empty() {
        PcList::new()
    } else {
        // Post-step ctx (cursor k+1): left = the byte we just
        // consumed. Mid-pattern `Op::AnchorB` re-fires after `\n`
        // when the instruction's baked m-bit is set (per-inst
        // multiline, regexp-modifiers). WBound stays unresolved
        // here — the closure leaves it terminal so the next BFS
        // step can resolve it under the upcoming right_byte.
        let ctx_after = PositionCtx {
            left_byte: Some(byte),
            is_text_start: false,
            is_text_end: false,
        };
        pool.closure_sorted(prog, merged_seeds, ctx_after)
    };
    (next_ready, next_deferred)
}
