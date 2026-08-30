//! LICM — the second walk of the egraph's first phase.
//!
//! Split from [`crate::optimize`] (rotation 537) when that file's
//! production half crossed the 500-line limit. The parent answers
//! "is this instruction redundant here" for one instruction at a
//! time, in dominator order; this answers "may this instruction
//! leave the loop it is in", which needs the loop nest rather than
//! the dominator walk and runs as its own pass afterwards. The two
//! share only `OptimizeStats` and the egraph they both write
//! `set_available_block` into.

use crate::dominator::DominatorTree;
use crate::egraph::Egraph;
use crate::loop_analysis::{LoopAnalysis, NaturalLoop};
use crate::optimize::{OptimizeStats, is_pure};
use torajs_core::ssa::{Function, InstKind, Operand};

/// LICM (loop-invariant code motion) second pass — walk each natural
/// loop innermost-first and mark every pure, loop-invariant instruction
/// for hoisting into the loop's preheader slot. Phase 1 records the
/// decision on the e-class via `set_available_block(canon, target)`;
/// the elaboration phase translates that into the actual emit-block
/// choice when rebuilding the function.
///
/// The hoist target is `idom(loop.header)` — in a reducible CFG this
/// is the natural preheader slot: it strictly dominates the loop body,
/// branches into the header (possibly via a shared chain), and is not
/// itself in the loop body. We do not synthesise a physical preheader
/// block; the canonical instruction simply joins the existing idom
/// block's tail (before its terminator). Subsequent passes that need a
/// dedicated preheader can run a CFG-restructuring step ahead of this.
///
/// Innermost-first iteration (`body.len()` ASC since outer body ⊇
/// inner body in a reducible CFG) gives single-pass iterative LICM:
/// once an instruction is hoisted out of its innermost containing
/// loop via `set_available_block`, its `available_block` lookup falls
/// outside that loop, so the next outer loop sees it as invariant and
/// hoists again. Two-layer nests resolve in one pass; deeper nests
/// resolve in nesting-depth passes — Phase 1 ships single-pass and
/// leaves nested re-iteration to a polish revisit.
pub(crate) fn licm_hoist(
    func: &Function,
    _dom: &DominatorTree,
    loop_info: &LoopAnalysis,
    egraph: &mut Egraph,
    stats: &mut OptimizeStats,
) {
    if loop_info.loops().is_empty() {
        return;
    }
    let mut loops: Vec<&NaturalLoop> = loop_info.loops().iter().collect();
    // Smaller body = inner (outer body ⊇ inner body in reducible CFG).
    loops.sort_by_key(|l| l.body.len());

    for lp in loops {
        let Some(target) = _dom.immediate_dominator(lp.header) else {
            // Loop whose header is the entry block (or unreachable) has
            // no preheader candidate — skip. Real-world JS rarely lands
            // here; the function always has an entry-block prologue.
            continue;
        };
        // Defensive: if the idom is itself in the loop body (irreducible
        // CFG edge), abandon this loop — hoisting into the body is no
        // gain. Reducible CFG (the common case for compiled JS) never
        // hits this branch.
        if lp.body.iter().any(|&b| b == target) {
            continue;
        }

        for &body_block in &lp.body {
            let bi = body_block.0 as usize;
            if bi >= func.blocks.len() {
                continue;
            }
            for inst in &func.blocks[bi].insts {
                let Some(result) = inst.result else {
                    continue;
                };
                if !is_pure(&inst.kind) {
                    continue;
                }
                let canon = egraph.opt_value(result);
                // Skip if the canon's emit slot is already outside this
                // loop — earlier inner-loop pass already hoisted it, or
                // it was never inside (canonical representative emitted
                // in a dominator block by GVN-redundancy collapse).
                match egraph.available_block(canon) {
                    Some(b) if lp.body.iter().any(|&x| x == b) => {}
                    _ => continue,
                }
                // Canonicalise operands through opt_value before
                // checking invariance — an operand may itself have
                // been GVN-unioned with a value defined outside the
                // loop, and the textbook invariant check operates on
                // the canonical representative.
                if !operands_loop_invariant(&inst.kind, lp, egraph, &func.params) {
                    continue;
                }
                // Mark for hoist. set_available_block overrides the
                // GVN-inserted source-block entry; elaboration consumes
                // the new target when rebuilding the function.
                egraph.set_available_block(canon, target);
                stats.licm_hoisted += 1;
            }
        }
    }
}

/// True iff every SSA Value operand of `kind` resolves (via opt_value)
/// to a block that lies outside `lp.body`. Constants are trivially
/// loop-invariant. A value with `available_block == None` is treated as
/// "defined before any block" (function parameter / entry-block prologue
/// before the optimiser has set its location) and is also invariant.
///
/// Variants without value operands (Alloca, GlobalRef, etc.) are not
/// LICM candidates — they are skeleton or have intrinsic block affinity;
/// `is_pure` already filters them out before this check fires.
fn operands_loop_invariant(
    kind: &InstKind,
    lp: &NaturalLoop,
    egraph: &mut Egraph,
    params: &[torajs_core::ssa::ValueId],
) -> bool {
    let mut check = |op: &Operand| -> bool {
        match op {
            Operand::Value(v) => {
                let canon = egraph.opt_value(*v);
                match egraph.available_block(canon) {
                    // None means the value was never GVN-inserted: it's
                    // either a function parameter (always defined before
                    // any loop body — invariant) or a skeleton (Call /
                    // Store / Alloca) result emitted somewhere we don't
                    // track. Only the parameter case is provably outside
                    // the loop; treat skeleton results conservatively as
                    // non-invariant.
                    None => params.iter().any(|p| *p == canon),
                    Some(b) => !lp.body.iter().any(|&x| x == b),
                }
            }
            // Constants (ConstI64, ConstBool, ConstF64, ConstPtrNull,
            // …) are invariant by construction.
            _ => true,
        }
    };
    match kind {
        InstKind::BinOp(_, l, r) => check(l) && check(r),
        InstKind::ICmp(_, l, r) => check(l) && check(r),
        InstKind::FCmp(_, l, r) => check(l) && check(r),
        InstKind::SiToFp(v)
        | InstKind::FpToSi(v)
        | InstKind::ZExtBoolToI64(v)
        | InstKind::ZExtI32ToI64(v)
        | InstKind::Neg(v)
        | InstKind::Ctpop(v)
        | InstKind::Identity(v) => check(v),
        // Load is filtered out by is_pure; including it here would be
        // unsafe without alias analysis (a Store inside the loop body
        // may invalidate the loaded location). Phase 1 conservatively
        // refuses to LICM-hoist any Load.
        _ => false,
    }
}
