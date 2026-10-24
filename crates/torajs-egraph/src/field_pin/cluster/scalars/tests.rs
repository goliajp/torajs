use super::*;
use torajs_core::ssa::{BinOp, IPred, StructId, Terminator};

const POS: u64 = 40;
const FIELDS: [(u64, Type); 1] = [(POS, Type::I64)];
const MEMBER: FuncId = FuncId(7);

fn v(x: ValueId) -> Operand {
    Operand::Value(x)
}

/// A body on receiver `%this`: `%r = this.pos`, then `between`, then
/// `this.pos = %r + 1`, returning `%r` when `ret_read`.
fn body_fn(between: impl Fn(&mut Function, BlockId, ValueId), ret_read: bool) -> Function {
    let mut f = Function::new(".L__cm_P__e.pin", Type::I64);
    let this = f.add_param(Type::Obj(StructId(0)), "this");
    let b = f.add_block();
    let r = f.append_inst(b, InstKind::Load(Type::I64, v(this), POS), Type::I64, None);
    between(&mut f, b, this);
    let add = InstKind::BinOp(BinOp::Add, v(r), Operand::ConstI64(1));
    let n = f.append_inst(b, add, Type::I64, None);
    f.append_void(b, InstKind::Store(v(n), v(this), POS));
    let ret = if ret_read { v(r) } else { Operand::ConstI64(0) };
    f.set_term(b, Terminator::Ret(Some(ret)));
    f
}

fn kinds(f: &Function) -> Vec<&InstKind> {
    f.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .map(|i| &i.kind)
        .collect()
}

fn writes() -> HashSet<FuncId> {
    [MEMBER].into_iter().collect()
}

#[test]
fn a_read_and_a_write_become_the_register_without_a_copy() {
    let mut f = body_fn(|_, _, _| {}, false);
    let r = body(&mut f, &FIELDS, &writes(), false);
    assert_eq!((r.accesses, r.copies), (2, 0));
    let k = kinds(&f);
    assert_eq!(k[0], &InstKind::Pinned(PinWord::Scalar(0)));
    assert!(matches!(k[1], InstKind::BinOp(BinOp::Add, ..)));
    assert!(matches!(k[2], InstKind::PinSet(PinWord::Scalar(0), _)));
    assert_eq!(k.len(), 3);
}

#[test]
fn a_read_used_past_its_write_reads_a_copy() {
    let mut f = body_fn(|_, _, _| {}, true);
    let r = body(&mut f, &FIELDS, &writes(), false);
    assert_eq!(r.copies, 1);
    let k = kinds(&f);
    let p = f.blocks[0].insts[0].result.unwrap();
    assert_eq!(k[1], &InstKind::Copy(Type::I64, v(p)));
    let c = f.blocks[0].insts[1].result.unwrap();
    // the add, before the write, still reads the register
    assert!(matches!(k[2], InstKind::BinOp(_, Operand::Value(x), _) if *x == p));
    assert_eq!(f.blocks[0].term, Terminator::Ret(Some(v(c))));
}

#[test]
fn a_read_used_past_a_call_to_a_member_reads_a_copy() {
    let call = |f: &mut Function, b, this| {
        f.append_void(b, InstKind::Call(MEMBER, vec![v(this)]));
    };
    let mut f = body_fn(call, false);
    let r = body(&mut f, &FIELDS, &writes(), false);
    assert_eq!(r.copies, 1);
    let k = kinds(&f);
    assert!(matches!(k[1], InstKind::Copy(..)));
    let c = f.blocks[0].insts[1].result.unwrap();
    // the call comes before the add, which now reads the copy
    assert!(matches!(k[2], InstKind::Call(..)));
    assert!(matches!(k[3], InstKind::BinOp(_, Operand::Value(x), _) if *x == c));
}

/// `%r = this.pos` in bb0, a branch to bb1 or bb2, both to bb3, which
/// returns `%r + 1`; bb2 writes the scalar when `write`.
fn diamond(write: bool) -> Function {
    let mut f = Function::new(".L__cm_P__e.pin", Type::I64);
    let this = f.add_param(Type::Obj(StructId(0)), "this");
    let [b0, b1, b2, b3] = [(); 4].map(|_| f.add_block());
    let r = f.append_inst(b0, InstKind::Load(Type::I64, v(this), POS), Type::I64, None);
    let c = InstKind::ICmp(IPred::Eq, v(r), Operand::ConstI64(0));
    let c = f.append_inst(b0, c, Type::Bool, None);
    let cond = Terminator::CondBr {
        cond: v(c),
        then_blk: b1,
        else_blk: b2,
    };
    f.set_term(b0, cond);
    f.set_term(b1, Terminator::Br(b3));
    if write {
        f.append_void(b2, InstKind::Store(Operand::ConstI64(9), v(this), POS));
    }
    f.set_term(b2, Terminator::Br(b3));
    let add = InstKind::BinOp(BinOp::Add, v(r), Operand::ConstI64(1));
    let s = f.append_inst(b3, add, Type::I64, None);
    f.set_term(b3, Terminator::Ret(Some(v(s))));
    f
}

