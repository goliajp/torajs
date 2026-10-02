//! State interning for the subset construction: dedup a `(ready,
//! deferred, attr)` triple against the pool, or reserve a new state
//! for it and queue its row for expansion. Shared by the eager build
//! ([`super::build`]) and the lazily filled table ([`super::lazy`]).

use crate::dfa::build_helpers::{LeftByteAttr, ctx_for, pc_set_is_accept, pc_set_is_accept_at_end};
use crate::dfa::pending_class::{KPropertyVerdict, PendingClass, classify_kproperty_shape};
use crate::dfa::pool::{PcList, Pool, StateItem, flatten_key};
use crate::dfa::search::DfaState;
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
    item: &StateItem,
    needs_attr_split: bool,
    flags: u8,
    pool: &mut Pool,
) -> PendingClass {
    let shape = match classify_kproperty_shape(prog, &item.ready, &item.deferred, flags) {
        KPropertyVerdict::NoKProperty => return PendingClass::INERT,
        KPropertyVerdict::Unserveable => {
            // RFC 20260711 chunk B — K-PROPERTY present but not
            // serveable by the single-class pending slot. The whole
            // table is marked poisoned: every consumer drops it and
            // the cp-aware Pike VM serves the program, so the builder
            // stops at the first poisoned intern.
            pool.poisoned = true;
            return PendingClass::INERT;
        }
        KPropertyVerdict::Shape(s) => s,
    };
    // yes_target = ε-closure(union of pc + 1 per K-PROPERTY pc),
    // interned recursively; the recursion may push further states.
    let yes_seeds: PcList = shape.yes_seeds.iter().map(|&pc| pc as u32).collect();
    let post_ready = pool.closure_sorted(prog, &yes_seeds, ctx_for(item.attr));
    let yes_target = intern_state(
        prog,
        post_ready,
        Default::default(),
        item.attr,
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

/// The row a freshly interned state starts from: no transitions, no
/// zero-width accepts, the accept flags of its own PC set.
fn fresh_row(prog: &Program, ready: &[u32], attr: LeftByteAttr, pool: &mut Pool) -> DfaState {
    DfaState {
        transitions: [0u32; 256],
        is_accept: pc_set_is_accept(prog, ready),
        is_accept_at_end: pc_set_is_accept_at_end(
            prog,
            ready,
            attr,
            &mut pool.closed,
            &mut pool.closure_work,
        ),
        // Round 3 Phase B sub-batch 6 attack #R-G — patched once every
        // row is known (`compute_monotone_accept`); `false` keeps the
        // executor's monotone fast path off until then, and for good
        // on a lazily filled table.
        monotone_accept: false,
        _pad: 0,
        accept_before_byte: [0u32; 8],
        // patched by `intern_state` once the recursive yes_target
        // intern returns
        pending_class: PendingClass::INERT,
    }
}

/// Look up `(ready, deferred, attr)` in the interning map; insert a
/// new state and queue its row if absent. States with empty ready and
/// empty deferred map to state 0 (dead). The attr (chunk 8.6b) is part
/// of the dedup key so states reached via different left-byte classes
/// don't collapse — they resolve WBound differently on outgoing
/// transitions. The deferred array (chunk 10b) lets u-flag `.` park
/// PCs behind the UTF-8 multi-byte tail without conflating them with
/// ready PCs at the same cursor.
pub(super) fn intern_state(
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
    // chunk 8.6b dedup — without `Op::WBound` / `Op::NWBound` the attr
    // never affects an outgoing transition, so all three attrs fold
    // onto one physical state and the state count matches the
    // pre-8.6b shape.
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
    // Reserve the slot BEFORE computing pending_class so the recursive
    // intern of yes_target can dedup against this very state: the
    // `/\p{L}+/u` loop-body state {3, 1, 2, 4} has yes_target =
    // ε-closure({3}) = itself, and the dedup hit ends that cycle.
    let i = pool.states.len() as u32;
    let row = fresh_row(prog, &ready, attr, pool);
    pool.states.push(row);
    pool.keys.push(pool.key_scratch.clone());
    pool.items.push(StateItem::empty());
    pool.set_to_idx.insert_at(slot, i);
    let item = StateItem {
        ready,
        deferred,
        attr,
    };
    // Round 3 Phase B sub-batch 4 attack #R-J v3 — a ready set that is
    // a singleton K-PROPERTY pc, or one whose byte consumers are all
    // K-PROPERTY with the same class_idx (the `+` / `*` loop body),
    // carries a pending class for the non-ASCII code points the byte
    // step cannot route.
    let pending_class = compute_pending_class_for(prog, &item, needs_attr_split, flags, pool);
    pool.states[i as usize].pending_class = pending_class;
    // v4 (regex-024) — the row is always expanded, pending class or
    // not: the executor consults `transitions[byte]` first and only
    // falls back to the pending class where the row is dead, so a
    // mixed state (`(\p{L})(\d+)(\p{L})/u` after the digit loop) keeps
    // its digit and ASCII-letter routes in the row.
    pool.items[i as usize] = item;
    pool.work.push(i);
    i
}
