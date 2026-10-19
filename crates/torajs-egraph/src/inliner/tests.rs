use super::*;
use torajs_core::ssa::{
    BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Module, Operand, SigId, SuspendKind,
    SuspendLane, Terminator, Type, ValueId, ValueInfo,
};

mod recursion;

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
    Module {
        funcs,
        ..Default::default()
    }
}

#[test]
fn callee_body_cost_sums_alu_ops() {
    let f = alu_body("leaf", 5);
    assert_eq!(callee_body_cost(&f), Cost::new(5));
}

#[test]
fn empty_module_returns_zero_stats() {
    let mut m = Module::default();
    let stats = inline_module(&mut m);
    assert_eq!(stats, InlinerStats::default());
}

#[test]
fn caller_with_no_calls_yields_zero_candidates() {
    let mut m = module_of(vec![caller("main", vec![])]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.candidates, 0);
    assert_eq!(stats.would_inline, 0);
}

#[test]
fn small_leaf_call_is_a_would_inline_candidate() {
    let leaf = alu_body("leaf", 3);
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.candidates, 1);
    assert_eq!(stats.would_inline, 1);
    assert_eq!(stats.skipped_callee_too_large, 0);
}

#[test]
fn class_prologue_callee_is_never_inlined() {
    // r505 (A12) — small enough to inline, refused by name: the
    // call is a link-judged site only while the body is a fn.
    let leaf = alu_body(torajs_core::ast::CLASS_PROLOGUE_FN, 3);
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.candidates, 1);
    assert_eq!(stats.would_inline, 0);
    assert_eq!(stats.inlined, 0);
    assert_eq!(stats.skipped_link_judged_unit, 1);
}

#[test]
fn oversized_leaf_is_rejected_with_callee_too_large() {
    let leaf = alu_body("fat", 300);
    let call = || void_inst(InstKind::Call(FuncId(1), vec![]));
    let main = caller("main", vec![call(), call()]);
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.candidates, 2);
    assert_eq!(stats.would_inline, 0);
    assert_eq!(stats.skipped_callee_too_large, 2);
}

#[test]
fn oversized_leaf_with_one_call_site_inlines() {
    let leaf = alu_body("fat", 300);
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.would_inline, 1);
    assert_eq!(stats.inlined, 1);
}

/// A sole-site user function's one call spliced: its out-of-line
/// body is gone (a declaration nothing calls), so the dump shows
/// the live copy only.
#[test]
fn spliced_sole_site_user_body_is_dropped() {
    let leaf = alu_body("loopSum", 8);
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.inlined, 1);
    assert_eq!(stats.sole_bodies_dropped, 1);
    assert!(m.funcs[1].is_declaration(), "the spliced body is dropped");
    assert_eq!(
        m.funcs[0].blocks[0].insts.len(),
        8,
        "the live copy stays in main"
    );
}

/// A callee's throw mark costs nothing and leaves with its `Ret`: the
/// spliced copy falls through into the caller instead of returning.
#[test]
fn spliced_body_drops_the_throw_mark() {
    let mut leaf = alu_body("loopSum", 8);
    leaf.blocks[0].insts.push(void_inst(InstKind::ThrowSignal));
    assert_eq!(callee_body_cost(&leaf), Cost::new(8));
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    assert_eq!(inline_module(&mut m).inlined, 1);
    let insts = &m.funcs[0].blocks[0].insts;
    assert_eq!(insts.len(), 8);
    assert!(!insts.iter().any(|i| i.kind == InstKind::ThrowSignal));
}

/// A callee that parks keeps its body and its call: the splice would
/// move the suspension into a function without its frame. The parking
/// function itself still inlines its own callees.
#[test]
fn suspending_callee_is_not_inlined() {
    let mut parks = alu_body("step", 2);
    let v = ValueId(parks.values.len() as u32);
    parks.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    parks.blocks[0].insts.push(val_inst(
        v,
        InstKind::Suspend(
            SuspendKind::Await,
            SuspendLane::Direct,
            Operand::ConstI64(0),
            Vec::new(),
        ),
    ));
    parks.blocks[0]
        .insts
        .push(void_inst(InstKind::Call(FuncId(2), vec![])));
    let leaf = alu_body("leaf", 2);
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, parks, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.skipped_suspends, 1);
    assert_eq!(stats.inlined, 1, "step inlines leaf");
    assert!(!m.funcs[1].is_declaration());
    let main_insts = &m.funcs[0].blocks[0].insts;
    assert_eq!(main_insts.len(), 1);
    assert!(matches!(main_insts[0].kind, InstKind::Call(FuncId(1), _)));
    let step_insts = &m.funcs[1].blocks[0].insts;
    assert!(
        step_insts
            .iter()
            .any(|i| matches!(i.kind, InstKind::Suspend(..)))
    );
    assert!(
        !step_insts
            .iter()
            .any(|i| matches!(i.kind, InstKind::Call(..)))
    );
}

/// A compiler-minted function (`__` prefix) may sit in a metadata
/// table by fid, so its body stays even when its one call splices.
#[test]
fn spliced_sole_site_minted_body_stays() {
    let leaf = alu_body("__boxed_leaf", 8);
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.inlined, 1);
    assert_eq!(stats.sole_bodies_dropped, 0);
    assert!(!m.funcs[1].is_declaration());
}

#[test]
fn address_taken_leaf_keeps_the_ceiling() {
    let leaf = alu_body("fat", 300);
    let main = caller(
        "main",
        vec![
            void_inst(InstKind::Call(FuncId(1), vec![])),
            val_inst(ValueId(90), InstKind::FnAddr(FuncId(1))),
        ],
    );
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.skipped_callee_too_large, 1);
}

