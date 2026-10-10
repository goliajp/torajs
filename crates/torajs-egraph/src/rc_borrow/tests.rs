use super::*;
use torajs_core::ssa::{BinOp, Block, BlockId, FuncId, HEADER_WORD_ARR_ANY, IPred, ValueInfo};

const LENGTH_GET_FID: FuncId = FuncId(0);
const ALLOC_FID: FuncId = FuncId(1);
const PRINT_FID: FuncId = FuncId(2);
const SLOW_FID: FuncId = FuncId(3);

const ARR: ValueId = ValueId(0);
const WORD: ValueId = ValueId(1);
const FLAGS: ValueId = ValueId(2);
const DENSE: ValueId = ValueId(3);
const DATA: ValueId = ValueId(4);
const ELEM: ValueId = ValueId(5);
const X: ValueId = ValueId(6);
const SLOW: ValueId = ValueId(7);
const BITS: ValueId = ValueId(8);
const CELL: ValueId = ValueId(9);
const LEN: ValueId = ValueId(10);

fn inst(result: Option<ValueId>, kind: InstKind) -> Inst {
    Inst {
        result,
        kind,
        origin: None,
    }
}

fn v(id: ValueId) -> Operand {
    Operand::Value(id)
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

/// The `const s = arr[i]; use(s.length)` shape after mem2reg: the
/// header test, the element arm, the slow arm, and the join holding
/// the retain, the use, the release.
fn fixture(tail: Vec<Inst>, header_const: i64) -> Module {
    let types = [
        Type::Ptr,
        Type::I64,
        Type::I64,
        Type::Bool,
        Type::Ptr,
        Type::Any,
        Type::Any,
        Type::Any,
        Type::I64,
        Type::Ptr,
        Type::I64,
    ];
    let values = types
        .iter()
        .map(|ty| ValueInfo {
            ty: *ty,
            name: None,
        })
        .collect();
    let bb0 = Block {
        id: BlockId(0),
        insts: vec![
            inst(
                Some(ARR),
                InstKind::Call(ALLOC_FID, vec![Operand::ConstI64(0)]),
            ),
            inst(Some(WORD), InstKind::Load(Type::I64, v(ARR), 0)),
            inst(
                Some(FLAGS),
                InstKind::BinOp(
                    BinOp::And,
                    v(WORD),
                    Operand::ConstI64(HEADER_WORD_ARR_ANY | 1),
                ),
            ),
            inst(
                Some(DENSE),
                InstKind::ICmp(IPred::Eq, v(FLAGS), Operand::ConstI64(header_const)),
            ),
        ],
        term: Terminator::CondBr {
            cond: v(DENSE),
            then_blk: BlockId(1),
            else_blk: BlockId(2),
        },
    };
    let bb1 = Block {
        id: BlockId(1),
        insts: vec![
            inst(Some(DATA), InstKind::Load(Type::Ptr, v(ARR), 32)),
            inst(
                Some(ELEM),
                InstKind::LoadDyn(Type::Any, v(DATA), Operand::ConstI64(0)),
            ),
            inst(Some(X), InstKind::Copy(Type::Any, v(ELEM))),
        ],
        term: Terminator::Br(BlockId(3)),
    };
    let bb2 = Block {
        id: BlockId(2),
        insts: vec![
            inst(Some(SLOW), InstKind::Call(SLOW_FID, vec![])),
            inst(Some(X), InstKind::Copy(Type::Any, v(SLOW))),
        ],
        term: Terminator::Br(BlockId(3)),
    };
    let mut insts = vec![inst(None, InstKind::RcIncAny(v(X)))];
    insts.extend(tail);
    insts.push(inst(None, InstKind::RcDecAny(v(X))));
    let bb3 = Block {
        id: BlockId(3),
        insts,
        term: Terminator::Ret(None),
    };
    let body = Function {
        name: "main".into(),
        params: vec![],
        ret: Type::Void,
        blocks: vec![bb0, bb1, bb2, bb3],
        values,
        current_origin: None,
    };
    Module {
        funcs: vec![
            declaration("__torajs_any_length_get"),
            declaration("__torajs_arr_alloc_any"),
            declaration("__torajs_str_print"),
            declaration("__torajs_slow"),
            body,
        ],
        ..Default::default()
    }
}

fn read_len() -> Vec<Inst> {
    vec![
        inst(Some(BITS), InstKind::PtrToInt(v(X))),
        inst(Some(CELL), InstKind::IntToPtr(v(BITS))),
        inst(Some(LEN), InstKind::Load(Type::I64, v(CELL), 4)),
    ]
}

fn body(m: &Module) -> &Function {
    &m.funcs[4]
}

fn count(f: &Function, pred: impl Fn(&InstKind) -> bool) -> usize {
    f.blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|i| pred(&i.kind))
        .count()
}