#[test]
fn a_read_used_in_another_block_no_write_reaches_keeps_the_register() {
    let mut f = diamond(false);
    assert_eq!(body(&mut f, &FIELDS, &writes(), false).copies, 0);
    let p = f.blocks[0].insts[0].result.unwrap();
    assert!(
        matches!(f.blocks[3].insts[0].kind, InstKind::BinOp(_, Operand::Value(x), _) if x == p)
    );
}

#[test]
fn a_read_used_where_one_path_wrote_the_scalar_reads_a_copy() {
    let mut f = diamond(true);
    assert_eq!(body(&mut f, &FIELDS, &writes(), false).copies, 1);
    let c = f.blocks[0].insts[1].result.unwrap();
    assert!(matches!(f.blocks[0].insts[1].kind, InstKind::Copy(..)));
    assert!(
        matches!(f.blocks[3].insts[0].kind, InstKind::BinOp(_, Operand::Value(x), _) if x == c)
    );
    // the compare in bb0, before any write, still reads the register
    let p = f.blocks[0].insts[0].result.unwrap();
    assert!(matches!(f.blocks[0].insts[2].kind, InstKind::ICmp(_, Operand::Value(x), _) if x == p));
}

#[test]
fn a_call_to_something_else_keeps_the_register() {
    let call = |f: &mut Function, b, this| {
        f.append_void(b, InstKind::Call(FuncId(9), vec![v(this)]));
    };
    let mut f = body_fn(call, false);
    assert_eq!(body(&mut f, &FIELDS, &writes(), false).copies, 0);
}

#[test]
fn a_load_into_a_value_with_other_defs_copies_the_register() {
    let mut f = body_fn(|_, _, _| {}, false);
    let r = f.blocks[0].insts[0].result.unwrap();
    let other = Inst {
        result: Some(r),
        kind: InstKind::Copy(Type::I64, Operand::ConstI64(3)),
        origin: None,
    };
    f.blocks[0].insts.insert(0, other);
    body(&mut f, &FIELDS, &writes(), false);
    let k = kinds(&f);
    assert_eq!(k[1], &InstKind::Pinned(PinWord::Scalar(0)));
    let p = f.blocks[0].insts[1].result.unwrap();
    assert_eq!(k[2], &InstKind::Copy(Type::I64, v(p)));
    assert_eq!(f.blocks[0].insts[2].result, Some(r));
}

#[test]
fn another_cell_or_field_stays_in_memory() {
    let mut f = body_fn(|_, _, _| {}, false);
    let other = f.add_param(Type::Obj(StructId(0)), "q");
    f.blocks[0].insts[0].kind = InstKind::Load(Type::I64, v(other), POS);
    let this = f.params[0];
    f.blocks[0].insts.push(Inst {
        result: None,
        kind: InstKind::Store(Operand::ConstI64(1), v(this), POS + 8),
        origin: None,
    });
    let r = body(&mut f, &FIELDS, &writes(), false);
    assert_eq!(r.accesses, 1);
    assert!(matches!(kinds(&f)[0], InstKind::Load(..)));
}

#[test]
fn an_entry_loads_before_the_call_and_stores_back_before_the_probe() {
    let mut f = Function::new("__cm_P__e", Type::I64);
    let this = f.add_param(Type::Obj(StructId(0)), "this");
    let b = f.add_block();
    let r = f.append_inst(b, InstKind::Call(MEMBER, vec![v(this)]), Type::I64, None);
    let g = InstKind::GlobalRef("flag".into());
    f.append_inst(b, g, Type::Ptr, None);
    f.set_term(b, Terminator::Ret(Some(v(r))));
    entry(&mut f, &FIELDS);
    let k = kinds(&f);
    assert_eq!(k[0], &InstKind::Load(Type::I64, v(this), POS));
    let l = f.blocks[0].insts[0].result.unwrap();
    assert_eq!(k[1], &InstKind::PinSet(PinWord::Scalar(0), v(l)));
    assert!(matches!(k[2], InstKind::Call(..)));
    assert_eq!(k[3], &InstKind::Pinned(PinWord::Scalar(0)));
    let q = f.blocks[0].insts[3].result.unwrap();
    assert_eq!(k[4], &InstKind::Store(v(q), v(this), POS));
    assert!(matches!(k[5], InstKind::GlobalRef(_)));
}

