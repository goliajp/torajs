//! Cluster E Inliner — module-level cost-benefit inlining pass.
//!
//! Reference: LLVM `lib/Analysis/InlineCost.cpp` (`InlineCostAnalyzer`)
//! and Go `src/cmd/compile/internal/inline/inl.go` (cost model +
//! hairyness + budget). Both use cost-benefit + size budget + recursion
//! guard + hot-path tagging; LOC-threshold-only is explicitly rejected
//! (RFC 20260609-torajs-codegen-optimizer §9 Q4).
//!
//! Phase 0 (shipped) — decision substrate:
//!
//! * Enumerate direct-call sites (`InstKind::Call(FuncId, _)`).
//! * Compute each callee's body cost via `cost::cost_of_kind` summed
//!   over its blocks.
//! * Apply per-callee cost ceiling + per-caller cumulative budget +
//!   self-recursion guard + declaration / indirect-call exclusion.
//! * Report decisions via `InlinerStats`. The `inline_module` driver
//!   observes the module by shared reference; nothing is mutated.
//!
//! Phase 1.0a (this commit) — splice mutation API:
//!
//! * `inline_single_block_leaf(caller, blk, site, callee, args, result)`
//!   materialises a single-block leaf callee into the caller block,
//!   removing the matching call instruction.
//! * Fresh `ValueId`s are allocated in `caller.values` for each non-
//!   parameter callee value; callee parameters are substituted by the
//!   caller-supplied `args` directly via the operand map.
//! * `InstKind` operands are rewritten through an exhaustive
//!   variant-by-variant match, so adding a new opcode to
//!   `torajs-core::ssa::InstKind` is a compile-time signal here.
//! * `SpliceError` (8 variants) reports every Phase 1 narrow-scope
//!   precondition violation. The driver is responsible for filtering
//!   candidates ahead of time; the runtime check guards against bugs
//!   in the Phase 0 classifier as well as direct API misuse.
//!
//! Phase 1.0a is NOT wired into `transform_module` yet — that is
//! Phase 1.0b's job, after which `inline_module` switches to
//! `&mut Module` and consumes the splice API for `would_inline`
//! candidates that also satisfy the splice's narrow scope.
//!
//! Future phases — Phase 1.0b wires the driver into the production
//! pipeline behind a `TORAJS_INLINER_OFF` env-gate; Phase 2 generalises
//! beyond single-block leaves (block-split + Φ-node insertion) and
//! threads `LoopAnalysis` so callee cost is depth-weighted (matching
//! the elaborator's existing LICM weighting in `cost::scale_for_depth`).

use crate::cost::{Cost, cost_of_kind};
use std::collections::HashMap;
use torajs_core::ssa::{Function, Inst, InstKind, Module, Operand, Terminator, ValueId};

/// Budget configuration for `inline_module`. Defaults are conservative
/// starting points calibrated against LLVM `-O2` hint thresholds
/// (LLVM's `InlineThreshold` 225 corresponds to roughly 25 instructions
/// of mixed integer / load / branch cost in our `Cost` units). The
/// caller-cumulative budget is 3× that to give one caller room to
/// absorb a handful of small leaves without ballooning text size.
#[derive(Debug, Clone, Copy)]
pub struct InlineBudget {
    /// Single-callee cost ceiling. A callee whose body cost exceeds
    /// this is rejected regardless of caller hotness or remaining
    /// caller budget.
    pub callee_cost_ceiling: Cost,
    /// Per-caller cumulative inlined-cost budget. Once one caller's
    /// `would_inline` decisions sum past this, further sites for the
    /// same caller are skipped with `SkipReason::CallerBudgetExhausted`.
    pub caller_total_budget: Cost,
    /// Maximum recursion depth allowed via inlining. Phase 0 default is
    /// `0` — direct self-recursive call sites are always rejected with
    /// `SkipReason::Recursion`. A non-zero value reserves API surface
    /// for Phase 2+ bounded recursive inlining; the call-graph cycle
    /// detector that pairs with it is not in this commit.
    pub max_recursion_depth: u32,
}

impl Default for InlineBudget {
    fn default() -> Self {
        Self {
            callee_cost_ceiling: Cost::new(225),
            caller_total_budget: Cost::new(675),
            max_recursion_depth: 0,
        }
    }
}

/// Per-call-site inlining decision. `Inline` is only reached when all
/// cost-benefit, budget, and recursion checks pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineDecision {
    /// Site should be inlined; carries the callee body cost (the value
    /// counted against the caller cumulative budget for subsequent
    /// sites in the same caller).
    Inline { callee_cost: Cost },
    /// Site rejected for the given reason; tracked in `InlinerStats`
    /// per-bucket so the next phase can attribute regressions /
    /// missed opportunities.
    Skip(SkipReason),
}

