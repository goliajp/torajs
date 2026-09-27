use torajs_core::ssa::Function;

use crate::egraph::Egraph;
use crate::elaborate::elaborate;
use crate::optimize::remove_pure_and_optimize;
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

/// End-to-end egraph pass over one SSA `Function`. Stitches together
/// `optimize::remove_pure_and_optimize` (Phase 1 of the algorithm —
/// canonicalize + GVN, no rule fires in Phase 0) and `elaborate::elaborate`
/// (Phase 2 — scope-tracked re-emission).
///
/// Phase 0 semantics: with no rule cluster active, this is identity —
/// the returned `Function` is round-trip-equivalent to the input.
/// Phase 1+ wires `try_rewrite` into optimize and flips this from
/// identity to a real optimizer.
pub struct EgraphPass<'a> {
    func: &'a Function,
}

impl<'a> EgraphPass<'a> {
    /// Build a pass bound to `func`. No work happens here; `run`
    /// performs the full three-phase walk.
    pub fn new(func: &'a Function) -> Self {
        Self { func }
    }

    /// Run the full pipeline, returning the rewritten function.
    /// Declarations (`is_declaration() == true`, i.e. extern intrinsics
    /// with no body) are returned unchanged — they have no blocks for
    /// the egraph to mutate.
    pub fn run(self) -> Function {
        if self.func.is_declaration() {
            return self.func.clone();
        }
        let dom = DominatorTree::compute(self.func);
        let loop_info = LoopAnalysis::compute(self.func, &dom);
        let mut egraph = Egraph::new(self.func.values.len());
        let _opt_stats = remove_pure_and_optimize(self.func, &dom, &mut egraph);
        let (out, _elab_stats) = elaborate(self.func, &dom, &loop_info, &mut egraph);
        out
    }
}