#[test]
fn element_read_borrowed_slow_arm_retained() {
    let mut m = fixture(read_len(), HEADER_WORD_ARR_ANY);
    let stats = borrow_array_elems(&mut m);
    assert_eq!(stats.webs_borrowed, 1);
    assert_eq!(stats.incs_elided, 1);
    let f = body(&m);
    // the retain now sits on the slow arm only, right after its copy
    assert_eq!(count(f, |k| matches!(k, InstKind::RcIncAny(_))), 1);
    assert!(matches!(
        f.blocks[2].insts.as_slice(),
        [
            _,
            Inst {
                kind: InstKind::Copy(Type::Any, _),
                ..
            },
            Inst {
                kind: InstKind::RcIncAny(_),
                ..
            },
            Inst {
                kind: InstKind::Copy(Type::Bool, Operand::ConstBool(true)),
                ..
            }
        ]
    ));
    assert!(matches!(
        f.blocks[1].insts.last(),
        Some(Inst {
            kind: InstKind::Copy(Type::Bool, Operand::ConstBool(false)),
            ..
        })
    ));
    // the release is behind the flag: join → (dec → cont) | cont
    let flag = f.blocks[1].insts.last().unwrap().result.unwrap();
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        else_blk,
    } = f.blocks[3].term
    else {
        panic!("join must end in the flag test");
    };
    assert_eq!(c, flag);
    assert!(matches!(
        f.blocks[then_blk.0 as usize].insts.as_slice(),
        [Inst {
            kind: InstKind::RcDecAny(_),
            ..
        }]
    ));
    assert_eq!(f.blocks[then_blk.0 as usize].term, Terminator::Br(else_blk));
    assert_eq!(f.blocks[else_blk.0 as usize].term, Terminator::Ret(None));
    assert_eq!(f.blocks[3].insts.len(), 3);
}

#[test]
fn opaque_call_in_window_keeps_pair() {
    let mut tail = vec![inst(None, InstKind::Call(SLOW_FID, vec![]))];
    tail.extend(read_len());
    let mut m = fixture(tail, HEADER_WORD_ARR_ANY);
    let stats = borrow_array_elems(&mut m);
    assert_eq!(stats.webs_borrowed, 0);
    assert_eq!(count(body(&m), |k| matches!(k, InstKind::RcIncAny(_))), 1);
    assert_eq!(body(&m).blocks.len(), 4);
}

#[test]
fn readonly_print_in_window_is_transparent() {
    let mut tail = vec![inst(None, InstKind::Call(PRINT_FID, vec![v(ARR)]))];
    tail.extend(read_len());
    let mut m = fixture(tail, HEADER_WORD_ARR_ANY);
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 1);
}

#[test]
fn heap_store_in_window_keeps_pair() {
    let mut tail = vec![inst(None, InstKind::Store(Operand::ConstI64(1), v(ARR), 8))];
    tail.extend(read_len());
    let mut m = fixture(tail, HEADER_WORD_ARR_ANY);
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 0);
}

#[test]
fn count_word_read_keeps_pair() {
    let tail = vec![
        inst(Some(BITS), InstKind::PtrToInt(v(X))),
        inst(Some(CELL), InstKind::IntToPtr(v(BITS))),
        inst(Some(LEN), InstKind::Load(Type::I64, v(CELL), 0)),
    ];
    let mut m = fixture(tail, HEADER_WORD_ARR_ANY);
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 0);
}