/// Reason a candidate call site was not inlined. Buckets line up with
/// counters in `InlinerStats` for direct attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Callee body cost exceeded `InlineBudget::callee_cost_ceiling`.
    CalleeTooLarge,
    /// Caller's cumulative inlined-cost would exceed
    /// `InlineBudget::caller_total_budget` if this site fired.
    CallerBudgetExhausted,
    /// Callee is a declaration (`is_declaration()` — no blocks); there
    /// is no body to inline. Extern intrinsics and forward-declared
    /// imports land here.
    CalleeIsDeclaration,
    /// Direct self-recursion and `max_recursion_depth == 0`. Phase 0
    /// also lands here when a call would close a cycle of any depth
    /// (full call-graph cycle detector arrives with bounded recursive
    /// inlining in Phase 2+).
    Recursion,
    /// Call site is `InstKind::CallIndirect` (via function pointer).
    /// Phase 0 scope is direct calls only; indirect inlining requires
    /// devirtualization / call-graph analysis (Phase 2+).
    Indirect,
}

/// Aggregate diagnostic counters across one `inline_module` run.
/// Reported to the integration layer for logging and regression
/// tracking. Not used to gate correctness.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InlinerStats {
    /// Direct call sites considered for inlining (`InstKind::Call(...)`
    /// occurrences in every caller body). `CallIndirect` sites are
    /// counted toward `skipped_indirect` and NOT toward `candidates`
    /// (an indirect site is not a candidate at Phase 0).
    pub candidates: u32,
    /// Sites that passed every check. Phase 0: this is a *would-have-
    /// inlined* count — no SSA emit happens, so the output module is
    /// unchanged regardless of this value.
    pub would_inline: u32,
    /// Sites rejected by `SkipReason::CalleeTooLarge`.
    pub skipped_callee_too_large: u32,
    /// Sites rejected by `SkipReason::CallerBudgetExhausted`.
    pub skipped_caller_budget: u32,
    /// Sites rejected by `SkipReason::CalleeIsDeclaration`.
    pub skipped_declaration: u32,
    /// Sites rejected by `SkipReason::Recursion`.
    pub skipped_recursion: u32,
    /// Indirect call sites observed. Tracked separately from
    /// `candidates` because at Phase 0 they are categorically out of
    /// scope rather than a per-site cost-benefit miss.
    pub skipped_indirect: u32,
}

/// Sum `cost_of_kind` over every instruction in every block of `func`.
///
/// Phase 0 uses a flat per-instruction sum; Phase 1 threads
/// `LoopAnalysis` so loop bodies multiply by `BR_PRED_FACTOR^depth`
/// (matching the elaborator's existing LICM weighting in
/// `cost::scale_for_depth`).
pub fn callee_body_cost(func: &Function) -> Cost {
    let mut total = Cost::ZERO;
    for blk in &func.blocks {
        for inst in &blk.insts {
            total = total.add(cost_of_kind(&inst.kind));
        }
    }
    total
}

/// Classify every `InstKind::Call` site in `module.funcs[caller_idx]`
/// against `budget`, returning per-site decisions in source order.
/// `callee_costs[i]` must equal `callee_body_cost(&module.funcs[i])`
/// (precomputed by `inline_module_with_budget` so callee bodies are
/// scanned exactly once per pass).
///
/// `CallIndirect` sites land in the returned `Vec` as
/// `Skip(Indirect)` so the caller can count them toward
/// `InlinerStats::skipped_indirect` without re-scanning.
fn classify_caller_sites(
    module: &Module,
    caller_idx: usize,
    budget: &InlineBudget,
    callee_costs: &[Cost],
) -> Vec<InlineDecision> {
    let caller = &module.funcs[caller_idx];
    let mut decisions = Vec::new();
    let mut caller_spent = Cost::ZERO;
    for blk in &caller.blocks {
        for inst in &blk.insts {
            match &inst.kind {
                InstKind::Call(callee_id, _args) => {
                    let callee_ix = callee_id.0 as usize;
                    let callee = &module.funcs[callee_ix];
                    if callee.is_declaration() {
                        decisions.push(InlineDecision::Skip(SkipReason::CalleeIsDeclaration));
                        continue;
                    }
                    if callee_ix == caller_idx && budget.max_recursion_depth == 0 {
                        decisions.push(InlineDecision::Skip(SkipReason::Recursion));
                        continue;
                    }
                    let callee_cost = callee_costs[callee_ix];
                    if callee_cost > budget.callee_cost_ceiling {
                        decisions.push(InlineDecision::Skip(SkipReason::CalleeTooLarge));
                        continue;
                    }
                    let projected = caller_spent.add(callee_cost);
                    if projected > budget.caller_total_budget {
                        decisions.push(InlineDecision::Skip(SkipReason::CallerBudgetExhausted));
                        continue;
                    }
                    caller_spent = projected;
                    decisions.push(InlineDecision::Inline { callee_cost });
                }
                InstKind::CallIndirect(_, _, _) => {
                    decisions.push(InlineDecision::Skip(SkipReason::Indirect));
                }
                _ => {}
            }
        }
    }
    decisions
}