#[test]
fn strict_keeps_the_store_and_traps_on_a_read_memory_disagrees_with() {
    let mut f = body_fn(|_, _, _| {}, false);
    body(&mut f, &FIELDS, &writes(), true);
    assert_eq!(f.blocks.len(), 3);
    let this = f.params[0];
    let head = kinds(&f);
    assert_eq!(head[0], &InstKind::Pinned(PinWord::Scalar(0)));
    assert_eq!(head[1], &InstKind::Load(Type::I64, v(this), POS));
    assert!(matches!(head[2], InstKind::ICmp(IPred::Ne, ..)));
    assert!(matches!(f.blocks[0].term, Terminator::CondBr { .. }));
    assert_eq!(f.blocks[1].term, Terminator::Unreachable);
    let rest: Vec<&InstKind> = f.blocks[2].insts.iter().map(|i| &i.kind).collect();
    assert!(matches!(rest[1], InstKind::PinSet(..)));
    assert!(matches!(rest[2], InstKind::Store(..)));
}

/// bb0 jumps to bb1, which loops on itself and then returns; `head`
/// fills bb0 and `lp` fills bb1 before its branch.
fn looped(
    head: impl Fn(&mut Function, BlockId, ValueId),
    lp: impl Fn(&mut Function, BlockId, ValueId),
) -> Function {
    let mut f = Function::new(".L__cm_P__e.pin", Type::I64);
    let this = f.add_param(Type::Obj(StructId(0)), "this");
    let [b0, b1, b2] = [(); 3].map(|_| f.add_block());
    head(&mut f, b0, this);
    f.set_term(b0, Terminator::Br(b1));
    lp(&mut f, b1, this);
    let back = Terminator::CondBr {
        cond: Operand::ConstBool(true),
        then_blk: b1,
        else_blk: b2,
    };
    f.set_term(b1, back);
    f.set_term(b2, Terminator::Ret(None));
    f
}

#[test]
fn a_read_used_in_a_loop_that_writes_the_scalar_reads_a_copy() {
    let read = |f: &mut Function, b, this| {
        f.append_inst(b, InstKind::Load(Type::I64, v(this), POS), Type::I64, None);
    };
    let bump = |f: &mut Function, b, this| {
        let r = ValueId(1);
        let add = InstKind::BinOp(BinOp::Add, v(r), Operand::ConstI64(1));
        let n = f.append_inst(b, add, Type::I64, None);
        f.append_void(b, InstKind::Store(v(n), v(this), POS));
    };
    let mut f = looped(read, bump);
    assert_eq!(f.blocks[0].insts[0].result, Some(ValueId(1)));
    assert_eq!(body(&mut f, &FIELDS, &writes(), false).copies, 1);
    // the second time round, the add reads what the first one wrote
    let c = f.blocks[0].insts[1].result.unwrap();
    assert!(
        matches!(f.blocks[1].insts[0].kind, InstKind::BinOp(_, Operand::Value(x), _) if x == c)
    );
}

#[test]
fn a_use_ahead_of_the_read_in_its_own_looping_block_reads_a_copy() {
    let lp = |f: &mut Function, b, this| {
        // `%u = %r + 2` comes round the loop from the read below it
        let r = ValueId(2);
        let add = InstKind::BinOp(BinOp::Add, v(r), Operand::ConstI64(2));
        f.append_inst(b, add, Type::I64, None);
        f.append_inst(b, InstKind::Load(Type::I64, v(this), POS), Type::I64, None);
        f.append_void(b, InstKind::Store(Operand::ConstI64(5), v(this), POS));
    };
    let mut f = looped(|_, _, _| {}, lp);
    assert_eq!(f.blocks[1].insts[1].result, Some(ValueId(2)));
    assert_eq!(body(&mut f, &FIELDS, &writes(), false).copies, 1);
    let c = f.blocks[1].insts[2].result.unwrap();
    assert!(matches!(f.blocks[1].insts[2].kind, InstKind::Copy(..)));
    assert!(
        matches!(f.blocks[1].insts[0].kind, InstKind::BinOp(_, Operand::Value(x), _) if x == c)
    );
}

/// A body whose `%cur` is a copy of the receiver on one path and, on
/// the other, of another cell `q` when `other` (else the receiver
/// again), and that reads `cur.pos` where they meet.
fn merged(other: bool) -> (Function, ValueId) {
    let mut f = diamond(false);
    let this = f.params[0];
    let q = f.add_param(Type::Obj(StructId(0)), "q");
    let cell = Type::Obj(StructId(0));
    let cur = f.append_inst(BlockId(1), InstKind::Copy(cell, v(this)), cell, None);
    f.blocks[2].insts.push(Inst {
        result: Some(cur),
        kind: InstKind::Copy(cell, v(if other { q } else { this })),
        origin: None,
    });
    f.insert_inst(
        BlockId(3),
        0,
        InstKind::Load(Type::I64, v(cur), POS),
        Type::I64,
    );
    (f, cur)
}

