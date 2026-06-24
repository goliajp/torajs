//! Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E) + v3 option A
//! — `PendingClass` struct + the build-side predicates that decide
//! which `DfaState`s carry an active K-PROPERTY cp-step handler.
//!
//! The struct itself is read by the executor (`dfa/search.rs`'s
//! `dfa_search_from`) and written by the BFS interner
//! (`dfa/build.rs::compute_pending_class_for`); the executor short-
//! circuits the 256-way `transitions[byte]` lookup whenever
//! `pending_class.active != 0`, decodes one UTF-8 cp at the cursor,
//! calls `prog.classes[class_idx].test_cp(cp)`, then branches to
//! `yes_target` (matched) or `no_target` (not matched / invalid UTF-8).
//!
//! Split out of `dfa/search.rs` + `dfa/build.rs` to keep both parent
//! files under the 500-line HARD limit; the struct + the two
//! lightweight build-side predicates live here so the substrate is
//! one logical surface.

use crate::program::{Op, Program};
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

/// Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E) — fixed-size
/// per-state K-PROPERTY cp-step record. Lives inside
/// [`super::search::DfaState`] so the `#[repr(C)]` slice-bake
/// invariant stays intact and the `INNER_DFA_STATE_SIZE` constant in
/// `user_regex_baked_layout.rs` captures the full per-state byte
/// count.
///
/// Layout (12 bytes, align 4):
///
/// | offset | size | field |
/// | ------ | ---- | ----- |
/// | 0      | 1    | active     |
/// | 1      | 1    | _pad       |
/// | 2      | 2    | class_idx  |
/// | 4      | 4    | yes_target |
/// | 8      | 4    | no_target  |
///
/// Default = [`PendingClass::INERT`] = all-zero = `active == 0` = the
/// executor treats this state as a normal 256-way dispatch state. K-
/// PROPERTY pending states populate `active == 1` + the triple at BFS
/// time (`dfa/build.rs::compute_pending_class_for`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingClass {
    /// 0 = inert (no K-PROPERTY handler at this state); non-zero =
    /// active, executor must consult `class_idx` + `yes_target` +
    /// `no_target` and ignore `transitions[byte]`.
    pub active: u8,
    /// Explicit 1-byte padding so `class_idx` lands deterministically
    /// at offset 2. Reads zero in the baked image; declaring it in the
    /// struct (rather than relying on Rust's struct-padding inference)
    /// makes the AOT bake byte sequence unambiguous.
    pub _pad: u8,
    /// Index into `prog.classes` for the `test_cp(cp)` call. `u16`
    /// matches `Inst.a`'s semantic class-count bound (well under 64K
    /// in production) and fits the 2-byte slot at offset 2 without
    /// extra padding.
    pub class_idx: u16,
    /// DFA state index to transition to on `test_cp(cp) == true`. Width
    /// matches `DfaState::transitions: [u32; 256]`.
    pub yes_target: u32,
    /// DFA state index to transition to on `test_cp(cp) == false` OR on
    /// invalid UTF-8 (bad lead / bad continuation / truncated).
    /// Typically `0` (dead) for K-PROPERTY pending states whose only
    /// non-dead successor is the post-class state via `yes_target`.
    pub no_target: u32,
}

impl PendingClass {
    /// All-zero `PendingClass` — `active == 0` means the owning
    /// [`super::search::DfaState`] runs the ordinary 256-way
    /// `transitions[byte]` dispatch. Used by `DfaState::default()` and
    /// the BFS interning path for every state that is NOT a K-PROPERTY
    /// pending state.
    pub const INERT: Self = Self {
        active: 0,
        _pad: 0,
        class_idx: 0,
        yes_target: 0,
        no_target: 0,
    };
}

/// Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E) — true iff `pc`
/// is a K-PROPERTY `Op::Class` instruction under u-flag. K-PROPERTY =
/// `cc.u_props != 0 && !cc.negate && cc.bits[16..32] all zero` (see
/// [`crate::charclass::CharClass::is_uflag_property_only`]). Used by
/// `dfa/build.rs` BFS to emit a pending state when this PC is among
/// the byte-consuming ops in the closure.
pub(crate) fn kproperty_pc_for(prog: &Program, pc: usize, flags: u8) -> Option<usize> {
    if flags & crate::parser::RE_FLAG_U == 0 {
        return None;
    }
    let ins = prog.insts.get(pc)?;
    if Op::from_u8(ins.op) != Some(Op::Class) {
        return None;
    }
    let cls = prog.classes.get(ins.a as usize)?;
    if cls.is_uflag_property_only() {
        Some(ins.a as usize)
    } else {
        None
    }
}

