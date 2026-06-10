//! torajs-egraph — Cranelift-form acyclic e-graph mid-end optimizer.
//!
//! Three-phase pipeline (RFC 20260609-torajs-codegen-optimizer §2.3, ported
//! from `wasmtime/cranelift/codegen/src/egraph/`):
//!
//! 1. **Canonicalize + GVN**: dominator-preorder walk, scoped GVN map,
//!    eager rewrite via ISLE-inspired rule macros. Pure operators float
//!    out of the layout into an implicit egraph (SSA value space reused +
//!    surgical union-node spines).
//! 2. **Rewrite**: rule fires create `Op::Union(orig, rewritten)` chains
//!    capped at `ECLASS_ENODE_LIMIT`. Recursion bounded by `REWRITE_LIMIT`.
//! 3. **Elaborate**: scoped elaboration converts the egraph back to SSA-
//!    canonical form. Scope-stack memoization → GVN / CSE / LICM all fall
//!    out from domtree-aware placement, no separate passes.
//!
//! Phase 0 (this commit and following 3-4 commits) ships substrate
//! scaffold only — no rules, no production wiring. `EgraphPass::run`
//! is identity (round-trip-equivalent) until Phase 1 lands the first
//! rule cluster.

pub mod cost;
pub mod devirt;
pub mod dominator;
pub mod egraph;
pub mod elaborate;
pub mod inliner;
pub mod loop_analysis;
pub mod mem2reg;
pub mod optimize;
pub mod phi_promote;
pub mod rc_peephole;
pub mod rewrite;
pub mod scope_map;
pub mod slot_forward;

use torajs_core::ssa::{Function, Module};

use crate::dominator::DominatorTree;
use crate::egraph::Egraph;
use crate::elaborate::elaborate;
use crate::loop_analysis::LoopAnalysis;
use crate::optimize::remove_pure_and_optimize;

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

