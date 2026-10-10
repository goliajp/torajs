//! The inliner's emit pass for one caller: splice every site the
//! classifier approved, then drop the out-of-line bodies of sole-site
//! callees whose one call is gone.

use super::{
    InlineBudget, InlineDecision, InlineTarget, InlinerStats, SpliceError, callee_body_cost,
    inline_multi_block_leaf, inline_single_block_leaf, sole_site,
};
use torajs_core::ssa::{Function, InstKind, Module};

/// Splice `decisions`' `Inline` sites into `module.funcs[caller_idx]`.
/// Self-recursive sites splice from `self_snapshot`, the caller's body
/// before its first splice; it is taken here when it is still `None`
/// and some approved site is a self-call.
pub(super) fn emit_caller(
    module: &mut Module,
    caller_idx: usize,
    decisions: &[(usize, usize, InlineDecision)],
    self_snapshot: &mut Option<Function>,
    budget: &InlineBudget,
    sole: &[bool],
    stats: &mut InlinerStats,
) {
    // Pass B — collect Inline targets with full splice payload.
    let mut targets: Vec<InlineTarget> = Vec::new();
    for (blk_idx, site_idx, dec) in decisions {
        if matches!(dec, InlineDecision::Inline { .. }) {
            let inst = &module.funcs[caller_idx].blocks[*blk_idx].insts[*site_idx];
            if let InstKind::Call(callee_id, args) = &inst.kind {
                targets.push((*blk_idx, *site_idx, *callee_id, args.clone(), inst.result));
            }
        }
    }
    // Reverse-sort so deeper sites splice first; lower-index
    // targets keep their captured `site_idx` valid.
    targets.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    // Self-recursive sites splice from a snapshot taken before any
    // splice in this caller fires, so every recursive site inlines
    // the ORIGINAL body (the GCC recursive-inlining shape). Without
    // this, the second self-site would clone a body already grown
    // by the first splice — compounding growth within one pass.
    let mut spliced_sole: Vec<usize> = Vec::new();
    if self_snapshot.is_none()
        && targets
            .iter()
            .any(|(_, _, cid, _, _)| cid.0 as usize == caller_idx)
    {
        *self_snapshot = Some(module.funcs[caller_idx].clone());
    }
    for (blk_idx, site_idx, callee_id, args, result_value) in targets {
        let callee_clone;
        let callee_ref: &Function = if callee_id.0 as usize == caller_idx {
            self_snapshot
                .as_ref()
                .expect("snapshot taken when any self target exists")
        } else {
            callee_clone = module.funcs[callee_id.0 as usize].clone();
            &callee_clone
        };
        // Stale-cost guard: the classifier priced this callee from
        // the costs precomputed at pass start, but a splice that
        // already fired in this same pass (a callee processed
        // earlier as a caller) may have grown the body since.
        // Re-price the live clone and skip rather than splice an
        // oversized body.
        let ceiling = if sole[callee_id.0 as usize] {
            budget.sole_site_ceiling
        } else {
            budget.callee_cost_ceiling
        };
        if callee_body_cost(callee_ref) > ceiling {
            stats.skipped_live_cost += 1;
            continue;
        }
        let caller = &mut module.funcs[caller_idx];
        // Phase 1.0a fast path: single-block leaf. If the callee's
        // shape is outside that envelope (multi-block / branchy
        // body), fall through to Phase 2.0a multi-block splice.
        match inline_single_block_leaf(caller, blk_idx, site_idx, callee_ref, &args, result_value) {
            Ok(()) => {
                stats.inlined += 1;
                stats.inlined_single_block += 1;
                spliced_sole.push(callee_id.0 as usize);
            }
            Err(SpliceError::NotSingleBlock) => {
                if inline_multi_block_leaf(
                    caller,
                    blk_idx,
                    site_idx,
                    callee_ref,
                    &args,
                    result_value,
                )
                .is_ok()
                {
                    stats.inlined += 1;
                    stats.inlined_multi_block += 1;
                    spliced_sole.push(callee_id.0 as usize);
                }
            }
            Err(_) => {}
        }
    }
    // A sole-site callee's one call is gone: its out-of-line body
    // is unreachable, unless a splice into it made it call itself.
    // A user function (no `__` prefix) is named by nothing but SSA
    // instructions, so its body goes; a compiler-minted one may sit
    // in a metadata table by fid.
    for callee in spliced_sole {
        if callee == caller_idx
            || !sole[callee]
            || module.funcs[callee].name.starts_with("__")
            || sole_site::still_referenced(module, callee)
        {
            continue;
        }
        module.funcs[callee].blocks.clear();
        stats.sole_bodies_dropped += 1;
    }
}
