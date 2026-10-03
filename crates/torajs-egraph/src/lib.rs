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

pub mod block_chain;
pub mod block_layout;
pub mod branch_fold;
pub mod cell_access;
pub mod cmp_fact_fold;
pub mod concat_num_fuse;
pub mod cost;
pub mod ctpop_idiom;
pub mod ctpop_range_sum;
pub mod devirt;
pub mod egraph;
mod egraph_pass;
pub mod elaborate;
mod fact_folds;
pub mod fconst_hoist;
pub mod float_demote;
pub mod frem_guard;
pub mod frem_int;
pub mod frem_narrow;
pub mod fresh_cell;
pub mod heap_to_stack;
pub mod iconst_hoist;
pub mod inliner;
pub mod int_entry;
pub mod interval;
pub mod late_gvn;
pub mod len_promote;
pub mod load_licm;
pub mod loop_rotate;
pub mod map_find_cse;
pub mod mem2reg;
pub mod merge_sroa;
pub mod null_check_fold;
pub mod optimize;
pub mod optimize_licm;
pub mod phi_promote;
pub mod phi_thread;
mod pipeline_tail;
pub mod print_narrow;
pub mod rc_dec_immediate;
mod rc_obj_pair;
pub mod rc_peephole;
pub mod rewrite;
pub mod scaled_addr;
pub mod scope_map;
pub mod select_form;
pub mod self_tail_call;
pub mod sext_elide;
pub mod slot_cells;
pub mod slot_forward;
pub mod srem_fquot;
pub mod srem_parity;
pub mod str_append;
pub mod throw_check_fold;
pub mod unroll_jam;

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Module, ValueId};

pub use crate::egraph_pass::EgraphPass;

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
/// Run one module pass behind the pair of environment gates every
/// pass in the pipeline carries: `TORAJS_<KEY>_OFF=1` skips it
/// (bisect gate) and `TORAJS_<KEY>_STATS=1` dumps its counters as
/// `torajs-<key>-stats:`. Returns the counters when the pass ran.
///
/// `key` is the screaming-snake pass name (`"BRANCH_FOLD"`); both
/// variable names and the stats label derive from it, so a new pass
/// can't drift into a mismatched gate/label triple.
fn gated_pass<S: std::fmt::Debug>(
    key: &str,
    module: &mut Module,
    pass: impl FnOnce(&mut Module) -> S,
) -> Option<S> {
    if std::env::var(format!("TORAJS_{key}_OFF")).as_deref() == Ok("1") {
        return None;
    }
    let stats = pass(module);
    if std::env::var(format!("TORAJS_{key}_STATS")).as_deref() == Ok("1") {
        let label = key.to_lowercase().replace('_', "-");
        eprintln!("torajs-{label}-stats: {stats:?}");
    }
    dump_after(key, module);
    Some(stats)
}

/// `TORAJS_SSA_DUMP_AFTER=<KEY>` — pretty-print the whole module to
/// stdout right after the named pass, `KEY` being the same screaming-
/// snake name its `_OFF` / `_STATS` gates use (plus `EGRAPH` for the
/// per-function egraph loop, which is not a `gated_pass`).
///
/// The end-of-pipeline `TORAJS_SSA_DUMP` answers "what shape did the
/// pipeline produce"; this answers "which pass produced it". Diffing
/// the dump across two runs is how a nondeterministic pass gets named:
/// walk the keys in pipeline order and the first one whose dump varies
/// owns the divergence. Toggling `_OFF` gates cannot do that job —
/// passes feed each other, so several will each "fix" one source.
///
/// A pass gated off never reaches here, so asking to dump after a
/// disabled pass prints nothing.
fn dump_after(key: &str, module: &Module) {
    if std::env::var("TORAJS_SSA_DUMP_AFTER").as_deref() == Ok(key) {
        module.print();
    }
}

