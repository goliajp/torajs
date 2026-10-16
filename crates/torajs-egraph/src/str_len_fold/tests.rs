use super::*;
use torajs_core::ssa::{Block, BlockId, Inst, StringId, StringLiteral, ValueInfo};

fn v(i: u32) -> Operand {
    Operand::Value(ValueId(i))
}
fn mk(result: u32, kind: InstKind) -> Inst {
    Inst {
        result: Some(ValueId(result)),
        kind,
        origin: None,
    }
}
fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}
fn func(name: &str, blocks: Vec<Block>) -> Function {
    let ty = |i: usize| match i {
        0 | 1 | 3 => Type::Str,
        5 => Type::I32,
        7 => Type::Bool,
        _ => Type::I64,
    };
    Function {
        name: name.into(),
        params: vec![ValueId(1), ValueId(2)],
        ret: Type::I64,
        values: (0..9)
            .map(|i| ValueInfo {
                ty: ty(i),
                name: None,
            })
            .collect(),
        blocks,
        current_origin: None,
    }
}

/// f(tail, i): s = "<16 units>" + tail (or the other way round);
/// c = (i & mask) <u s.length; cond_br c, ret 1, ret 2
fn module(mask: i64, literal_left: bool, reread: bool) -> Module {
    let (l, r) = if literal_left {
        (v(0), v(1))
    } else {
        (v(1), v(0))
    };
    let mut insts = vec![
        mk(0, InstKind::StaticStrRef(StringId(0))),
        mk(3, InstKind::Call(FuncId(0), vec![l, r])),
        mk(
            4,
            InstKind::BinOp(BinOp::And, v(2), Operand::ConstI64(mask)),
        ),
        mk(5, InstKind::Load(Type::I32, v(3), STR_LEN_OFF)),
        mk(6, InstKind::ZExtI32ToI64(v(5))),
        mk(7, InstKind::ICmp(IPred::Ult, v(4), v(6))),
    ];
    if reread {
        insts.push(mk(
            8,
            InstKind::Select(Type::I64, v(7), Operand::ConstI64(1), Operand::ConstI64(2)),
        ));
    }
    let body = func(
        "f",
        vec![
            blk(
                0,
                insts,
                Terminator::CondBr {
                    cond: v(7),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            blk(1, vec![], Terminator::Ret(Some(Operand::ConstI64(1)))),
            blk(2, vec![], Terminator::Ret(Some(Operand::ConstI64(2)))),
        ],
    );
    Module {
        funcs: vec![func("__torajs_str_concat", vec![]), body],
        strings: vec![StringLiteral::encode_from_str("  Hello42 word  ")],
        ..Module::default()
    }
}

#[test]
fn an_index_below_the_left_literal_length_is_inside() {
    let mut m = module(15, true, false);
    let stats = fold_checks(&mut m);
    assert_eq!(
        stats,
        StrLenFoldStats {
            folded: 1,
            blocks_removed: 1
        }
    );
    let f = &m.funcs[1];
    assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
    // the compare and the length read that fed it are gone
    assert_eq!(f.blocks[0].insts.len(), 3);
}

#[test]
fn an_index_reaching_the_length_keeps_its_check() {
    let mut m = module(16, true, false);
    assert_eq!(fold_checks(&mut m).folded, 0);
}

#[test]
fn a_literal_on_the_right_says_nothing_of_the_length() {
    // past the longest string the kernel answers its left operand
    let mut m = module(15, false, false);
    assert_eq!(fold_checks(&mut m).folded, 0);
}

#[test]
fn a_compare_read_by_more_than_branches_stays() {
    let mut m = module(15, true, true);
    assert_eq!(fold_checks(&mut m).folded, 0);
}
