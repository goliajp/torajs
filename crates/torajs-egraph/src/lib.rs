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

pub mod anynum;
pub mod block_chain;
pub mod block_layout;
pub mod branch_fold;
pub mod cell_access;
mod cfg_edit;
pub mod cmp_fact_fold;
pub mod concat_len;
pub mod concat_licm;
pub mod concat_num_fuse;
pub mod const_copy_fwd;
pub mod cost;
pub mod ctpop_idiom;
pub mod ctpop_range_sum;
pub mod devirt;
pub mod egraph;
mod egraph_pass;
pub mod elaborate;
mod fact_folds;
pub mod fconst_hoist;
pub mod fdiv_pair;
pub mod float_demote;
pub mod fn_int_clone;
pub mod frame_arr_drop;
pub mod frame_cell_clean;
pub mod frem_guard;
pub mod frem_int;
pub mod frem_narrow;
pub mod fresh_cell;
pub mod heap_to_stack;
pub mod iconst_hoist;
pub mod inliner;
pub mod int_entry;
pub mod interval;
pub mod iv_merge;
pub mod late_gvn;
pub mod len_promote;
pub mod load_licm;
pub mod loop_rotate;
pub mod loop_widen;
pub mod map_find_cse;
pub mod mem2reg;
pub mod merge_sroa;
pub mod null_check_fold;
pub mod optimize;
pub mod optimize_licm;
mod pass_gates;
mod pass_timer;
pub mod phi_promote;
pub mod phi_thread;
mod pipeline_head;
mod pipeline_loop;
mod pipeline_tail;
pub mod print_narrow;
pub mod rc_borrow;
mod rc_borrow_check;
mod rc_borrow_origin;
mod rc_borrow_rewrite;
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
pub mod srem_counter;
pub mod srem_fquot;
pub mod srem_parity;
pub mod str_append;
mod strip_assumes;
mod tail_dup;
pub mod throw_check_fold;
pub mod unroll_jam;
pub mod unroll_pair;

use id_hash::{HashMap, HashSet};
pub use torajs_core::ssa::id_hash;

use torajs_core::ssa::{Module, ValueId};

use branch_fold::sum_facts::bound_sums;

pub use crate::egraph_pass::EgraphPass;
use crate::pass_gates::{dump_after, gated_pass};

/// Per function, the values its interval facts prove inside i32 (none
/// when the analysis is off).
fn sext_seeds(
    module: &Module,
    facts: Option<&[HashMap<ValueId, interval::NumFact>]>,
) -> Vec<HashSet<ValueId>> {
    facts
        .map(|facts| {
            module
                .funcs
                .iter()
                .zip(facts)
                .map(|(func, fm)| interval::sext32_set(fm, func))
                .collect()
        })
        .unwrap_or_default()
}

