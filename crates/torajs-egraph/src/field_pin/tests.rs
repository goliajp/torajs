use super::*;
use torajs_core::ssa::{ArrId, BinOp, Block, FuncId, IPred, PinFact, StructId, ValueInfo};

fn mk(result: Option<u32>, kind: InstKind) -> Inst {
    Inst {
        result: result.map(ValueId),
        kind,
        origin: None,
    }
}
fn v(i: u32) -> Operand {
    Operand::Value(ValueId(i))
}
fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> torajs_core::ssa::Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}
fn field(r: u32, base: u32) -> Inst {
    mk(Some(r), InstKind::Load(Type::Arr(ArrId(0)), v(base), 32))
}
fn ty(t: Type) -> ValueInfo {
    ValueInfo { ty: t, name: None }
}

/// `while (this.toks[this.pos] …) { this.toks[this.pos]; this.m() }`:
/// b0 → b1 (header): %1 = this.toks · %2 = its length · %3 = 0 < %2 |
/// b2 (body): %6 = this.toks · %4 = its data · %5 = the element · call
/// m(this) · → b1 | b3: ret %2.
fn walk() -> Function {
    let blocks = vec![
        block(0, vec![], Terminator::Br(BlockId(1))),
        block(
            1,
            vec![
                field(1, 0),
                mk(Some(2), InstKind::Load(Type::I64, v(1), 8)),
                mk(
                    Some(3),
                    InstKind::ICmp(IPred::Slt, Operand::ConstI64(0), v(2)),
                ),
            ],
            Terminator::CondBr {
                cond: v(3),
                then_blk: BlockId(2),
                else_blk: BlockId(3),
            },
        ),
        block(
            2,
            vec![
                field(6, 0),
                mk(Some(4), InstKind::Load(Type::Ptr, v(6), 32)),
                mk(Some(5), InstKind::Load(Type::I64, v(4), 0)),
                mk(None, InstKind::Call(FuncId(0), vec![v(0)])),
            ],
            Terminator::Br(BlockId(1)),
        ),
        block(3, vec![], Terminator::Ret(Some(v(2)))),
    ];
    Function {
        name: "__cm_P__m".into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks,
        values: vec![
            ty(Type::Obj(StructId(0))),
            ty(Type::Arr(ArrId(0))),
            ty(Type::I64),
            ty(Type::Bool),
            ty(Type::Ptr),
            ty(Type::I64),
            ty(Type::Arr(ArrId(0))),
        ],
        current_origin: None,
    }
}

fn module(f: Function, pinned: bool) -> Module {
    let pins = if pinned {
        vec![PinFact {
            sid: StructId(0),
            field_off: 32,
            arr: ArrId(0),
            cluster: vec![FuncId(0)],
            entries: vec![FuncId(0)],
            direct_only: true,
            quiet: vec![],
        }]
    } else {
        vec![]
    };
    Module {
        funcs: vec![f],
        pins,
        ..Default::default()
    }
}

fn loads(b: &Block) -> usize {
    (b.insts.iter())
        .filter(|i| matches!(i.kind, InstKind::Load(..)))
        .count()
}

#[test]
fn a_loop_reads_the_three_words_once_at_the_entry() {
    let mut m = module(walk(), true);
    let st = pin_field_loads(&mut m);
    assert_eq!((st.members, st.loads_merged), (1, 1));
    let f = &m.funcs[0];
    let head: Vec<_> = f.blocks[0].insts.iter().map(|i| &i.kind).collect();
    assert_eq!(
        head,
        [
            &InstKind::Load(Type::Arr(ArrId(0)), v(0), 32),
            &InstKind::Load(Type::I64, v(1), 8),
            &InstKind::Load(Type::Ptr, v(1), 32),
        ]
    );
    assert_eq!(loads(&f.blocks[1]), 0);
    // the element read stays where it was
    assert_eq!(loads(&f.blocks[2]), 1);
}

