use super::*;
use torajs_core::ssa::{ArrId, Block, Inst, ValueInfo};

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

fn decl(name: &str) -> Function {
    Function {
        name: name.into(),
        params: vec![],
        ret: Type::Void,
        blocks: vec![],
        values: vec![],
        current_origin: None,
    }
}

/// b0: %0 = alloca_bytes `size` (typed `ty`) · `body` · %1 = %0 == null
/// · cond_br %1, b2, b1 | b1: release %0 · %2 = copy 7 · b2 | b2: ret —
/// lowering's release of a frame literal behind its nullish guard.
fn module(size: u64, ty: Type, released: Operand) -> Module {
    let blocks = vec![
        Block {
            id: BlockId(0),
            insts: vec![
                mk(Some(0), InstKind::AllocaBytes(size)),
                mk(
                    Some(1),
                    InstKind::ICmp(IPred::Eq, v(0), Operand::ConstPtrNull),
                ),
            ],
            term: Terminator::CondBr {
                cond: v(1),
                then_blk: BlockId(2),
                else_blk: BlockId(1),
            },
        },
        Block {
            id: BlockId(1),
            insts: vec![
                mk(None, InstKind::Call(FuncId(0), vec![released])),
                mk(Some(2), InstKind::Copy(Type::I64, Operand::ConstI64(7))),
            ],
            term: Terminator::Br(BlockId(2)),
        },
        Block {
            id: BlockId(2),
            insts: vec![],
            term: Terminator::Ret(None),
        },
    ];
    let info = |ty| ValueInfo { ty, name: None };
    Module {
        funcs: vec![
            decl(DROP),
            Function {
                name: "main".into(),
                params: vec![],
                ret: Type::Void,
                blocks,
                values: vec![info(ty), info(Type::Bool), info(Type::I64)],
                current_origin: None,
            },
        ],
        ..Default::default()
    }
}

fn calls(f: &Function) -> Vec<BlockId> {
    f.blocks
        .iter()
        .filter(|b| b.insts.iter().any(|i| matches!(i.kind, InstKind::Call(..))))
        .map(|b| b.id)
        .collect()
}

#[test]
fn a_frame_literal_release_is_called_only_off_its_born_shape() {
    let mut m = module(56, Type::Arr(ArrId(0)), v(0));
    let s = guard_frame_arr_drops(&mut m);
    assert_eq!(s.drops_guarded, 1);
    let f = &m.funcs[1];
    // the nullish guard folded: the entry goes straight to the test
    assert_eq!(f.blocks[0].term, Terminator::Br(BlockId(1)));
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        else_blk,
    } = f.blocks[1].term
    else {
        panic!("the release block ends in the born-shape test");
    };
    assert!(matches!(
        f.blocks[1].insts.last().map(|i| (&i.kind, i.result)),
        Some((InstKind::ICmp(IPred::Ne, _, Operand::ConstI64(0)), Some(r))) if r == c
    ));
    let loads: Vec<u64> = f.blocks[1]
        .insts
        .iter()
        .filter_map(|i| match i.kind {
            InstKind::Load(Type::I64, Operand::Value(ValueId(0)), off) => Some(off),
            _ => None,
        })
        .collect();
    assert_eq!(loads, vec![ARR_PROPS_OFF, ARR_DATA_PTR_OFF]);
    // the call sits alone on the slow arm, which rejoins the rest
    assert_eq!(calls(f), vec![then_blk]);
    assert_eq!(f.blocks[then_blk.0 as usize].term, Terminator::Br(else_blk));
    let rest = &f.blocks[else_blk.0 as usize];
    assert!(matches!(
        rest.insts[..],
        [Inst {
            result: Some(ValueId(2)),
            ..
        }]
    ));
    assert_eq!(rest.term, Terminator::Br(BlockId(2)));
}

#[test]
fn other_releases_are_left_alone() {
    // not an array cell, too small for a header, or not the cell itself
    for (size, ty, released) in [
        (56, Type::Ptr, v(0)),
        (32, Type::Arr(ArrId(0)), v(0)),
        (56, Type::Arr(ArrId(0)), v(2)),
    ] {
        let mut m = module(size, ty, released);
        assert_eq!(guard_frame_arr_drops(&mut m).drops_guarded, 0);
        assert_eq!(m.funcs[1].blocks.len(), 3);
        assert_eq!(calls(&m.funcs[1]), vec![BlockId(1)]);
    }
}