#[test]
fn declaration_callee_is_skipped() {
    let leaf = declaration("extern_intrinsic");
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.candidates, 1);
    assert_eq!(stats.would_inline, 0);
    assert_eq!(stats.skipped_declaration, 1);
}

#[test]
fn callee_grown_within_pass_is_priced_at_its_grown_cost() {
    // funcs[0] = b: 150 ALU + 1 self-call (cost 152 ≤ 225). As a
    // caller it self-inlines first (bottom-up: before a) and
    // roughly doubles; its cost is re-priced then. funcs[1] = a:
    // calls b — the classifier sees the grown body and skips.
    let mut b = alu_body("b", 150);
    b.blocks[0]
        .insts
        .push(void_inst(InstKind::Call(FuncId(0), vec![])));
    b.blocks[0].term = Terminator::Ret(None);
    let a = caller("a", vec![void_inst(InstKind::Call(FuncId(0), vec![]))]);
    let mut m = module_of(vec![b, a]);
    // depth 1 so b's self-site fires and grows b within the pass
    let budget = InlineBudget {
        max_recursion_depth: 1,
        ..InlineBudget::default()
    };
    let stats = inline_module_with_budget(&mut m, budget);
    assert_eq!(
        (stats.skipped_callee_too_large, stats.skipped_live_cost),
        (1, 0),
        "a's site must be refused at b's re-priced cost"
    );
    let b_cost = callee_body_cost(&m.funcs[0]);
    assert!(
        b_cost > Cost::new(225),
        "precondition: b must have grown past the ceiling (got {b_cost:?})"
    );
    // a's body is untouched — its call to b survives
    assert!(matches!(
        m.funcs[1].blocks[0].insts[0].kind,
        InstKind::Call(FuncId(0), _)
    ));
}

#[test]
fn indirect_call_is_skipped_without_consuming_candidate_slot() {
    let main = caller(
        "main",
        vec![void_inst(InstKind::CallIndirect(
            SigId(0),
            Operand::ConstI64(0),
            vec![],
        ))],
    );
    let mut m = module_of(vec![main]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.candidates, 0);
    assert_eq!(stats.skipped_indirect, 1);
    assert_eq!(stats.would_inline, 0);
}

#[test]
fn caller_budget_exhausts_after_repeated_inlines() {
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
        ..InlineBudget::default()
    };
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module_with_budget(&mut m, budget);
    assert_eq!(stats.candidates, 3);
    assert_eq!(stats.would_inline, 2);
    assert_eq!(stats.skipped_caller_budget, 1);
}

// ---- Phase 1.0b emit-side tests ----

#[test]
fn single_block_leaf_call_is_actually_inlined() {
    // Leaf: 4 ALU adds, void return. Caller: one void call site.
    // Expected after Phase 1.0b: main's block body grows from 1
    // (the call inst) to 4 (the 4 inlined add insts), and the
    // call instruction itself is gone.
    let leaf = alu_body("leaf", 4);
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.would_inline, 1);
    assert_eq!(stats.inlined, 1, "single-block leaf passes splice");
    assert_eq!(
        m.funcs[0].blocks[0].insts.len(),
        4,
        "call inst replaced by 4 leaf adds"
    );
    for inst in &m.funcs[0].blocks[0].insts {
        assert!(
            matches!(inst.kind, InstKind::BinOp(BinOp::Add, _, _)),
            "every inlined inst is the leaf Add"
        );
    }
}

#[test]
fn multi_block_void_callee_takes_phase_2_splice_fallback() {
    // Multi-block void leaf — cheap by cost (no body work), so the
    // classifier produces a would_inline decision. The Phase 1.0a
    // splice rejects it with NotSingleBlock, then the driver falls
    // through to Phase 2.0a multi-block splice which succeeds.
    // Expect inlined_multi_block = 1 and the caller block to gain
    // the cloned callee blocks + a continuation block.
    let multi = Function {
        name: "two_blocks".into(),
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
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, multi]);
    let stats = inline_module(&mut m);
    assert_eq!(stats.would_inline, 1);
    assert_eq!(stats.inlined, 1, "Phase 2.0a multi-block splice ok");
    assert_eq!(stats.inlined_multi_block, 1);
    assert_eq!(stats.inlined_single_block, 0);
    // Caller's main function: original block 0 (now Br to cloned
    // entry) + 2 cloned callee blocks + 1 continuation = 4 blocks.
    assert_eq!(m.funcs[0].blocks.len(), 4);
    assert_eq!(
        m.funcs[0].blocks[0].insts.len(),
        0,
        "original call inst replaced; pre-call insts empty"
    );
}

#[test]
fn dry_run_env_gate_skips_emit_but_classifies() {
    // TORAJS_INLINER_OFF=1 → stats still reflect classification,
    // but no module mutation happens (inlined stays 0). Mirrors
    // the existing TORAJS_EGRAPH_OFF gate pattern in lib.rs::tests.
    //
    // SAFETY: env mutation is process-global; we set+remove in a
    // controlled scope and rely on cargo nextest's per-test
    // process isolation.
    unsafe {
        std::env::set_var("TORAJS_INLINER_OFF", "1");
    }
    let leaf = alu_body("leaf", 4);
    let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
    let mut m = module_of(vec![main, leaf]);
    let before_inst_count = m.funcs[0].blocks[0].insts.len();
    let stats = inline_module(&mut m);
    unsafe {
        std::env::remove_var("TORAJS_INLINER_OFF");
    }
    assert_eq!(stats.would_inline, 1, "classification still happens");
    assert_eq!(stats.inlined, 0, "dry-run does not emit");
    assert_eq!(
        m.funcs[0].blocks[0].insts.len(),
        before_inst_count,
        "module unchanged in dry-run"
    );
}