#[test]
fn other_header_test_is_not_an_array_read() {
    let mut m = fixture(read_len(), 1 << 50);
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 0);
}

#[test]
fn length_call_is_wrapped_as_last_use() {
    let tail = vec![inst(Some(LEN), InstKind::Call(LENGTH_GET_FID, vec![v(X)]))];
    let mut m = fixture(tail, HEADER_WORD_ARR_ANY);
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 1);
    let f = body(&m);
    assert!(matches!(
        f.blocks[3].insts.as_slice(),
        [
            Inst {
                kind: InstKind::RcIncAny(Operand::Value(X)),
                ..
            },
            Inst {
                kind: InstKind::Call(LENGTH_GET_FID, _),
                ..
            },
            Inst {
                kind: InstKind::RcDecAny(Operand::Value(X)),
                ..
            },
        ]
    ));
    assert!(matches!(f.blocks[3].term, Terminator::CondBr { .. }));
}

#[test]
fn use_after_length_call_keeps_pair() {
    let mut tail = vec![inst(Some(LEN), InstKind::Call(LENGTH_GET_FID, vec![v(X)]))];
    tail.extend(read_len());
    let mut m = fixture(tail, HEADER_WORD_ARR_ANY);
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 0);
}

#[test]
fn holder_passed_to_other_call_keeps_pair() {
    let tail = vec![inst(None, InstKind::Call(SLOW_FID, vec![v(X)]))];
    let mut m = fixture(tail, HEADER_WORD_ARR_ANY);
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 0);
}

#[test]
fn retain_not_at_the_join_keeps_pair() {
    // the retain moved into the element arm only: the slow arm's
    // copy reaches the join unretained, so it cannot be normalized
    let mut m = fixture(read_len(), HEADER_WORD_ARR_ANY);
    let f = &mut m.funcs[4];
    let inc = f.blocks[3].insts.remove(0);
    f.blocks[1].insts.insert(0, inc);
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 0);
}

/// `read_len` with the cell's field at +16 (or slot 2) read as `ty`
/// into value 11, then `between`, then a load through value 11.
fn read_through(ty: Type, dyn_read: bool, between: Vec<Inst>) -> Module {
    let inner = ValueId(11);
    let mut tail = vec![
        inst(Some(BITS), InstKind::PtrToInt(v(X))),
        inst(Some(CELL), InstKind::IntToPtr(v(BITS))),
        inst(
            Some(inner),
            if dyn_read {
                InstKind::LoadDyn(ty, v(CELL), Operand::ConstI64(2))
            } else {
                InstKind::Load(ty, v(CELL), 16)
            },
        ),
    ];
    tail.extend(between);
    tail.push(inst(Some(LEN), InstKind::Load(Type::I64, v(inner), 8)));
    let mut m = fixture(tail, HEADER_WORD_ARR_ANY);
    m.funcs[4].values.push(ValueInfo { ty, name: None });
    m
}

#[test]
fn pointer_read_through_holder_live_across_call_keeps_pair() {
    // the pointer the element holds dies with it: a call that may
    // drop the slot while that pointer is still to be read is a hazard
    for (ty, dyn_read) in [(Type::Ptr, false), (Type::Any, false), (Type::Any, true)] {
        let call = vec![inst(None, InstKind::Call(SLOW_FID, vec![]))];
        let mut m = read_through(ty, dyn_read, call);
        assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 0, "{ty:?}");
    }
}

#[test]
fn pointer_read_through_holder_used_before_call_is_borrowed() {
    let mut m = read_through(Type::Ptr, false, vec![]);
    let f = &mut m.funcs[4];
    // before the release, after the last read through value 11
    let at = f.blocks[3].insts.len() - 1;
    f.blocks[3]
        .insts
        .insert(at, inst(None, InstKind::Call(SLOW_FID, vec![])));
    assert_eq!(borrow_array_elems(&mut m).webs_borrowed, 1);
}
