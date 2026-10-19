//! Split shapes on hand-built resume functions: what crosses each
//! suspension, where it lives in the frame, how the function parks,
//! lands and ends.

use super::*;
use torajs_core::ssa::suspend::{FRAME_AWAITING_OFF, FRAME_STATE_OFF};
use torajs_core::ssa::{
    BinOp, FuncId, IPred, Inst, SuspendKind, SuspendLane, Terminator, Type, visit_value_operands,
};

const RT: SuspendRuntime = SuspendRuntime {
    trace_links: FuncId(90),
    visit_sig: torajs_core::ssa::SigId(0),
    await_promise: FuncId(91),
    await_direct: FuncId(92),
    ret: FuncId(93),
    throw: FuncId(94),
    obj_alloc: FuncId(95),
    obj_drop_sized: FuncId(96),
};

fn sfn(nparams: u32, ret_repr: i64) -> SuspendFn {
    SuspendFn {
        wrapper: FuncId(0),
        resume: FuncId(1),
        drop: FuncId(2),
        trace: FuncId(3),
        traced_params: Vec::new(),
        desc_sym: "__frame_desc_0".into(),
        size_placeholder: torajs_core::ssa::suspend::FRAME_SIZE_PLACEHOLDER_BASE,
        nparams,
        ret_repr,
    }
}

/// `(frame)`: its parameters read from the frame on entry, as the
/// lowering leaves them.
fn body(ret: Type, params: &[Type]) -> (Function, ValueId, Vec<ValueId>) {
    let mut f = Function::new("__async_resume_t", ret);
    let frame = f.add_param(Type::Ptr, "__frame");
    let b = f.add_block();
    let ps = (params.iter().enumerate())
        .map(|(i, &t)| {
            f.append_inst(
                b,
                InstKind::Load(t, Operand::Value(frame), FRAME_PARAMS_OFF + 8 * i as u64),
                t,
                None,
            )
        })
        .collect();
    (f, frame, ps)
}

fn calls_to(f: &Function, fid: FuncId) -> usize {
    (f.blocks.iter().flat_map(|b| &b.insts))
        .filter(|i| matches!(&i.kind, InstKind::Call(g, _) if *g == fid))
        .count()
}