/// Apply the Cluster E inliner (Phase 1.0b) followed by `EgraphPass`
/// to every function in a `Module`, returning the transformed module.
///
/// Order matters: the inliner runs **before** the per-function egraph
/// pass so that any `InstKind::Identity` aliases the splice emits to
/// bind callee `Ret(Some(_))` values to the caller's call-result
/// `ValueId` get collapsed by `elaborate.rs`'s existing identity-
/// dropping path.
///
/// Honors two independent env gates (used for bisection when
/// validating which sub-pass is responsible for a regression):
/// * `TORAJS_EGRAPH_OFF=1` — skip both inliner and egraph; return the
///   module unchanged.
/// * `TORAJS_INLINER_OFF=1` — skip only the inliner emit pass
///   (classification stats are still produced but discarded here);
///   `EgraphPass` still runs.
///
/// This is the canonical integration entry point for the `tr build` /
/// `tr run` new-pipeline drivers; they call this between
/// `ssa_lower::lower_with_arity` and `torajs_codegen::compile_function`.
pub fn transform_module(mut module: Module) -> Module {
    if std::env::var("TORAJS_EGRAPH_OFF").as_deref() == Ok("1") {
        return module;
    }
    let inliner_stats = inliner::inline_module(&mut module);
    // TORAJS_INLINER_STATS=1 — dump per-compile inliner decision
    // counters to stderr for fire-rate attribution (which SkipReason
    // bucket holds the production call sites a given corpus exposes).
    if std::env::var("TORAJS_INLINER_STATS").as_deref() == Ok("1") {
        eprintln!("torajs-inliner-stats: {inliner_stats:?}");
    }
    // Indirect-call promotion — after the first inliner round (which
    // exposes fn-pointer slots by splicing fn-typed params into
    // callers), before a second round that inlines the promoted
    // direct calls. `TORAJS_DEVIRT_OFF=1` skips (bisect gate).
    if std::env::var("TORAJS_DEVIRT_OFF").as_deref() != Ok("1") {
        let devirt_stats = devirt::devirtualize_module(&mut module);
        if std::env::var("TORAJS_DEVIRT_STATS").as_deref() == Ok("1") {
            eprintln!("torajs-devirt-stats: {devirt_stats:?}");
        }
        if devirt_stats.rewritten > 0 {
            let round2 = inliner::inline_module(&mut module);
            if std::env::var("TORAJS_INLINER_STATS").as_deref() == Ok("1") {
                eprintln!("torajs-inliner-stats-round2: {round2:?}");
            }
        }
    }
    // Block-local store→load forwarding — clears the alloca+store+load
    // param round-trips the inliner splices into hot loops (mem2reg's
    // degenerate in-block case). After inlining/devirt (the food),
    // before the egraph pass (forwarded constants feed const-fold /
    // GVN). `TORAJS_SLOTFWD_OFF=1` skips (bisect gate).
    if std::env::var("TORAJS_SLOTFWD_OFF").as_deref() != Ok("1") {
        let fwd_stats = slot_forward::forward_slot_loads(&mut module);
        if std::env::var("TORAJS_SLOTFWD_STATS").as_deref() == Ok("1") {
            eprintln!("torajs-slotfwd-stats: {fwd_stats:?}");
        }
    }
    // Cross-block single-def-block slot promotion — the dominance
    // fast path of LLVM mem2reg (param spills whose loads live in
    // branch arms). After slot_forward (in-block round-trips already
    // cleared, slots possibly DSE'd), before the egraph pass.
    // `TORAJS_MEM2REG_OFF=1` skips (bisect gate).
    if std::env::var("TORAJS_MEM2REG_OFF").as_deref() != Ok("1") {
        let m2r_stats = mem2reg::promote_slots(&mut module);
        if std::env::var("TORAJS_MEM2REG_STATS").as_deref() == Ok("1") {
            eprintln!("torajs-mem2reg-stats: {m2r_stats:?}");
        }
    }
    for func in module.funcs.iter_mut() {
        let new_func = EgraphPass::new(func).run();
        *func = new_func;
    }
    // Full mem2reg φ promotion — loop-carried / join-slot cells the
    // earlier passes left behind. AFTER the egraph pass (GVN /
    // elaborate never see the multi-def Copy it emits), before
    // rc_peephole (which treats Copy as rc-transparent).
    // `TORAJS_MEM2REG_PHI_OFF=1` skips (bisect gate).
    if std::env::var("TORAJS_MEM2REG_PHI_OFF").as_deref() != Ok("1") {
        let phi_stats = phi_promote::promote_phi_slots(&mut module);
        if std::env::var("TORAJS_MEM2REG_PHI_STATS").as_deref() == Ok("1") {
            eprintln!("torajs-mem2reg-phi-stats: {phi_stats:?}");
        }
    }
    // TORAJS_SSA_DUMP=1 — pretty-print the post-egraph pre-peephole
    // SSA to stdout. Debug surface for attributing which pass shaped
    // a given inst stream (mirrors TORAJS_INLINER_STATS).
    if std::env::var("TORAJS_SSA_DUMP").as_deref() == Ok("1") {
        module.print();
    }
    // RC elide peephole — after the egraph pass so pure-inst dedup /
    // identity collapse has already tightened the windows between
    // retain/release pairs. `TORAJS_RC_PEEPHOLE_OFF=1` skips (bisect
    // gate, mirrors TORAJS_INLINER_OFF).
    if std::env::var("TORAJS_RC_PEEPHOLE_OFF").as_deref() != Ok("1") {
        let rc_stats = rc_peephole::elide_rc_pairs(&mut module);
        if std::env::var("TORAJS_RC_PEEPHOLE_STATS").as_deref() == Ok("1") {
            eprintln!("torajs-rc-peephole-stats: {rc_stats:?}");
        }
    }
    module
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    fn val_inst(result: ValueId, kind: InstKind) -> Inst {
        Inst {
            result: Some(result),
            kind,
            origin: None,
        }
    }
    fn fixture(values: Vec<ValueInfo>, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values,
            current_origin: None,
        }
    }

    #[test]
    fn pass_is_identity_on_no_rule_function() {
        // Phase 0: with no rewrite rules, EgraphPass::run returns a
        // function with the same inst count, same kinds, same term.
        let values = vec![
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                val_inst(
                    ValueId(0),
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::ConstI64(4)),
                ),
                val_inst(
                    ValueId(1),
                    InstKind::BinOp(BinOp::Sub, Operand::ConstI64(9), Operand::ConstI64(1)),
                ),
            ],
            term: Terminator::Ret(None),
        }];
        let f = fixture(values, blocks);
        let out = EgraphPass::new(&f).run();
        assert_eq!(out.blocks.len(), 1);
        assert_eq!(out.blocks[0].insts.len(), 2);
        assert!(matches!(
            out.blocks[0].insts[0].kind,
            InstKind::BinOp(BinOp::Add, _, _)
        ));
        assert!(matches!(
            out.blocks[0].insts[1].kind,
            InstKind::BinOp(BinOp::Sub, _, _)
        ));
    }

    #[test]
    fn pass_dedups_identical_pure_adds_in_same_block() {
        // Two identical adds → optimize records GVN union; elaborate
        // drops the redundant second one. End-to-end pass-level test.
        // (Use Value+Value operands — both-const operands would
        // const-fold to Identity in chunk 11c, bypassing GVN.)
        let values = vec![
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                val_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                ),
                val_inst(
                    ValueId(3),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                ),
            ],
            term: Terminator::Ret(None),
        }];
        let f = fixture(values, blocks);
        let out = EgraphPass::new(&f).run();
        assert_eq!(
            out.blocks[0].insts.len(),
            1,
            "GVN must drop the redundant second add"
        );
    }

    #[test]
    fn declaration_passes_through_unchanged() {
        // is_declaration() == true (no blocks) → pass is no-op clone.
        let f = fixture(vec![], vec![]);
        assert!(f.is_declaration());
        let out = EgraphPass::new(&f).run();
        assert!(out.is_declaration());
        assert_eq!(out.name, f.name);
    }

    #[test]
    fn env_gate_skips_transform() {
        // TORAJS_EGRAPH_OFF=1 → transform_module returns the input
        // module verbatim (no per-function pass). Use a SAFETY note:
        // env mutation is process-global; this test sets+unsets in a
        // controlled scope. Test runner serializes the env read.
        // SAFETY: we set and then immediately unset; no other thread
        // in the test process should be reading TORAJS_EGRAPH_OFF
        // concurrently.
        unsafe {
            std::env::set_var("TORAJS_EGRAPH_OFF", "1");
        }
        let mut m = torajs_core::ssa::Module::default();
        m.funcs.push(fixture(vec![], vec![]));
        let original_func_count = m.funcs.len();
        let out = transform_module(m);
        assert_eq!(out.funcs.len(), original_func_count);
        unsafe {
            std::env::remove_var("TORAJS_EGRAPH_OFF");
        }
    }
}