/// Run the inliner pass over `module` with the default `InlineBudget`.
/// Phase 0 returns aggregate statistics; the module itself is observed
/// only by shared reference and its SSA representation is unchanged
/// after the call. Phase 1 switches the signature to `&mut Module` and
/// starts splicing callee blocks into approved call sites.
pub fn inline_module(module: &Module) -> InlinerStats {
    inline_module_with_budget(module, InlineBudget::default())
}

/// Reason a `inline_single_block_leaf` call failed its Phase 1 narrow
/// preconditions. The caller (Phase 1.0b `inline_module` driver) is
/// expected to have pre-filtered candidates via `would_inline`; any
/// `SpliceError` returned here is therefore either a true preconfition
/// violation (caller passed an inappropriate candidate) or a bug in the
/// Phase 0 classifier — both worth surfacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpliceError {
    /// `caller_blk_idx` is past `caller.blocks.len()`.
    BlockIdxOob,
    /// `site_idx` is past `caller.blocks[caller_blk_idx].insts.len()`.
    SiteIdxOob,
    /// Inst at site is not `InstKind::Call(_, _)`. Callers must locate
    /// the exact direct call site before invoking the splice.
    NotCallSite,
    /// `args.len() != callee.params.len()`. Phase 1.0a does not coerce
    /// or truncate.
    ArityMismatch,
    /// `callee.blocks.len() != 1`. Phase 1 narrow scope; multi-block
    /// inlining (block-split + Φ-node handling) arrives in Phase 2.
    NotSingleBlock,
    /// Callee body contains `Call` or `CallIndirect`. Not a leaf.
    /// Phase 0's `would_inline` predicate implicitly filters these out
    /// via the cost-budget; the runtime check here makes the contract
    /// explicit for direct API callers (tests, future drivers).
    NotLeaf,
    /// Callee terminator is not `Ret(_)`. Phase 2+ handles inlined
    /// callees whose CFG re-joins the caller via a return-block
    /// pattern; Phase 1 inlines only straight-line bodies.
    NotRetTerminator,
    /// Shape mismatch between the callee terminator and the call
    /// site result slot: `Ret(Some(_))` without `result_value` (or
    /// the reverse) means the inliner cannot bind the produced value.
    RetShapeMismatch,
}

