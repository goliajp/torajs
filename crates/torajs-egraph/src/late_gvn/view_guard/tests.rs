use super::*;
use torajs_core::ssa::{BinOp, Inst, StructId, ValueInfo};

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
fn void(kind: InstKind) -> Inst {
    Inst {
        result: None,
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
fn cond_br(c: u32, t: u32, e: u32) -> Terminator {
    Terminator::CondBr {
        cond: v(c),
        then_blk: BlockId(t),
        else_blk: BlockId(e),
    }
}
fn cell(r: u32) -> Inst {
    mk(r, InstKind::GlobalRef(UNDEF_CELL.into()))
}

const VIEW: Type = Type::Inline(StructId(0));

/// `const t = xs[i]; this.pos = i + 1; t.k`:
/// b0 → (b1: t = cell | b2: t = elem addr) → b3 (J: store; t == NULL)
/// → (b4 raise | b5: t == cell → (b4 | b6: load t.k)).
/// `elem_ty` types the address, `j_extra` lands in J before the test.
fn view_read(elem_ty: Type, j_extra: Vec<Inst>, b6_extra: Vec<Inst>) -> Module {
    let values: Vec<ValueInfo> = [
        Type::Ptr,  // 0 this
        Type::I64,  // 1 i
        Type::I64,  // 2 unused
        Type::Bool, // 3 out of range
        Type::Ptr,  // 4 cell ref
        VIEW,       // 5 t
        Type::Ptr,  // 6 data
        Type::I64,  // 7 bits
        Type::I64,  // 8 addr bits
        elem_ty,    // 9 elem addr
        Type::I64,  // 10 i + 1
        Type::Bool, // 11 t == NULL
        Type::Ptr,  // 12 cell ref
        Type::Bool, // 13 t == cell
        Type::I64,  // 14 t.k
    ]
    .into_iter()
    .map(|ty| ValueInfo { ty, name: None })
    .collect();
    let mut j = vec![
        mk(10, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1))),
        void(InstKind::Store(v(10), v(0), 40)),
    ];
    j.extend(j_extra);
    j.push(mk(
        11,
        InstKind::ICmp(IPred::Eq, v(5), Operand::ConstPtrNull),
    ));
    let mut b6 = vec![mk(14, InstKind::Load(Type::I64, v(5), 0))];
    b6.extend(b6_extra);
    let mut m = Module {
        funcs: vec![Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(1), ValueId(6)],
            ret: Type::I64,
            values,
            blocks: vec![
                blk(0, vec![], cond_br(3, 1, 2)),
                blk(
                    1,
                    vec![cell(4), mk(5, InstKind::Copy(VIEW, v(4)))],
                    Terminator::Br(BlockId(3)),
                ),
                blk(
                    2,
                    vec![
                        mk(7, InstKind::PtrToInt(v(6))),
                        mk(8, InstKind::BinOp(BinOp::Add, v(7), v(1))),
                        mk(9, InstKind::IntToPtr(v(8))),
                        mk(5, InstKind::Copy(VIEW, v(9))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                blk(3, j, cond_br(11, 4, 5)),
                blk(4, vec![], Terminator::Ret(None)),
                blk(
                    5,
                    vec![cell(12), mk(13, InstKind::ICmp(IPred::Eq, v(5), v(12)))],
                    cond_br(13, 4, 6),
                ),
                blk(6, b6, Terminator::Ret(Some(v(14)))),
            ],
            current_origin: None,
        }],
        ..Default::default()
    };
    m.arr_layouts.intern(VIEW);
    m
}

fn target(f: &Function, b: usize) -> usize {
    match f.blocks[b].term {
        Terminator::Br(t) => t.0 as usize,
        _ => panic!("block {b} ends in a plain branch"),
    }
}

#[test]
fn each_arm_goes_where_its_view_sends_it() {
    let mut m = view_read(VIEW, vec![], vec![]);
    let st = thread_view_guards(&mut m);
    assert_eq!((st.joins, st.copies), (1, 1));
    let f = &m.funcs[0];
    // the test-only block is gone; the element arm runs J then the read
    assert_eq!(f.blocks.len(), 7);
    let j = target(f, 2);
    assert_eq!(j, 3);
    assert_eq!(
        f.blocks[3].insts.len(),
        2,
        "J keeps its store, drops its test"
    );
    let read = target(f, j);
    assert!(matches!(f.blocks[read].term, Terminator::Ret(Some(_))));
    // the cell arm runs a copy of J's store, then raises
    let copy = target(f, 1);
    assert_ne!(copy, j);
    let insts = &f.blocks[copy].insts;
    assert_eq!(insts.len(), 2);
    assert!(matches!(
        insts[1].kind,
        InstKind::Store(_, Operand::Value(_), 40)
    ));
    assert_ne!(
        insts[0].result,
        Some(ValueId(10)),
        "the copy defines afresh"
    );
    assert!(matches!(
        f.blocks[target(f, copy)].term,
        Terminator::Ret(None)
    ));
}

#[test]
fn a_program_without_arrays_laid_out_in_place_is_skipped() {
    let mut m = view_read(VIEW, vec![], vec![]);
    m.arr_layouts = Default::default();
    assert_eq!(thread_view_guards(&mut m).joins, 0);
}

#[test]
fn an_address_not_typed_as_a_view_is_not_known_to_be_an_element() {
    let mut m = view_read(Type::Ptr, vec![], vec![]);
    assert_eq!(thread_view_guards(&mut m).joins, 0);
}

#[test]
fn a_join_whose_values_are_read_below_it_is_left_alone() {
    let below = vec![void(InstKind::Store(v(10), v(0), 48))];
    let mut m = view_read(VIEW, vec![], below);
    assert_eq!(thread_view_guards(&mut m).joins, 0);
}

/// J's own definition answers the test below it (the cell test block
/// folds), but not J's test against its arms' definitions.
#[test]
fn a_join_that_defines_the_view_again_answers_only_what_follows_it() {
    let again = vec![mk(5, InstKind::Copy(VIEW, v(9)))];
    let mut m = view_read(VIEW, again, vec![]);
    let st = thread_view_guards(&mut m);
    assert_eq!((st.joins, st.copies), (1, 0));
    let f = &m.funcs[0];
    assert!(matches!(f.blocks[1].term, Terminator::Br(BlockId(3))));
    assert!(matches!(f.blocks[3].term, Terminator::CondBr { .. }));
}

fn push_i64(f: &mut Function) {
    f.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
}

/// A merged local: `%15` is defined in b0 and again in J by
/// `%15 = add %15, 1`, read only inside J. A copy of J would read a
/// fresh id it has not defined, and leave `%15` unchanged below.
#[test]
fn a_join_defining_a_merged_local_is_not_copied() {
    let bump = mk(15, InstKind::BinOp(BinOp::Add, v(15), Operand::ConstI64(1)));
    let mut m = view_read(VIEW, vec![bump], vec![]);
    push_i64(&mut m.funcs[0]);
    (m.funcs[0].blocks[0].insts).push(mk(15, InstKind::Copy(Type::I64, v(1))));
    assert_eq!(thread_view_guards(&mut m).joins, 0);
    let f = &m.funcs[0];
    assert!(matches!(f.blocks[1].term, Terminator::Br(BlockId(3))));
    assert!(matches!(f.blocks[3].term, Terminator::CondBr { .. }));
}

#[test]
fn a_join_reading_its_own_value_before_defining_it_is_not_copied() {
    let ahead = vec![
        mk(15, InstKind::BinOp(BinOp::Add, v(16), Operand::ConstI64(1))),
        mk(16, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(2))),
    ];
    let mut m = view_read(VIEW, ahead, vec![]);
    push_i64(&mut m.funcs[0]);
    push_i64(&mut m.funcs[0]);
    assert_eq!(thread_view_guards(&mut m).joins, 0);
}

/// Without an arm holding the cell nothing is copied, so a merged
/// local in J does not stop the element arm skipping the tests.
#[test]
fn a_merged_local_in_a_join_only_elements_reach_is_kept() {
    let bump = mk(15, InstKind::BinOp(BinOp::Add, v(15), Operand::ConstI64(1)));
    let mut m = view_read(VIEW, vec![bump], vec![]);
    let f = &mut m.funcs[0];
    push_i64(f);
    f.blocks[0]
        .insts
        .push(mk(15, InstKind::Copy(Type::I64, v(1))));
    f.blocks[0].term = Terminator::Br(BlockId(2));
    f.blocks[1].term = Terminator::Ret(None);
    let st = thread_view_guards(&mut m);
    assert_eq!((st.joins, st.copies), (1, 0));
    let f = &m.funcs[0];
    let j = target(f, target(f, 0));
    assert_eq!(f.blocks[j].insts.len(), 3, "J keeps its bump and store");
    assert!(matches!(
        f.blocks[target(f, j)].term,
        Terminator::Ret(Some(_))
    ));
}