fn frame_stores(f: &Function, frame: ValueId) -> Vec<u64> {
    let mut v: Vec<u64> = (f.blocks.iter().flat_map(|b| &b.insts))
        .filter_map(|i| match i.kind {
            InstKind::Store(_, Operand::Value(p), off) if p == frame => Some(off),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

/// `async (i) => { const t = i * 3; await null; return t + i }`: the
/// parameter is read again from the frame, the product crosses in a
/// frame word, the body parks once and returns through the runtime.
#[test]
fn a_direct_await_carries_a_computed_value_and_rereads_the_parameter() {
    let (mut f, frame, ps) = body(Type::I64, &[Type::I64]);
    let i = ps[0];
    let b = f.blocks[0].id;
    let t = f.append_inst(
        b,
        InstKind::BinOp(BinOp::Mul, Operand::Value(i), Operand::ConstI64(3)),
        Type::I64,
        None,
    );
    f.append_void(
        b,
        InstKind::Suspend(
            SuspendKind::Await,
            SuspendLane::Direct,
            Operand::ConstI64(0),
            Vec::new(),
        ),
    );
    let s = f.append_inst(
        b,
        InstKind::BinOp(BinOp::Add, Operand::Value(t), Operand::Value(i)),
        Type::I64,
        None,
    );
    f.set_term(b, Terminator::Ret(Some(Operand::Value(s))));
    let (words, size) = split_function(&mut f, &sfn(1, 1), &RT);
    assert_eq!(words.len(), 1);
    // header, one parameter word, one cell for `t`
    assert_eq!(size, FRAME_PARAMS_OFF + 16);
    assert_eq!(f.ret, Type::Void);
    assert_eq!(calls_to(&f, RT.await_direct), 1);
    assert_eq!(calls_to(&f, RT.ret), 1);
    // the park records the point and spills `t`
    assert_eq!(
        frame_stores(&f, frame),
        vec![FRAME_STATE_OFF, FRAME_PARAMS_OFF + 8]
    );
    // the entry reads the state and picks the start or the landing
    let entry = &f.blocks[0];
    assert!(matches!(
        entry.insts.last().unwrap().kind,
        InstKind::ICmp(IPred::Eq, _, Operand::ConstI64(0))
    ));
    assert!(matches!(entry.term, Terminator::CondBr { .. }));
    // the landing reads the parameter again rather than spilling it
    let param_reads = (f.blocks.iter().flat_map(|b| &b.insts))
        .filter(|i| {
            matches!(i.kind, InstKind::Load(_, Operand::Value(p), off)
            if p == frame && off == FRAME_PARAMS_OFF)
        })
        .count();
    assert_eq!(param_reads, 2);
}

/// A promise suspension's result is the frame's awaiting word, read
/// and cleared at the landing, and the cell the operand came from does
/// not cross.
#[test]
fn a_promise_await_lands_with_the_settled_cell() {
    let (mut f, frame, ps) = body(Type::Void, &[Type::Promise]);
    let b = f.blocks[0].id;
    let settled = f.append_inst(
        b,
        InstKind::Suspend(
            SuspendKind::Await,
            SuspendLane::Promise(Type::I64),
            Operand::Value(ps[0]),
            Vec::new(),
        ),
        Type::Promise,
        None,
    );
    f.append_void(b, InstKind::Call(FuncId(50), vec![Operand::Value(settled)]));
    f.set_term(b, Terminator::Ret(None));
    let (_, size) = split_function(&mut f, &sfn(1, 7), &RT);
    assert_eq!(size, FRAME_PARAMS_OFF + 8, "nothing but the parameter");
    assert_eq!(calls_to(&f, RT.await_promise), 1);
    let landing = (f.blocks.iter())
        .find(|b| (b.insts.iter()).any(|i| i.result == Some(settled)))
        .unwrap();
    assert!(matches!(
        landing.insts[0].kind,
        InstKind::Load(Type::Promise, Operand::Value(p), FRAME_AWAITING_OFF) if p == frame
    ));
    assert!(matches!(
        landing.insts[1].kind,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(p), FRAME_AWAITING_OFF) if p == frame
    ));
}

/// A buffer whose address is handed to a call lives in the frame: its
/// alloca is gone and every use is the frame word's address.
#[test]
fn an_escaping_buffer_moves_into_the_frame() {
    let (mut f, frame, _) = body(Type::Void, &[]);
    let b = f.blocks[0].id;
    let buf = f.append_inst(b, InstKind::AllocaBytes(20), Type::Ptr, None);
    f.append_void(b, InstKind::Call(FuncId(50), vec![Operand::Value(buf)]));
    f.append_void(
        b,
        InstKind::Suspend(
            SuspendKind::Await,
            SuspendLane::Direct,
            Operand::ConstI64(0),
            Vec::new(),
        ),
    );
    let v = f.append_inst(
        b,
        InstKind::Load(Type::I64, Operand::Value(buf), 8),
        Type::I64,
        None,
    );
    f.append_void(b, InstKind::Call(FuncId(50), vec![Operand::Value(v)]));
    f.set_term(b, Terminator::Ret(None));
    let (_, size) = split_function(&mut f, &sfn(0, 7), &RT);
    assert_eq!(
        size,
        FRAME_PARAMS_OFF + 24,
        "20 bytes round up to three words"
    );
    let all: Vec<&Inst> = f.blocks.iter().flat_map(|b| &b.insts).collect();
    assert!(
        !all.iter()
            .any(|i| matches!(i.kind, InstKind::AllocaBytes(_)))
    );
    assert!(all.iter().any(|i| matches!(i.kind,
        InstKind::Load(Type::I64, Operand::Value(p), off) if p == frame && off == FRAME_PARAMS_OFF + 8)));
}

/// A value defined before a loop holding a suspension and read after
/// it reaches its use from both the loop's entry and its landing: it
/// crosses through its slot, and every read of it outside its block is
/// a slot read the verifier accepts.
#[test]
fn a_value_read_after_a_loop_with_a_suspension_crosses_once() {
    let (mut f, frame, _) = body(Type::I64, &[]);
    let entry = f.blocks[0].id;
    let head = f.add_block();
    let looped = f.add_block();
    let exit = f.add_block();
    let a = f.append_inst(entry, InstKind::Call(FuncId(51), vec![]), Type::I64, None);
    f.set_term(entry, Terminator::Br(head));
    let c = f.append_inst(head, InstKind::Call(FuncId(52), vec![]), Type::Bool, None);
    f.set_term(
        head,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk: looped,
            else_blk: exit,
        },
    );
    f.append_void(
        looped,
        InstKind::Suspend(
            SuspendKind::Await,
            SuspendLane::Direct,
            Operand::ConstI64(0),
            Vec::new(),
        ),
    );
    f.set_term(looped, Terminator::Br(head));
    f.set_term(exit, Terminator::Ret(Some(Operand::Value(a))));
    let (_, size) = split_function(&mut f, &sfn(0, 1), &RT);
    assert_eq!(size, FRAME_PARAMS_OFF + 8);
    // `a` is read only through its slot outside the entry
    for b in &f.blocks {
        for i in &b.insts {
            visit_value_operands(&i.kind, |v| {
                assert!(v != a || (b.insts.iter()).any(|d| d.result == Some(a)));
            });
        }
    }
    assert_eq!(
        frame_stores(&f, frame),
        vec![FRAME_STATE_OFF, FRAME_PARAMS_OFF]
    );
}

/// A block that propagates a throw rejects the result instead of
/// signalling it.
#[test]
fn a_propagating_return_rejects_the_result() {
    let (mut f, _, _) = body(Type::I64, &[]);
    let b = f.blocks[0].id;
    let thrown = f.add_block();
    f.append_void(
        b,
        InstKind::Suspend(
            SuspendKind::Await,
            SuspendLane::Direct,
            Operand::ConstI64(0),
            Vec::new(),
        ),
    );
    f.set_term(b, Terminator::Br(thrown));
    f.append_void(thrown, InstKind::ThrowSignal);
    f.set_term(thrown, Terminator::Ret(Some(Operand::ConstI64(0))));
    split_function(&mut f, &sfn(0, 1), &RT);
    assert_eq!(calls_to(&f, RT.throw), 1);
    assert_eq!(calls_to(&f, RT.ret), 0);
}

#[test]
fn the_sizes_reach_the_allocation_and_the_drop_only() {
    let s = sfn(0, 1);
    let mut alloc = Function::new("w", Type::Void);
    let b = alloc.add_block();
    let ph = Operand::ConstI64(s.size_placeholder);
    alloc.append_inst(b, InstKind::Call(RT.obj_alloc, vec![ph]), Type::Ptr, None);
    let p = alloc.add_param(Type::Ptr, "p");
    alloc.append_void(b, InstKind::Store(ph, Operand::Value(p), 0));
    alloc.set_term(b, Terminator::Ret(None));
    let mut drop = alloc.clone();
    drop.name = "d".into();
    let mut m = Module {
        funcs: vec![alloc, Function::new("r", Type::Void), drop],
        ..Default::default()
    };
    let sizes: HashMap<i64, i64> = [(s.size_placeholder, 64)].into_iter().collect();
    let drops: HashSet<u32> = [2].into_iter().collect();
    patch_sizes(&mut m, &RT, &sizes, &drops);
    let arg = |f: usize, i: usize| match &m.funcs[f].blocks[0].insts[i].kind {
        InstKind::Call(_, a) => a[0],
        InstKind::Store(v, _, _) => *v,
        _ => unreachable!(),
    };
    assert_eq!(arg(0, 0), Operand::ConstI64(64));
    assert_eq!(arg(0, 1), ph, "a store outside the drop is not a size");
    assert_eq!(arg(2, 1), Operand::ConstI64(64));
}

fn direct(held: Vec<Operand>) -> InstKind {
    InstKind::Suspend(
        SuspendKind::Await,
        SuspendLane::Direct,
        Operand::ConstI64(0),
        held,
    )
}

/// `let s = make(); await null; use(s)` with `s` in a stack cell; the
/// suspension holds `s` as the lowering read it, or as the e-graph
/// forwarded the stored value; `rewritten` stores another value in the
/// cell between that read and the suspension.
fn held_local(forwarded: bool, rewritten: bool) -> Vec<Vec<u64>> {
    let (mut f, _, _) = body(Type::Void, &[]);
    let b = f.blocks[0].id;
    let slot = f.append_inst(b, InstKind::Alloca(Type::Str), Type::Ptr, None);
    let s = f.append_inst(b, InstKind::Call(FuncId(51), vec![]), Type::Str, None);
    f.append_void(
        b,
        InstKind::Store(Operand::Value(s), Operand::Value(slot), 0),
    );
    let held = if forwarded {
        s
    } else {
        let read = InstKind::Load(Type::Str, Operand::Value(slot), 0);
        f.append_inst(b, read, Type::Str, None)
    };
    if rewritten {
        let other = f.append_inst(b, InstKind::Call(FuncId(52), vec![]), Type::Str, None);
        f.append_void(
            b,
            InstKind::Store(Operand::Value(other), Operand::Value(slot), 0),
        );
    }
    f.append_void(b, direct(vec![Operand::Value(held)]));
    let read = InstKind::Load(Type::Str, Operand::Value(slot), 0);
    let after = f.append_inst(b, read, Type::Str, None);
    f.append_void(b, InstKind::Call(FuncId(50), vec![Operand::Value(after)]));
    f.set_term(b, Terminator::Ret(None));
    split_function(&mut f, &sfn(0, 7), &RT).0
}

/// A value the body owns at a point is reported from the frame word of
/// the cell it crosses in, whether the lowering's read of the cell
/// survived or the e-graph forwarded the stored value to it; a cell
/// written after the read holds something else and is not reported.
#[test]
fn a_held_value_is_found_in_the_frame_word_of_its_cell() {
    assert_eq!(held_local(false, false), vec![vec![FRAME_PARAMS_OFF]]);
    assert_eq!(held_local(true, false), vec![vec![FRAME_PARAMS_OFF]]);
    assert_eq!(held_local(false, true), vec![Vec::<u64>::new()]);
}

/// The trace reads the state and visits each point's words, tagged.
#[test]
fn the_trace_visits_the_words_of_the_parked_point() {
    use torajs_core::ssa::suspend::TRACE_STATE_WORD;
    let mut t = Function::new("__async_trace_t", Type::Void);
    for n in ["frame", "visit", "ctx"] {
        t.add_param(Type::Ptr, n);
    }
    let b = t.add_block();
    t.set_term(b, Terminator::Ret(None));
    trace::add_points(&mut t, &[vec![], vec![64, 72]], RT.visit_sig);
    let idx: Vec<Operand> = (t.blocks.iter().flat_map(|b| &b.insts))
        .filter_map(|i| match &i.kind {
            InstKind::CallIndirect(_, _, a) => Some(a[0]),
            _ => None,
        })
        .collect();
    let tagged = |w: i64| Operand::ConstI64(TRACE_STATE_WORD | w);
    assert_eq!(idx, vec![tagged(8), tagged(9)]);
    assert!(matches!(t.blocks[0].term, Terminator::CondBr { .. }));
}

/// Past a few points the entry finds its landing by binary search.
#[test]
fn many_points_dispatch_through_a_binary_search() {
    let (mut f, _, _) = body(Type::Void, &[]);
    let b = f.blocks[0].id;
    for _ in 0..9 {
        f.append_void(b, direct(Vec::new()));
    }
    f.set_term(b, Terminator::Ret(None));
    let (words, _) = split_function(&mut f, &sfn(0, 7), &RT);
    assert_eq!(words.len(), 9);
    let compares = |p: IPred| {
        (f.blocks.iter().flat_map(|b| &b.insts))
            .filter(|i| matches!(i.kind, InstKind::ICmp(q, ..) if q == p))
            .count()
    };
    // state 0 first; the nine landings split 4 | 5, the five 2 | 3,
    // and each part is a chain
    assert_eq!(compares(IPred::Slt), 2);
    assert_eq!(compares(IPred::Eq), 1 + 3 + 1 + 2);
}

/// `s` in a cell whose address escapes (so the cell lives in the
/// frame), read and held at the suspension; `called` makes a call
/// between the read and the park, which may write through the address.
fn held_in_moved_cell(called: bool) -> Vec<Vec<u64>> {
    let (mut f, _, _) = body(Type::Void, &[]);
    let b = f.blocks[0].id;
    let slot = f.append_inst(b, InstKind::Alloca(Type::Str), Type::Ptr, None);
    let s = f.append_inst(b, InstKind::Call(FuncId(51), vec![]), Type::Str, None);
    f.append_void(
        b,
        InstKind::Store(Operand::Value(s), Operand::Value(slot), 0),
    );
    f.append_void(b, InstKind::Call(FuncId(53), vec![Operand::Value(slot)]));
    let read = InstKind::Load(Type::Str, Operand::Value(slot), 0);
    let held = f.append_inst(b, read, Type::Str, None);
    if called {
        f.append_void(b, InstKind::Call(FuncId(54), vec![]));
    }
    f.append_void(b, direct(vec![Operand::Value(held)]));
    f.set_term(b, Terminator::Ret(None));
    split_function(&mut f, &sfn(0, 7), &RT).0
}

/// A value read from a cell that lives in the frame is reported from
/// that word only when nothing between the read and the park may write
/// it: a call may, through the address the cell gave out.
#[test]
fn a_frame_cell_written_after_the_read_is_not_reported() {
    assert_eq!(held_in_moved_cell(false), vec![vec![FRAME_PARAMS_OFF]]);
    assert_eq!(held_in_moved_cell(true), vec![Vec::<u64>::new()]);
}

/// `s` stored into a cell on one arm of a branch only, the cell holding
/// another value on the other arm, and held at a suspension after the
/// join: the store does not dominate the park, so the cell's word is
/// not known to hold `s` there and is not reported.
#[test]
fn a_store_on_one_arm_does_not_report_the_cell() {
    let (mut f, _, _) = body(Type::Void, &[]);
    let entry = f.blocks[0].id;
    let (arm, join) = (f.add_block(), f.add_block());
    let slot = f.append_inst(entry, InstKind::Alloca(Type::Str), Type::Ptr, None);
    let other = f.append_inst(entry, InstKind::Call(FuncId(52), vec![]), Type::Str, None);
    f.append_void(
        entry,
        InstKind::Store(Operand::Value(other), Operand::Value(slot), 0),
    );
    let s = f.append_inst(entry, InstKind::Call(FuncId(51), vec![]), Type::Str, None);
    let c = f.append_inst(entry, InstKind::Call(FuncId(55), vec![]), Type::Bool, None);
    f.set_term(
        entry,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk: arm,
            else_blk: join,
        },
    );
    f.append_void(
        arm,
        InstKind::Store(Operand::Value(s), Operand::Value(slot), 0),
    );
    f.set_term(arm, Terminator::Br(join));
    f.append_void(join, direct(vec![Operand::Value(s)]));
    let read = InstKind::Load(Type::Str, Operand::Value(slot), 0);
    let after = f.append_inst(join, read, Type::Str, None);
    f.append_void(
        join,
        InstKind::Call(FuncId(50), vec![Operand::Value(after)]),
    );
    f.set_term(join, Terminator::Ret(None));
    let (words, _) = split_function(&mut f, &sfn(0, 7), &RT);
    assert_eq!(words, vec![Vec::<u64>::new()]);
}