#[test]
fn a_lone_read_in_a_loop_test_stays() {
    let mut f = walk();
    f.blocks[2].insts.remove(0);
    f.blocks[2].insts[0] = mk(Some(4), InstKind::Load(Type::Ptr, v(1), 32));
    let before = f.clone();
    let mut m = module(f, true);
    assert_eq!(pin_field_loads(&mut m).members, 0);
    assert_eq!(m.funcs[0], before);
}

#[test]
fn reads_on_two_arms_stay() {
    // b0 branches to b1 and b2, each reading the field once
    let mut f = walk();
    f.blocks = vec![
        block(
            0,
            vec![],
            Terminator::CondBr {
                cond: Operand::ConstI64(1),
                then_blk: BlockId(1),
                else_blk: BlockId(2),
            },
        ),
        block(1, vec![field(1, 0)], Terminator::Ret(None)),
        block(2, vec![field(6, 0)], Terminator::Ret(None)),
    ];
    let before = f.clone();
    let mut m = module(f, true);
    assert_eq!(pin_field_loads(&mut m).members, 0);
    assert_eq!(m.funcs[0], before);
}

#[test]
fn without_a_pin_nothing_moves() {
    let mut m = module(walk(), false);
    pin_field_loads(&mut m);
    assert_eq!(m.funcs[0], walk());
}

#[test]
fn reads_split_by_a_call_share_the_entry_loads() {
    // %1 = this.toks · %2 = its length · call · %6 = this.toks · %7 =
    // its length · %8 = %2 + %7 · ret %8
    let mut f = walk();
    f.blocks = vec![block(
        0,
        vec![
            field(1, 0),
            mk(Some(2), InstKind::Load(Type::I64, v(1), 8)),
            mk(None, InstKind::Call(FuncId(0), vec![v(0)])),
            field(6, 0),
            mk(Some(7), InstKind::Load(Type::I64, v(6), 8)),
            mk(Some(8), InstKind::BinOp(BinOp::Add, v(2), v(7))),
        ],
        Terminator::Ret(Some(v(8))),
    )];
    f.values.extend([ty(Type::I64), ty(Type::I64)]);
    let mut m = module(f, true);
    let st = pin_field_loads(&mut m);
    assert_eq!((st.members, st.loads_merged), (1, 2));
    let insts = &m.funcs[0].blocks[0].insts;
    assert_eq!(insts.len(), 4);
    assert_eq!(insts[3].kind, InstKind::BinOp(BinOp::Add, v(2), v(2)));
}

#[test]
fn one_read_on_a_straight_path_stays() {
    let mut f = walk();
    f.blocks = vec![block(
        0,
        vec![field(1, 0), mk(Some(2), InstKind::Load(Type::I64, v(1), 8))],
        Terminator::Ret(Some(v(2))),
    )];
    let before = f.clone();
    let mut m = module(f, true);
    assert_eq!(pin_field_loads(&mut m).members, 0);
    assert_eq!(m.funcs[0], before);
}

#[test]
fn the_field_of_another_cell_is_not_merged() {
    let mut f = walk();
    f.params.push(ValueId(7));
    f.values
        .extend([ty(Type::Obj(StructId(0))), ty(Type::Arr(ArrId(0)))]);
    f.blocks[2].insts.push(field(8, 7));
    let mut m = module(f, true);
    pin_field_loads(&mut m);
    let other = InstKind::Load(Type::Arr(ArrId(0)), v(7), 32);
    assert!(m.funcs[0].blocks[2].insts.iter().any(|i| i.kind == other));
}

#[test]
fn a_receiver_bound_through_an_id_is_the_receiver() {
    // the inliner's `id` binding of a spliced callee's receiver
    let mut f = walk();
    f.blocks[0]
        .insts
        .push(mk(Some(7), InstKind::Identity(v(0))));
    f.values.push(ty(Type::Obj(StructId(0))));
    f.blocks[2].insts.push(field(8, 7));
    f.values.push(ty(Type::Arr(ArrId(0))));
    let mut m = module(f, true);
    let st = pin_field_loads(&mut m);
    assert_eq!(st.loads_merged, 2);
    assert_eq!(loads(&m.funcs[0].blocks[2]), 1);
}