#[test]
fn a_merge_of_copies_of_the_receiver_is_the_receiver() {
    let (mut f, cur) = merged(false);
    assert!(receivers(&f).contains(&cur));
    assert!(!foreign(&f, StructId(0), &FIELDS, &receivers(&f), None));
    assert_eq!(body(&mut f, &FIELDS, &writes(), false).accesses, 2);
}

#[test]
fn a_merge_with_another_cell_keeps_every_scalar_in_memory() {
    let (mut f, cur) = merged(true);
    assert!(!receivers(&f).contains(&cur));
    assert!(foreign(&f, StructId(0), &FIELDS, &receivers(&f), None));
    assert_eq!(body(&mut f, &FIELDS, &writes(), false).accesses, 1);
}

#[test]
fn a_cell_the_member_allocated_is_not_foreign() {
    let alloc = FuncId(3);
    let mut f = body_fn(|_, _, _| {}, false);
    let new = InstKind::Call(alloc, vec![Operand::ConstI64(48)]);
    let cell = f.append_inst(BlockId(0), new, Type::Obj(StructId(0)), None);
    f.append_void(
        BlockId(0),
        InstKind::Store(Operand::ConstI64(0), v(cell), POS),
    );
    assert!(!foreign(
        &f,
        StructId(0),
        &FIELDS,
        &receivers(&f),
        Some(alloc)
    ));
    assert!(foreign(&f, StructId(0), &FIELDS, &receivers(&f), None));
}

/// A body that also accesses `cell.pos`, or `cell` at a computed
/// offset, through `access`.
fn also(access: impl Fn(&mut Function) -> InstKind) -> Function {
    let mut f = body_fn(|_, _, _| {}, false);
    let kind = access(&mut f);
    f.append_inst(BlockId(0), kind, Type::I64, None);
    f
}

fn fresh_cell(f: &mut Function, alloc: FuncId) -> ValueId {
    let new = InstKind::Call(alloc, vec![Operand::ConstI64(48)]);
    f.append_inst(BlockId(0), new, Type::Obj(StructId(0)), None)
}

#[test]
fn a_computed_offset_on_any_cell_of_the_class_but_a_new_one_is_foreign() {
    let alloc = FuncId(3);
    let at = Operand::ConstI64(8);
    let other = |f: &mut Function| f.add_param(Type::Obj(StructId(0)), "q");
    let fs = [
        also(|f| InstKind::LoadDyn(Type::I64, v(other(f)), at)),
        also(|f| InstKind::StoreDynScaled8(at, v(other(f)), at)),
        also(|f| InstKind::LoadU8Dyn(v(f.params[0]), at)),
    ];
    for f in &fs {
        assert!(foreign(f, StructId(0), &FIELDS, &receivers(f), Some(alloc)));
    }
    let f = also(|f| InstKind::StoreDyn(at, v(fresh_cell(f, alloc)), at));
    assert!(!foreign(
        &f,
        StructId(0),
        &FIELDS,
        &receivers(&f),
        Some(alloc)
    ));
}

#[test]
fn copies_of_one_another_the_receiver_never_reaches_are_not_it() {
    let cell = Type::Obj(StructId(0));
    let mut f = body_fn(|_, _, _| {}, false);
    let a = f.append_inst(
        BlockId(0),
        InstKind::Copy(cell, Operand::ConstI64(0)),
        cell,
        None,
    );
    let b = f.append_inst(BlockId(0), InstKind::Copy(cell, v(a)), cell, None);
    let def_a = f.blocks[0].insts.len() - 2;
    f.blocks[0].insts[def_a].kind = InstKind::Copy(cell, v(b));
    let recv = receivers(&f);
    assert!(!recv.contains(&a) && !recv.contains(&b));
    f.append_void(BlockId(0), InstKind::Store(Operand::ConstI64(0), v(a), POS));
    assert!(foreign(&f, StructId(0), &FIELDS, &receivers(&f), None));
}

#[test]
fn an_allocated_cell_defined_again_is_foreign() {
    let alloc = FuncId(3);
    let cell = Type::Obj(StructId(0));
    let mut f = body_fn(|_, _, _| {}, false);
    let other = f.add_param(cell, "q");
    let c = fresh_cell(&mut f, alloc);
    f.blocks[0].insts.push(Inst {
        result: Some(c),
        kind: InstKind::Copy(cell, v(other)),
        origin: None,
    });
    f.append_void(BlockId(0), InstKind::Store(Operand::ConstI64(0), v(c), POS));
    assert!(foreign(
        &f,
        StructId(0),
        &FIELDS,
        &receivers(&f),
        Some(alloc)
    ));
}