/// The per-function egraph pass over every function.
fn egraph_functions(module: &mut Module) {
    for func in module.funcs.iter_mut() {
        *func = EgraphPass::new(func).run();
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
/// * `TORAJS_EGRAPH_OFF=1` — skip both inliner and egraph; only the
///   typed number nodes are expanded, since codegen has no lowering
///   for them.
/// * `TORAJS_INLINER_OFF=1` — skip only the inliner emit pass
///   (classification stats are still produced but discarded here);
///   `EgraphPass` still runs.
///
/// This is the canonical integration entry point for the `tr build` /
/// `tr run` new-pipeline drivers; they call this between
/// `ssa_lower::lower_with_arity` and `torajs_codegen::compile_function`.
pub fn transform_module(mut module: Module) -> Module {
    // the analyses kept for an earlier module's functions are of no
    // use to this one
    torajs_core::ssa::dominator::forget_kept_analyses();
    if std::env::var("TORAJS_EGRAPH_OFF").as_deref() == Ok("1") {
        anynum::expand::expand_number_nodes(&mut module);
        return module;
    }
    pass_timer::arm();
    pipeline_head::inline_and_forward(&mut module);
    pass_timer::timed("EGRAPH", || egraph_functions(&mut module));
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
    // The typed number-box nodes expand to the NaN-box bit operations
    // here: after φ promotion (the number flow through a loop cell is
    // read in the promoted form), before late_gvn and every pass that
    // matches the bit shapes. It is part of the lowering — codegen has
    // no rule for the nodes — so it has no off switch.
    // A loop's number-valued `any` cells run as scalar lanes in a
    // copy of the loop (`anynum`), before the nodes expand.
    // `TORAJS_ANYNUM_CELL_OFF=1` skips.
    gated_pass("ANYNUM_CELL", &mut module, anynum::unbox_loop_cells);
    // `rc_inc_any` / `rc_dec_any` of a value every path has tested
    // as a number go (`anynum::rc_fold`). `TORAJS_ANYNUM_RC_OFF=1` skips.
    gated_pass("ANYNUM_RC", &mut module, anynum::rc_fold::fold_number_rcs);
    pass_timer::timed("ANYNUM_EXPAND", || {
        anynum::expand::expand_number_nodes(&mut module)
    });
    dump_after("ANYNUM_EXPAND", &module);
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
    // `TORAJS_FN_INT_CLONE_OFF=1` skips (bisect gate).
    let clones = gated_pass("FN_INT_CLONE", &mut module, fn_int_clone::clone_functions)
        .map_or_else(Vec::new, |s| s.clones);
    // `TORAJS_INT_ENTRY_OFF=1` skips (bisect gate).
    gated_pass("INT_ENTRY", &mut module, int_entry::version_loops);
    // Integer range analysis (analysis-only, RFC 20260611) — interval
    // lattice with branch refinement + loop widening over the post-φ
    // canonical shape. Feeds float demotion below and seeds sext_elide
    // R2 with loop cells provably inside i32 range (the popcount
    // residual-pair case). `TORAJS_INTERVAL_OFF=1` skips (bisect gate).
    let (mut interval_facts, fact_cache) =
        pass_timer::timed("INTERVAL", || interval::run::analyze_gated(&module));
    // Loop sums take their add-recurrence bound before float_demote
    // reads the facts: one the bound keeps inside ±2^53 demotes with
    // no window check. `TORAJS_SUM_FACTS_OFF=1` skips.
    if let Some(facts) = &mut interval_facts {
        gated_pass("SUM_FACTS", &mut module, |m| {
            bound_sums(m, facts, fact_cache.as_ref())
        });
    }
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
    let sext_seeds = pass_timer::timed("SEXT_SEEDS", || {
        sext_seeds(&module, interval_facts.as_deref())
    });
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
    gated_pass("BRANCH_FOLD", &mut module, |m| {
        branch_fold::fold_branches(m, fact_cache.as_ref())
    });
    // Array header words out of loops (`array_header_passes`). After
    // branch_fold: its folded-away arms are no longer loop body.
    pipeline_loop::array_header_passes(&mut module);
    // Frame-literal array releases behind an inline born-shape test
    // (`frame_arr_drop`). After the header passes, which read the
    // release as a call naming the array.
    gated_pass(
        "FRAME_ARR_DROP",
        &mut module,
        frame_arr_drop::guard_frame_arr_drops,
    );
    // Repeated nullish checks and pending-exception probes whose
    // answer is already known. After branch_fold (it removes the
    // out-of-range arms whose calls would otherwise sit between).
    throw_check_fold::run(&mut module);
    // Integral-f64 print narrowing — `print_f64(sitofp w)` with `w`
    // provably inside ±2^53 prints the same digits through
    // `print_i64(w)`; the demoted accumulator's exit bridge is the
    // shape (its guard just folded above, so the bridge has one def).
    // `TORAJS_PRINT_NARROW_OFF=1` skips (bisect gate).
    gated_pass("PRINT_NARROW", &mut module, |m| {
        print_narrow::narrow_prints(m, fact_cache.as_ref())
    });
    drop(fact_cache);
    pipeline_loop::hoist_loop_consts(&mut module);
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
    // Integer sum loops run two i64 lanes per vector in front of their
    // scalar loop, the growth guards checked once per block (see
    // `loop_widen`). After branch_fold and the ctpop passes, which
    // leave the guarded straight-line bodies it matches; before
    // select_form and block_layout. `TORAJS_LOOP_WIDEN_OFF=1` skips.
    gated_pass("LOOP_WIDEN", &mut module, loop_widen::widen_sum_loops);
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
    pipeline_tail::form_selects_and_fuse(&mut module);
    pipeline_tail::finish_pipeline(&mut module, interval_facts.as_deref(), &clones);
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
    fn insts_over_two_literals_fold_away() {
        // Both BinOps fold to literals no use reads, so nothing is left.
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
        assert!(out.blocks[0].insts.is_empty());
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