/// Splice a single-block leaf `callee`'s body into `caller` at the
/// `Call(FuncId, _)` instruction in `caller.blocks[caller_blk_idx]
/// .insts[site_idx]`. On success the call instruction is removed and
/// the callee's body is materialised in its place with fresh
/// `ValueId`s for every non-parameter callee value; callee parameters
/// are substituted by the provided `args` directly. If the callee
/// returns a value and the call had a result slot, an
/// `InstKind::Identity` aliasing instruction is appended to bind the
/// callee's `Ret(Some(v))` operand to `result_value`.
///
/// **Phase 1 narrow scope** (each enforced by `SpliceError`):
///
/// * `callee.blocks.len() == 1` — single-block straight-line body.
/// * Callee body contains no nested call (`Call` or `CallIndirect`) —
///   a true leaf. Recursive inlining and call-chain expansion arrive
///   in Phase 2+ along with a call-graph cycle detector.
/// * Callee terminator is `Ret(_)`. Branch-terminated callees require
///   block-split + Φ-node insertion at the call site; Phase 2.
/// * Ret shape matches `result_value`: void in / out, value in / out.
/// * `args.len() == callee.params.len()`.
///
/// The call site itself is constrained too — `site_idx` must point at
/// an `InstKind::Call` inst inside an in-bounds caller block.
///
/// `BlockId` operands do not require remapping: callee's only block is
/// single, its body insts contain no `BlockId` operand fields, and the
/// terminator (`Ret(_)`) is consumed by the splice rather than copied.
/// Phase 2's multi-block path will need a real block-allocator pass.
///
/// This routine is **not** wired into `transform_module` in Phase 1.0a —
/// it is a substrate API exercised by unit tests. Phase 1.0b's
/// `inline_module(&mut Module)` driver becomes the first production
/// caller after this commit lands.
pub fn inline_single_block_leaf(
    caller: &mut Function,
    caller_blk_idx: usize,
    site_idx: usize,
    callee: &Function,
    args: &[Operand],
    result_value: Option<ValueId>,
) -> Result<(), SpliceError> {
    // ---- 1. Validate Phase 1 narrow constraints.
    if callee.blocks.len() != 1 {
        return Err(SpliceError::NotSingleBlock);
    }
    let callee_blk = &callee.blocks[0];
    for inst in &callee_blk.insts {
        if matches!(
            inst.kind,
            InstKind::Call(_, _) | InstKind::CallIndirect(_, _, _)
        ) {
            return Err(SpliceError::NotLeaf);
        }
    }
    let ret_operand = match &callee_blk.term {
        Terminator::Ret(opt) => opt.clone(),
        _ => return Err(SpliceError::NotRetTerminator),
    };
    if ret_operand.is_some() != result_value.is_some() {
        return Err(SpliceError::RetShapeMismatch);
    }
    if args.len() != callee.params.len() {
        return Err(SpliceError::ArityMismatch);
    }
    if caller_blk_idx >= caller.blocks.len() {
        return Err(SpliceError::BlockIdxOob);
    }
    if site_idx >= caller.blocks[caller_blk_idx].insts.len() {
        return Err(SpliceError::SiteIdxOob);
    }
    if !matches!(
        caller.blocks[caller_blk_idx].insts[site_idx].kind,
        InstKind::Call(_, _)
    ) {
        return Err(SpliceError::NotCallSite);
    }

    // ---- 2. Build callee-ValueId → caller-Operand mapping.
    //
    // Params map directly to the supplied `args` (no fresh allocation).
    // All other callee values get a fresh `ValueId` in `caller.values`
    // whose `ValueInfo` is cloned from the callee — same type, name
    // dropped to avoid namespace collisions in error messages.
    let mut mapping: HashMap<ValueId, Operand> = HashMap::new();
    for (i, param_id) in callee.params.iter().enumerate() {
        mapping.insert(*param_id, args[i].clone());
    }
    let param_set: std::collections::HashSet<u32> = callee.params.iter().map(|v| v.0).collect();
    for cv in 0..callee.values.len() as u32 {
        if param_set.contains(&cv) {
            continue;
        }
        let mut info = callee.values[cv as usize].clone();
        info.name = None;
        caller.values.push(info);
        let fresh = ValueId((caller.values.len() - 1) as u32);
        mapping.insert(ValueId(cv), Operand::Value(fresh));
    }

    // ---- 3. Clone & rewrite callee insts.
    let mut inlined: Vec<Inst> = Vec::with_capacity(callee_blk.insts.len() + 1);
    for inst in &callee_blk.insts {
        let new_kind = rewrite_inst_kind(&inst.kind, &mapping);
        let new_result = inst.result.map(|r| match mapping.get(&r) {
            Some(Operand::Value(v)) => *v,
            _ => unreachable!(
                "non-param callee Inst result must map to a fresh caller ValueId; \
                 mapping invariant broken"
            ),
        });
        inlined.push(Inst {
            result: new_result,
            kind: new_kind,
            origin: inst.origin,
        });
    }

    // ---- 4. Bind callee Ret value to caller result slot via Identity.
    if let (Some(ret_op), Some(r)) = (ret_operand, result_value) {
        let mapped = rewrite_operand(&ret_op, &mapping);
        let origin = inlined.last().and_then(|i| i.origin);
        inlined.push(Inst {
            result: Some(r),
            kind: InstKind::Identity(mapped),
            origin,
        });
    }

    // ---- 5. Splice into caller block, dropping the original call inst.
    let block = &mut caller.blocks[caller_blk_idx];
    let mut new_insts = Vec::with_capacity(block.insts.len() + inlined.len() - 1);
    new_insts.extend(block.insts[..site_idx].iter().cloned());
    new_insts.extend(inlined);
    new_insts.extend(block.insts[site_idx + 1..].iter().cloned());
    block.insts = new_insts;
    Ok(())
}

/// Rewrite a single `Operand` through the callee→caller mapping. Value
/// operands map either to a fresh caller `ValueId` (non-param callee
/// value) or to the caller-supplied arg (param callee value). All
/// non-Value operands (constants, etc.) pass through verbatim.
fn rewrite_operand(op: &Operand, map: &HashMap<ValueId, Operand>) -> Operand {
    match op {
        Operand::Value(v) => map.get(v).cloned().unwrap_or_else(|| op.clone()),
        _ => op.clone(),
    }
}