/// The integer range analysis, or `None` under `TORAJS_INTERVAL_OFF=1`;
/// `TORAJS_INTERVAL_STATS=1` prints each fn's fact counts to stderr.
fn analyze_intervals(module: &Module) -> Option<Vec<HashMap<ValueId, interval::NumFact>>> {
    (std::env::var("TORAJS_INTERVAL_OFF").as_deref() != Ok("1")).then(|| {
        let facts = interval::analyze_module(module);
        if std::env::var("TORAJS_INTERVAL_STATS").as_deref() == Ok("1") {
            for (func, fm) in module.funcs.iter().zip(&facts) {
                if !func.is_declaration() {
                    let st = interval::stats_for(fm, func);
                    eprintln!("torajs-interval-stats: {} {st:?}", func.name);
                }
            }
        }
        facts
    })
}

/// The per-function egraph pass over every function.
fn egraph_functions(module: &mut Module) {
    for func in module.funcs.iter_mut() {
        *func = EgraphPass::new(func).run();
    }
}

/// The two passes that take array header words out of loops: the
/// length of a loop that stores it into a register, then header-field
/// loads of arrays the loop never writes. Before throw_check_fold (its
/// loop-entry probe would otherwise sit between the preheader and the
/// header).
fn array_header_passes(module: &mut Module) {
    // Array length word into a register across a loop that stores it
    // (`len_promote`): the pop loop's `str len` → next `ldr len` chain.
    // After branch_fold (its folded arms are no longer loop body),
    // before load_licm, which then sees the loop without the stores.
    // `TORAJS_LEN_PROMOTE_OFF=1` skips (bisect gate).
    gated_pass("LEN_PROMOTE", module, len_promote::promote_lengths);
    // Array header-field loads out of loops that never write that
    // array's header (allocation-site alias slices, `load_licm`).
    // After branch_fold (a folded-away out-of-range arm is no longer
    // loop body), before throw_check_fold (its loop-entry probe would
    // otherwise sit between the preheader and the header).
    // `TORAJS_LOAD_LICM_OFF=1` skips (bisect gate).
    gated_pass("LOAD_LICM", module, load_licm::hoist_header_loads);
}

/// The two loop-constant hoists: each distinct in-loop `ConstF64`
/// operand mints one preheader Copy and body uses read the value,
/// killing the per-iteration 3-inst rematerialization + GPR→FPR
/// crossing (mandelbrot decomposition D1); its i64 twin does the same
/// for a compare's non-imm12 constant (rotation 623, the fill-loop
/// bound). After branch_fold (its folds shrink loop bodies), before
/// select_form / cmp_sink (their formed shapes read the already-
/// hoisted operands). `TORAJS_FCONST_HOIST_OFF=1` /
/// `TORAJS_ICONST_HOIST_OFF=1` skip either (bisect gates).
fn hoist_loop_consts(module: &mut Module) {
    gated_pass("FCONST_HOIST", module, fconst_hoist::hoist_fp_consts);
    gated_pass("ICONST_HOIST", module, iconst_hoist::hoist_int_consts);
}

/// What the egraph pass eats: callees inlined (a second round after
/// devirt turns fn-pointer calls direct), then the param round-trips
/// that splices leave forwarded and promoted.
fn inline_and_forward(module: &mut Module) {
    let inliner_stats = inliner::inline_module(module);
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
    if let Some(devirt_stats) = gated_pass("DEVIRT", module, devirt::devirtualize_module)
        && devirt_stats.rewritten > 0
    {
        let round2 = inliner::inline_module(module);
        if std::env::var("TORAJS_INLINER_STATS").as_deref() == Ok("1") {
            eprintln!("torajs-inliner-stats-round2: {round2:?}");
        }
    }
    // Block-local store→load forwarding — clears the alloca+store+load
    // param round-trips the inliner splices into hot loops (mem2reg's
    // degenerate in-block case). After inlining/devirt (the food),
    // before the egraph pass (forwarded constants feed const-fold /
    // GVN). `TORAJS_SLOTFWD_OFF=1` skips (bisect gate).
    gated_pass("SLOTFWD", module, slot_forward::forward_slot_loads);
    // Cross-block single-def-block slot promotion — the dominance
    // fast path of LLVM mem2reg (param spills whose loads live in
    // branch arms). After slot_forward (in-block round-trips already
    // cleared, slots possibly DSE'd), before the egraph pass.
    // `TORAJS_MEM2REG_OFF=1` skips (bisect gate).
    gated_pass("MEM2REG", module, mem2reg::promote_slots);
    // after the slot passes (the ctor's `this` reaches the cell through
    // a param spill), before heap_to_stack; `TORAJS_FRESH_CELL_OFF=1` skips
    gated_pass("FRESH_CELL", module, fresh_cell::forward_fresh_cells);
    // Heap-to-stack — a header cell whose every use now sits in this
    // function becomes an alloca. After the slot passes above (the
    // cell pointer often reaches its uses through an inlined local's
    // spill slot), then those passes again to forward its fields.
    // `TORAJS_HEAP_TO_STACK_OFF=1` skips (bisect gate).
    if gated_pass("HEAP_TO_STACK", module, heap_to_stack::promote_module)
        .is_some_and(|s| s.promoted > 0)
    {
        gated_pass("SLOTFWD", module, slot_forward::forward_slot_loads);
        gated_pass("MEM2REG", module, mem2reg::promote_slots);
    }
}