/// Round 3 Phase B sub-batch 4 attack #R-J v3 option A — analysis of a
/// `(ready, deferred)` PC set to decide whether it is "K-PROPERTY
/// shaped" for the pending_class mechanism. Pure predicate — no
/// state interning, no recursion — so the BFS-side caller in
/// `dfa/build.rs::compute_pending_class_for` can take this verdict
/// and decide whether to recursively intern the yes_target.
///
/// Returns `Some(KPropertyShape)` iff:
/// 1. `deferred[0..3]` are all empty.
/// 2. At least one PC in `ready` is K-PROPERTY ([`kproperty_pc_for`]).
/// 3. All K-PROPERTY PCs in `ready` share the same `class_idx`.
/// 4. All non-K-PROPERTY PCs in `ready` are ε-only
///    ([`is_epsilon_only_pc`]).
///
/// Returns `None` otherwise → the caller falls back to the ordinary
/// 256-way transitions path.
pub(crate) struct KPropertyShape {
    /// The shared K-PROPERTY class index (the value to bake into
    /// `PendingClass::class_idx`).
    pub class_idx: u16,
    /// The seed PCs for the yes_target ε-closure (i.e. one `pc + 1`
    /// per K-PROPERTY PC the ready set contained). Caller computes
    /// `epsilon_closure_with_ctx(seeds, ctx)` and interns the result
    /// via `intern_state`, then writes the resulting state index into
    /// `PendingClass::yes_target`.
    pub yes_seeds: Vec<usize>,
}

pub(crate) fn classify_kproperty_shape(
    prog: &Program,
    ready: &BTreeSet<usize>,
    deferred: &[BTreeSet<usize>; 3],
    flags: u8,
) -> Option<KPropertyShape> {
    if !deferred.iter().all(|d| d.is_empty()) {
        return None;
    }
    let mut kp_pcs: Vec<usize> = Vec::new();
    let mut kp_class_idx: Option<usize> = None;
    for &pc in ready.iter() {
        if let Some(cidx) = kproperty_pc_for(prog, pc, flags) {
            match kp_class_idx {
                None => kp_class_idx = Some(cidx),
                Some(existing) if existing != cidx => {
                    // Multi-class K-PROPERTY disjunction (e.g.
                    // `\p{L}|\p{N}` in a Split) — out of v3-A scope;
                    // caller falls back to normal byte-step.
                    return None;
                }
                _ => {}
            }
            kp_pcs.push(pc);
        }
    }
    let class_idx = kp_class_idx?;
    // v4 (regex-024 regression fix) — non-K-PROPERTY byte-consumers in
    // the ready set are NOT a disqualifier any more. The DFA build now
    // enqueues "mixed" states for ordinary byte_step (so transitions[]
    // populates for the non-K-PROPERTY byte path, e.g. `\d+` digits in
    // `(\p{L})(\d+)(\p{L})/u`'s middle state) AND records the K-
    // PROPERTY pending_class triple. At executor time the
    // pending_class handler is consulted ONLY when transitions[byte]
    // would have routed to dead (state 0) — i.e. for bytes the
    // ordinary byte_step couldn't consume but a K-PROPERTY cp can.
    //
    // Pre-v4 (v3-A) this gate rejected multi-PC sets where a non-K-
    // PROPERTY byte-consumer coexisted with K-PROPERTY (the "post-
    // digit-loop" state of `(\p{L})(\d+)(\p{L})/u` had {Split, Class[d],
    // Match, Class[\p{L}]} multi-PC ready set, gate fired, no pending
    // emitted, transitions[0xCE] = 0 → DFA fails on non-ASCII letter
    // input). The v4 fallback-style pending closes that case while
    // keeping the v3-A loop-body (`\p{L}+/u`) state unchanged
    // (transitions stays all-zero there because no non-K-PROPERTY
    // byte-consumer participates in byte_step; the pending handler
    // takes every byte).
    let yes_seeds: Vec<usize> = kp_pcs.iter().map(|&pc| pc + 1).collect();
    Some(KPropertyShape {
        class_idx: class_idx as u16,
        yes_seeds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    /// Round 3 Phase B sub-batch 4 attack #R-J v2 — `PendingClass` is
    /// baked into each `DfaState`'s tail 12 bytes by the AOT pipeline
    /// (ssa_lower); the layout-lock test guards against silent re-order
    /// / type-change of the 5 sub-fields so the byte image stays
    /// portable. See the struct doc for the offset table.
    #[test]
    fn pending_class_layout_locked() {
        assert_eq!(offset_of!(PendingClass, active), 0);
        assert_eq!(offset_of!(PendingClass, _pad), 1);
        assert_eq!(offset_of!(PendingClass, class_idx), 2);
        assert_eq!(offset_of!(PendingClass, yes_target), 4);
        assert_eq!(offset_of!(PendingClass, no_target), 8);
        assert_eq!(size_of::<PendingClass>(), 12);
        assert_eq!(align_of::<PendingClass>(), 4);
    }

    #[test]
    fn inert_is_all_zero() {
        let i = PendingClass::INERT;
        assert_eq!(i.active, 0);
        assert_eq!(i._pad, 0);
        assert_eq!(i.class_idx, 0);
        assert_eq!(i.yes_target, 0);
        assert_eq!(i.no_target, 0);
    }
}