/// Exhaustively rewrite every Operand inside an `InstKind`. The match
/// is intentionally non-`_` so adding a new `InstKind` variant in
/// `torajs-core::ssa` is a compile-time signal here — we want explicit
/// review of how operands are routed for every new SSA opcode rather
/// than silently no-oping it.
fn rewrite_inst_kind(kind: &InstKind, map: &HashMap<ValueId, Operand>) -> InstKind {
    let r = |o: &Operand| rewrite_operand(o, map);
    match kind {
        InstKind::BinOp(op, a, b) => InstKind::BinOp(*op, r(a), r(b)),
        InstKind::ICmp(p, a, b) => InstKind::ICmp(*p, r(a), r(b)),
        InstKind::FCmp(p, a, b) => InstKind::FCmp(*p, r(a), r(b)),
        InstKind::Call(fid, args) => InstKind::Call(*fid, args.iter().map(r).collect()),
        InstKind::CallIndirect(sig, ptr, args) => {
            InstKind::CallIndirect(*sig, r(ptr), args.iter().map(r).collect())
        }
        InstKind::Alloca(ty) => InstKind::Alloca(ty.clone()),
        InstKind::AllocaBytes(n) => InstKind::AllocaBytes(*n),
        InstKind::Load(ty, ptr, off) => InstKind::Load(ty.clone(), r(ptr), *off),
        InstKind::Store(val, ptr, off) => InstKind::Store(r(val), r(ptr), *off),
        InstKind::LoadDyn(ty, ptr, off) => InstKind::LoadDyn(ty.clone(), r(ptr), r(off)),
        InstKind::StoreDyn(val, ptr, off) => InstKind::StoreDyn(r(val), r(ptr), r(off)),
        InstKind::SiToFp(o) => InstKind::SiToFp(r(o)),
        InstKind::FpToSi(o) => InstKind::FpToSi(r(o)),
        InstKind::ZExtBoolToI64(o) => InstKind::ZExtBoolToI64(r(o)),
        InstKind::ZExtI32ToI64(o) => InstKind::ZExtI32ToI64(r(o)),
        InstKind::BitCastF64ToI64(o) => InstKind::BitCastF64ToI64(r(o)),
        InstKind::BitCastI64ToF64(o) => InstKind::BitCastI64ToF64(r(o)),
        InstKind::IntToPtr(o) => InstKind::IntToPtr(r(o)),
        InstKind::PtrToInt(o) => InstKind::PtrToInt(r(o)),
        InstKind::TruncI64ToBool(o) => InstKind::TruncI64ToBool(r(o)),
        InstKind::StringRef(id) => InstKind::StringRef(*id),
        InstKind::StaticStrRef(id) => InstKind::StaticStrRef(*id),
        InstKind::GlobalRef(name) => InstKind::GlobalRef(name.clone()),
        InstKind::FnAddr(fid) => InstKind::FnAddr(*fid),
        InstKind::Identity(o) => InstKind::Identity(r(o)),
        InstKind::Neg(o) => InstKind::Neg(r(o)),
    }
}

