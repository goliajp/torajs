//! One transition of the subset construction: from a state's item and
//! a byte, the target state (interned) and whether a zero-width accept
//! sits before the byte. The eager build runs it for all 256 bytes of
//! a row; the lazily filled table runs it for one `(state, byte)`.

use alloc::vec::Vec;

use crate::dfa::build_helpers::{attr_of_byte, ctx_for, pc_set_is_accept};
use crate::dfa::ctx::{PositionCtx, closure_into};
use crate::dfa::intern::intern_state;
use crate::dfa::pool::{PcList, Pool, StateItem};
use crate::dfa::sparse::SparseSet;
use crate::dfa::step::byte_step_into;
use crate::program::Program;

/// Scratch for expanding transitions, allocated once per builder: the
/// right-byte closure, the byte-step output and the merged seeds.
pub(super) struct Expander {
    closed_with_right: SparseSet,
    closure_work: Vec<usize>,
    ready_from_step: PcList,
    deferred_from_step: [PcList; 3],
    merged_seeds: PcList,
    /// Without WBound / NWBound: whether the state's one closure
    /// reaches Match, i.e. a zero-width accept before every byte.
    whole_accept: bool,
}

impl Expander {
    pub(super) fn new(prog: &Program) -> Self {
        Self {
            closed_with_right: SparseSet::new(prog.len()),
            closure_work: Vec::new(),
            ready_from_step: PcList::new(),
            deferred_from_step: Default::default(),
            merged_seeds: PcList::new(),
            whole_accept: false,
        }
    }

    /// Per-state work every byte of the row shares. The right byte only
    /// reaches the closure through WBound / NWBound, so without them
    /// one closure serves all 256 bytes and the accept-before-byte mask
    /// is all or nothing. Must precede [`Self::step`] for `item`.
    pub(super) fn prepare(&mut self, prog: &Program, item: &StateItem, split: bool) {
        if split {
            return;
        }
        let ctx = ctx_for(item.attr);
        self.close(prog, item, ctx, ctx.at_word_boundary(None));
        self.whole_accept = pc_set_is_accept(prog, self.closed_with_right.as_slice());
    }

    fn close(&mut self, prog: &Program, item: &StateItem, ctx: PositionCtx, at_wb: bool) {
        self.closed_with_right.clear();
        closure_into(
            prog,
            item.ready.iter().map(|&pc| pc as usize),
            ctx,
            Some(at_wb),
            &mut self.closed_with_right,
            &mut self.closure_work,
        );
    }

    /// The transition on `byte` out of the state `item` describes:
    /// `(target, accept_before_byte)`. Interns the target, which may
    /// set `pool.poisoned`.
    pub(super) fn step(
        &mut self,
        prog: &Program,
        item: &StateItem,
        byte: u8,
        split: bool,
        flags: u8,
        pool: &mut Pool,
    ) -> (u32, bool) {
        let mut accept_before = self.whole_accept;
        if split {
            // chunk 8.6b — re-close with right = byte to resolve
            // WBound / NWBound at the cursor given left = attr. If
            // that reaches Match, the accept is zero-width at this
            // cursor before the byte is stepped (`byte_step` drops
            // Match since it consumes nothing).
            let ctx = ctx_for(item.attr);
            self.close(prog, item, ctx, ctx.at_word_boundary(Some(byte)));
            accept_before = pc_set_is_accept(prog, self.closed_with_right.as_slice());
        }
        self.ready_from_step.clear();
        for d in self.deferred_from_step.iter_mut() {
            d.clear();
        }
        byte_step_into(
            prog,
            self.closed_with_right
                .as_slice()
                .iter()
                .map(|&pc| pc as usize),
            byte,
            &mut self.ready_from_step,
            &mut self.deferred_from_step,
        );
        let (next_ready, next_deferred) = advance_deferred(
            prog,
            byte,
            &item.deferred,
            &self.ready_from_step,
            &self.deferred_from_step,
            &mut self.merged_seeds,
            pool,
        );
        let next = intern_state(
            prog,
            next_ready,
            next_deferred,
            attr_of_byte(byte),
            split,
            flags,
            pool,
        );
        (next, accept_before)
    }
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
        // Post-step ctx (cursor k+1): left = the byte just consumed.
        // Mid-pattern `Op::AnchorB` re-fires after `\n` when the
        // instruction's baked m-bit is set (per-inst multiline,
        // regexp-modifiers). WBound stays unresolved here — the
        // closure leaves it terminal so the next step can resolve it
        // under the upcoming right byte.
        let ctx_after = PositionCtx {
            left_byte: Some(byte),
            is_text_start: false,
            is_text_end: false,
        };
        pool.closure_sorted(prog, merged_seeds, ctx_after)
    };
    (next_ready, next_deferred)
}