pub fn transform_module(mut module: Module) -> Module {
    if std::env::var("TORAJS_EGRAPH_OFF").as_deref() == Ok("1") {
        return module;
    }
    inline_and_forward(&mut module);
    egraph_functions(&mut module);
    dump_after("EGRAPH", &module);
    // Full mem2reg φ promotion — loop-carried / join-slot cells the
    // earlier passes left behind. AFTER the egraph pass (GVN /
    // elaborate never see the multi-def Copy it emits), before
    // rc_peephole (which treats Copy as rc-transparent).
    // `TORAJS_MEM2REG_PHI_OFF=1` skips (bisect gate).
    gated_pass("MEM2REG_PHI", &mut module, phi_promote::promote_phi_slots);
    // Stack cells merged through one Copy (the inlined generator step
    // object) read field by field (`merge_sroa`): needs that Copy.
    gated_pass("MERGE_SROA", &mut module, merge_sroa::replace_merged_cells);
    // Post-φ dominator GVN — the main egraph GVN ran before φ
    // promotion (it must not see multi-def Copies), so loop-carried
    // reads were opaque Loads to it and cross-block CSE of shapes
    // like mandelbrot's duplicated `zr * zr` never fired. This pass
    // re-runs a scoped GVN on the promoted form; entries touching
    // φ-web multi-def values ride a restricted EBB-chain scope (see
    // late_gvn.rs module doc). `TORAJS_LATE_GVN_OFF=1` skips.
    late_gvn::run(&mut module);
    // A goto's `state = copy K; br head` jumps past the compare chain
    // its constant decides (`phi_thread`); needs the φ-destructed
    // copies, so after MEM2REG_PHI. `TORAJS_PHI_THREAD_OFF=1` skips.
    gated_pass("PHI_THREAD", &mut module, phi_thread::thread_phi_constants);
    // frem truncation recovery (W3 C2, ann-width RFC §5.3) — narrows
    // the -0-insensitive float `%` shapes C1 minted (single-use frem
    // into an integral fcmp or an fptosi sink) back to srem, so the
    // interval analysis / float_demote below see the recovered int
    // loops directly. `TORAJS_FREM_NARROW_OFF=1` skips (bisect gate).
    gated_pass("FREM_NARROW", &mut module, frem_narrow::narrow_frems);
    // `TORAJS_INT_ENTRY_OFF=1` skips (bisect gate).
    gated_pass("INT_ENTRY", &mut module, int_entry::version_loops);
    // Integer range analysis (analysis-only, RFC 20260611) — interval
    // lattice with branch refinement + loop widening over the post-φ
    // canonical shape. Feeds float demotion below and seeds sext_elide
    // R2 with loop cells provably inside i32 range (the popcount
    // residual-pair case). `TORAJS_INTERVAL_OFF=1` skips (bisect gate).
    let mut interval_facts = analyze_intervals(&module);
    // Float demotion (RFC 20260611 phase 1b-i) — integer-valued f64
    // closures whose every op is provably exact rewrite to i64 in
    // place (frem→srem kills the per-iteration fmod libcall). Facts
    // stay valid across the rewrite (values keep their id and bounds),
    // so demoted in-i32-range cells seed sext_elide below for free.
    // `TORAJS_FLOAT_DEMOTE_OFF=1` skips (bisect gate).
    if let Some(facts) = &interval_facts {
        gated_pass("FLOAT_DEMOTE", &mut module, |m| {
            float_demote::demote_floats(m, facts)
        });
    }
    if let Some(facts) = &mut interval_facts {
        fact_folds::fact_folds(&mut module, facts);
    }
    // Parity-compare strength reduction — a srem-by-2^k whose every
    // use is an eq/ne-vs-0 compare rewrites to and-by-mask (codegen
    // expands SRem to sdiv+msub; the divider trip is invisible to
    // GVN). After float_demote (its FRem→SRem output is the feeder).
    // `TORAJS_SREM_PARITY_OFF=1` skips (bisect gate).
    gated_pass("SREM_PARITY", &mut module, srem_parity::reduce_parity_srems);
    let sext_seeds: Vec<HashSet<ValueId>> = interval_facts
        .map(|facts| {
            module
                .funcs
                .iter()
                .zip(&facts)
                .map(|(func, fm)| interval::sext32_set(fm, func))
                .collect()
        })
        .unwrap_or_default();
    // Redundant ToInt32 sext-pair elimination — after phi_promote so
    // it sees the final canonical shape (loop cells as multi-def Copy,
    // unknown to its own one-pass lattice but seedable from the
    // interval analysis above), before rc_peephole / codegen.
    // Transposes operand-side `shl 32`+`ashr 32` pairs on And/Or/Xor
    // into one result-side pair and collapses pairs over provably-
    // sext-32 sources. `TORAJS_SEXT_ELIDE_OFF=1` skips (bisect gate).
    gated_pass("SEXT_ELIDE", &mut module, |m| {
        sext_elide::elide_sext_pairs(m, &sext_seeds)
    });
    // Kernighan popcount loop-idiom recognition (LLVM
    // LoopIdiomRecognize analogue) — after sext_elide, whose
    // interval-seeded pair elision produces the pair-free 5-inst loop
    // body this pass matches. Replaces the whole 2-block loop with a
    // single `ctpop` + add. `TORAJS_CTPOP_OFF=1` skips (bisect gate).
    gated_pass("CTPOP", &mut module, ctpop_idiom::recognize_ctpop_loops);
    // Constant-branch folding over interval evidence — the ctpop
    // collapse just made the float_demote growth guards provably
    // false (count is a ctpop ≤ 64, the accumulator is trip-bounded),
    // which the flow-insensitive lattice cannot see through cell
    // joins; branch_fold's demand-driven point/recurrence evaluators
    // can. After ctpop (the evidence source), before select_form
    // (folded diamonds must not be select-formed).
    // `TORAJS_BRANCH_FOLD_OFF=1` skips (bisect gate).
    gated_pass("BRANCH_FOLD", &mut module, branch_fold::fold_branches);
    // Array header words out of loops (`array_header_passes`). After
    // branch_fold: its folded-away arms are no longer loop body.
    array_header_passes(&mut module);
    // Repeated nullish checks and pending-exception probes whose
    // answer is already known. After branch_fold (it removes the
    // out-of-range arms whose calls would otherwise sit between).
    throw_check_fold::run(&mut module);
    // Integral-f64 print narrowing — `print_f64(sitofp w)` with `w`
    // provably inside ±2^53 prints the same digits through
    // `print_i64(w)`; the demoted accumulator's exit bridge is the
    // shape (its guard just folded above, so the bridge has one def).
    // `TORAJS_PRINT_NARROW_OFF=1` skips (bisect gate).
    gated_pass("PRINT_NARROW", &mut module, print_narrow::narrow_prints);
    hoist_loop_consts(&mut module);
    // Ctpop-range-sum reduction recognition — collapse a counted
    // `acc += ctpop(i)` loop into one `CtpopRangeSum` super-inst that
    // codegen expands to an 8-wide SIMD reduction (RFC
    // 20260719-ctpop-range-sum blade 2). After branch_fold, which is
    // what flattens the body to the straight-line chain matched here;
    // before block_layout, which lays out the collapsed result.
    gated_pass(
        "CTPOP_RANGESUM",
        &mut module,
        ctpop_range_sum::form_ctpop_range_sums,
    );
    // Self-tail-call elimination — rewrite `return f(args)` self
    // recursion into parameter rebinding + a branch back to the header
    // behind a runtime cell==env guard, so 100k-deep tail recursion
    // runs in O(1) stack (RFC 20260810-self-tail-call). After
    // mem2reg/phi_promote (param uses are direct SSA and the multi-def
    // Copy shape is legal) and after branch_fold (CFG settled); before
    // select_form so the matched throw_check diamond is still the raw
    // ssa_lower shape. `TORAJS_SELF_TAIL_CALL_OFF=1` skips (bisect
    // gate).
    gated_pass(
        "SELF_TAIL_CALL",
        &mut module,
        self_tail_call::eliminate_self_tail_calls,
    );
    // Select formation — if-convert pure CondBr diamonds into csel-
    // shaped `Select` defs (RFC 20260719-select-formation blade 2).
    // After ctpop so every arm-shaping rewrite (float_demote /
    // srem_parity) is done; before rc_peephole, which treats Select
    // as rc-transparent. `TORAJS_SELECT_FORM_OFF=1` skips (bisect
    // gate, mirrors the sibling passes).
    gated_pass("SELECT_FORM", &mut module, select_form::form_selects);
    // Compare sink — restore ICmp/Select adjacency that the hoisted
    // speculated arms just broke, so codegen's adjacency-gated NZCV
    // fuse fires (RFC 20260719-select-formation route ③). Right after
    // select_form: the Selects exist, and liveness is computed later
    // on the sunk order, which is what makes the delayed compare's
    // operands readable by construction.
    gated_pass("CMP_SINK", &mut module, select_form::cmp_sink::sink_cmps);
    // TORAJS_SSA_DUMP=1 — pretty-print the post-egraph pre-peephole
    // SSA to stdout. Debug surface for attributing which pass shaped
    // a given inst stream (mirrors TORAJS_INLINER_STATS).
    if std::env::var("TORAJS_SSA_DUMP").as_deref() == Ok("1") {
        module.print();
    }
    // Number-to-string concat fusion — a strict `to_str` + `concat`
    // (right operand) + `drop` triple becomes one `concat_num` call
    // that formats the digits straight into the result allocation
    // (S1-A2 attack B1). Before str_append: this is the more
    // specific shape, and a number-on-the-left temp would otherwise
    // be claimed as str_append's drop-left operand.
    // `TORAJS_CONCAT_NUM_FUSE_OFF=1` skips (bisect gate).
    gated_pass(
        "CONCAT_NUM_FUSE",
        &mut module,
        concat_num_fuse::fuse_concat_nums,
    );
    // String-append ownership forwarding — an adjacent `concat` +
    // `drop-left` pair becomes one `append`, which may then grow the
    // left cell in place instead of reallocating it. Last of the
    // inst-level rewrites so inlined-in and spliced pairs are already
    // adjacent; before rc_peephole, whose window a call closes anyway.
    // `TORAJS_STR_APPEND_OFF=1` skips (bisect gate).
    gated_pass("STR_APPEND", &mut module, str_append::rewrite_str_appends);
    // Scaled-addressing fold — `LoadDyn(_, base, Shl(idx, 3))` with a
    // single-use shift becomes `LoadDynScaled8` and the shift dies
    // (S7 knife c2): the AGU does the ×8, taking the shift off the
    // address dependency chain of every array-index loop iteration.
    // After the inst-level rewrites above so spliced/inlined shapes
    // are final; before regalloc by construction (SSA→SSA).
    // `TORAJS_SCALED_ADDR_OFF=1` skips (bisect gate).
    gated_pass(
        "SCALED_ADDR",
        &mut module,
        scaled_addr::fold_scaled_addresses,
    );
    pipeline_tail::finish_pipeline(&mut module);
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