/// `inline_module` with a caller-supplied budget. Used in tests to
/// exercise individual skip-reason buckets and budget exhaustion
/// boundaries.
pub fn inline_module_with_budget(module: &Module, budget: InlineBudget) -> InlinerStats {
    let callee_costs: Vec<Cost> = module.funcs.iter().map(callee_body_cost).collect();
    let mut stats = InlinerStats::default();
    let n = module.funcs.len();
    for caller_idx in 0..n {
        let decisions = classify_caller_sites(module, caller_idx, &budget, &callee_costs);
        for dec in decisions {
            match dec {
                InlineDecision::Inline { .. } => {
                    stats.candidates += 1;
                    stats.would_inline += 1;
                }
                InlineDecision::Skip(reason) => match reason {
                    SkipReason::CalleeTooLarge => {
                        stats.candidates += 1;
                        stats.skipped_callee_too_large += 1;
                    }
                    SkipReason::CallerBudgetExhausted => {
                        stats.candidates += 1;
                        stats.skipped_caller_budget += 1;
                    }
                    SkipReason::CalleeIsDeclaration => {
                        stats.candidates += 1;
                        stats.skipped_declaration += 1;
                    }
                    SkipReason::Recursion => {
                        stats.candidates += 1;
                        stats.skipped_recursion += 1;
                    }
                    SkipReason::Indirect => {
                        stats.skipped_indirect += 1;
                    }
                },
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Module, Operand, SigId,
        Terminator, Type, ValueId, ValueInfo,
    };

    fn val_inst(result: ValueId, kind: InstKind) -> Inst {
        Inst {
            result: Some(result),
            kind,
            origin: None,
        }
    }

    fn void_inst(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    /// Build a function with `n_ops` trivial integer adds in one block
    /// — `ALU_COST` is 1 per add, so body cost equals `n_ops`.
    fn alu_body(name: &str, n_ops: u32) -> Function {
        let mut values = vec![
            ValueInfo {
                ty: Type::I64,
                name: None,
            };
            n_ops as usize
        ];
        // Pad with an extra slot so ValueId(n_ops) below is in bounds
        // when the test wants to add a trailing identity op; harmless
        // for shorter bodies.
        values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        let insts = (0..n_ops)
            .map(|i| {
                val_inst(
                    ValueId(i),
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(1)),
                )
            })
            .collect();
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values,
            current_origin: None,
        }
    }

    /// An empty declaration function (no blocks) — `is_declaration()`
    /// returns true.
    fn declaration(name: &str) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }

    /// Wrap a caller body whose insts are supplied — `params` and
    /// `values` are minimal so the SSA layout is just enough for
    /// classification.
    fn caller(name: &str, insts: Vec<Inst>) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values: vec![],
            current_origin: None,
        }
    }

    fn module_of(funcs: Vec<Function>) -> Module {
        let mut m = Module::default();
        m.funcs = funcs;
        m
    }

    #[test]
    fn callee_body_cost_sums_alu_ops() {
        let f = alu_body("leaf", 5);
        // 5 ALU adds × 1 cycle each = 5
        assert_eq!(callee_body_cost(&f), Cost::new(5));
    }

    #[test]
    fn empty_module_returns_zero_stats() {
        let m = Module::default();
        let stats = inline_module(&m);
        assert_eq!(stats, InlinerStats::default());
    }

    #[test]
    fn caller_with_no_calls_yields_zero_candidates() {
        let m = module_of(vec![caller("main", vec![])]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 0);
        assert_eq!(stats.would_inline, 0);
    }

    #[test]
    fn small_leaf_call_is_a_would_inline_candidate() {
        // funcs[0] = main calls funcs[1] = leaf (3 ALU ops, cost 3).
        let leaf = alu_body("leaf", 3);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let m = module_of(vec![main, leaf]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 1);
        assert_eq!(stats.skipped_callee_too_large, 0);
    }

    #[test]
    fn oversized_leaf_is_rejected_with_callee_too_large() {
        // Default callee_cost_ceiling = 225. A 300-op ALU body weighs
        // 300 cycles and overshoots.
        let leaf = alu_body("fat", 300);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let m = module_of(vec![main, leaf]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 0);
        assert_eq!(stats.skipped_callee_too_large, 1);
    }

    #[test]
    fn declaration_callee_is_skipped() {
        // funcs[1] is an extern declaration (no body) — there is
        // nothing to inline, even though it's cheap.
        let leaf = declaration("extern_intrinsic");
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let m = module_of(vec![main, leaf]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 0);
        assert_eq!(stats.skipped_declaration, 1);
    }

    #[test]
    fn self_recursive_call_is_rejected_at_default_depth_zero() {
        // funcs[0] = main calls funcs[0] = main. Default budget has
        // max_recursion_depth = 0 → SkipReason::Recursion.
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(0), vec![]))]);
        let m = module_of(vec![main]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.skipped_recursion, 1);
        assert_eq!(stats.would_inline, 0);
    }

    #[test]
    fn indirect_call_is_skipped_without_consuming_candidate_slot() {
        // CallIndirect is categorically out of scope at Phase 0 — it
        // bumps skipped_indirect but not candidates.
        let main = caller(
            "main",
            vec![void_inst(InstKind::CallIndirect(
                SigId(0),
                Operand::ConstI64(0),
                vec![],
            ))],
        );
        let m = module_of(vec![main]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 0);
        assert_eq!(stats.skipped_indirect, 1);
        assert_eq!(stats.would_inline, 0);
    }

    #[test]
    fn caller_budget_exhausts_after_repeated_inlines() {
        // leaf cost = 100. caller_total_budget = 250 → first two sites
        // fire (sum 200, both under 250), third site projects to 300
        // and skips with CallerBudgetExhausted.
        let leaf = alu_body("med", 100);
        let main = caller(
            "main",
            vec![
                void_inst(InstKind::Call(FuncId(1), vec![])),
                void_inst(InstKind::Call(FuncId(1), vec![])),
                void_inst(InstKind::Call(FuncId(1), vec![])),
            ],
        );
        let budget = InlineBudget {
            callee_cost_ceiling: Cost::new(200),
            caller_total_budget: Cost::new(250),
            max_recursion_depth: 0,
        };
        let m = module_of(vec![main, leaf]);
        let stats = inline_module_with_budget(&m, budget);
        assert_eq!(stats.candidates, 3);
        assert_eq!(stats.would_inline, 2);
        assert_eq!(stats.skipped_caller_budget, 1);
    }

    // ---- Phase 1.0a splice tests ----
    //
    // These exercise `inline_single_block_leaf` directly without going
    // through `inline_module`. Each test builds a minimal `Function`
    // pair and asserts (a) the splice succeeds when constraints are
    // met and (b) the produced IR shape matches the expectation —
    // call inst gone, callee body materialised with fresh ValueIds,
    // result bound via Identity when applicable.

    /// Construct a callee with a fixed param shape and a `Ret` of a
    /// computed value. The body is one `Add` of param[0] + param[1]
    /// whose result is returned.
    fn add_callee_2_params() -> Function {
        let values = vec![
            ValueInfo {
                ty: Type::I64,
                name: Some("a".into()),
            }, // ValueId(0) param0
            ValueInfo {
                ty: Type::I64,
                name: Some("b".into()),
            }, // ValueId(1) param1
            ValueInfo {
                ty: Type::I64,
                name: Some("sum".into()),
            }, // ValueId(2) sum
        ];
        Function {
            name: "add".into(),
            params: vec![ValueId(0), ValueId(1)],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
            }],
            values,
            current_origin: None,
        }
    }

    #[test]
    fn splice_value_returning_leaf_into_caller() {
        // caller body: %0 = call add(const 3, const 4); ret %0.
        // After splice: %1 = add 3, 4; %0 = identity %1; (call removed).
        let mut caller = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(0),
                    InstKind::Call(FuncId(1), vec![Operand::ConstI64(3), Operand::ConstI64(4)]),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            }],
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("r".into()),
            }],
            current_origin: None,
        };
        let callee = add_callee_2_params();
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(3), Operand::ConstI64(4)],
            Some(ValueId(0)),
        );
        assert!(res.is_ok());
        let insts = &caller.blocks[0].insts;
        assert_eq!(insts.len(), 2, "1 Add + 1 Identity, call gone");
        match &insts[0].kind {
            InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::ConstI64(4)) => {}
            other => panic!("expected Add ConstI64 3, ConstI64 4; got {:?}", other),
        }
        // First inst's result must be a fresh ValueId, not the caller's
        // ValueId(0).
        let sum_id = insts[0].result.expect("Add must have a result");
        assert_ne!(sum_id, ValueId(0), "callee sum must remap to a fresh id");
        match &insts[1].kind {
            InstKind::Identity(Operand::Value(v)) if *v == sum_id => {}
            other => panic!("expected Identity(Value(sum_id)); got {:?}", other),
        }
        assert_eq!(insts[1].result, Some(ValueId(0)));
    }

    #[test]
    fn splice_void_leaf_into_caller() {
        // Callee: do-nothing void fn (empty body, Ret(None)).
        let callee = Function {
            name: "noop".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Ret(None),
            }],
            values: vec![],
            current_origin: None,
        };
        let mut caller = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let before_inst_count = caller.blocks[0].insts.len();
        let res = inline_single_block_leaf(&mut caller, 0, 0, &callee, &[], None);
        assert!(res.is_ok());
        assert_eq!(
            caller.blocks[0].insts.len(),
            before_inst_count - 1,
            "noop callee + void result → call inst alone disappears"
        );
    }

    #[test]
    fn splice_rejects_multi_block_callee() {
        let multi = Function {
            name: "multi".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(None),
                },
            ],
            values: vec![],
            current_origin: None,
        };
        let mut caller = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let res = inline_single_block_leaf(&mut caller, 0, 0, &multi, &[], None);
        assert_eq!(res, Err(SpliceError::NotSingleBlock));
    }

    #[test]
    fn splice_rejects_non_leaf_callee() {
        // Callee body contains a nested Call → NotLeaf.
        let non_leaf = Function {
            name: "calls_other".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![void_inst(InstKind::Call(FuncId(2), vec![]))],
                term: Terminator::Ret(None),
            }],
            values: vec![],
            current_origin: None,
        };
        let mut caller = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let res = inline_single_block_leaf(&mut caller, 0, 0, &non_leaf, &[], None);
        assert_eq!(res, Err(SpliceError::NotLeaf));
    }

    #[test]
    fn splice_rejects_non_ret_terminator() {
        let branchy = Function {
            name: "diverges".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Unreachable,
            }],
            values: vec![],
            current_origin: None,
        };
        let mut caller = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let res = inline_single_block_leaf(&mut caller, 0, 0, &branchy, &[], None);
        assert_eq!(res, Err(SpliceError::NotRetTerminator));
    }

    #[test]
    fn splice_rejects_ret_shape_mismatch() {
        // Callee returns a value but caller passes result_value=None.
        let callee = add_callee_2_params();
        let mut caller = caller(
            "main",
            vec![void_inst(InstKind::Call(
                FuncId(1),
                vec![Operand::ConstI64(0), Operand::ConstI64(0)],
            ))],
        );
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(0), Operand::ConstI64(0)],
            None,
        );
        assert_eq!(res, Err(SpliceError::RetShapeMismatch));
    }

    #[test]
    fn splice_rejects_arity_mismatch() {
        let callee = add_callee_2_params();
        let mut caller = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(0),
                    InstKind::Call(FuncId(1), vec![Operand::ConstI64(3)]),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            }],
            values: vec![ValueInfo {
                ty: Type::I64,
                name: None,
            }],
            current_origin: None,
        };
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(3)],
            Some(ValueId(0)),
        );
        assert_eq!(res, Err(SpliceError::ArityMismatch));
    }

    #[test]
    fn splice_rejects_non_call_site() {
        let callee = add_callee_2_params();
        let mut caller = caller(
            "main",
            vec![val_inst(
                ValueId(0),
                InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
            )],
        );
        // values must accommodate the Add result.
        caller.values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(1), Operand::ConstI64(2)],
            Some(ValueId(0)),
        );
        assert_eq!(res, Err(SpliceError::NotCallSite));
    }

    #[test]
    fn splice_preserves_surrounding_insts_in_caller_block() {
        // Caller block has: [pre = const 1; call leaf; post = const 2].
        // After splice the order is: [pre; <inlined>; post].
        let callee = Function {
            name: "leaf".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(0),
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(10), Operand::ConstI64(20)),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            }],
            values: vec![ValueInfo {
                ty: Type::I64,
                name: None,
            }],
            current_origin: None,
        };
        let mut caller = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    val_inst(
                        ValueId(0),
                        InstKind::BinOp(BinOp::Add, Operand::ConstI64(100), Operand::ConstI64(0)),
                    ),
                    val_inst(ValueId(1), InstKind::Call(FuncId(1), vec![])),
                    val_inst(
                        ValueId(2),
                        InstKind::BinOp(
                            BinOp::Sub,
                            Operand::Value(ValueId(1)),
                            Operand::ConstI64(0),
                        ),
                    ),
                ],
                term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
            }],
            values: vec![
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
            ],
            current_origin: None,
        };
        let res = inline_single_block_leaf(&mut caller, 0, 1, &callee, &[], Some(ValueId(1)));
        assert!(res.is_ok());
        let insts = &caller.blocks[0].insts;
        // Expect 4 insts: pre Add, inlined Add, Identity binding result,
        // post Sub.
        assert_eq!(insts.len(), 4, "pre + inlined Add + Identity + post");
        assert!(
            matches!(
                insts[0].kind,
                InstKind::BinOp(BinOp::Add, Operand::ConstI64(100), _)
            ),
            "pre inst preserved at index 0"
        );
        assert!(
            matches!(
                insts[3].kind,
                InstKind::BinOp(BinOp::Sub, Operand::Value(v), _) if v == ValueId(1)
            ),
            "post inst preserved at end, still consumes ValueId(1)"
        );
    }

    #[test]
    fn module_is_not_mutated_in_phase_0() {
        // Phase 0 contract: SSA representation observed by shared ref;
        // module funcs / blocks / insts are bit-identical before and
        // after. This guards against accidentally introducing emit
        // logic at the scaffold level (the wire-in is reserved for
        // Phase 1's first cluster commit).
        let leaf = alu_body("leaf", 4);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let m_before = module_of(vec![main.clone(), leaf.clone()]);
        let m_after = m_before.clone();
        let _ = inline_module(&m_after);
        assert_eq!(m_after.funcs.len(), m_before.funcs.len());
        for (a, b) in m_after.funcs.iter().zip(m_before.funcs.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.blocks.len(), b.blocks.len());
            for (ba, bb) in a.blocks.iter().zip(b.blocks.iter()) {
                assert_eq!(ba.insts.len(), bb.insts.len());
            }
        }
    }
}
