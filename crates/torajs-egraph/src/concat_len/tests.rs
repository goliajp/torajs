use super::*;
use crate::str_append::test_support::{
    BODY, STR_CONCAT, STR_DROP, SUBSTR_CONCAT_SUBSTR_STR, SUBSTR_CONCAT_SUBSTR_SUBSTR, call,
    concat, drop_of, module_with_blocks, v,
};
use torajs_core::ssa::{FuncId, IPred};

fn inst(result: u32, kind: InstKind) -> Inst {
    Inst {
        result: Some(ValueId(result)),
        kind,
        origin: None,
    }
}

fn len_load(result: u32, of: u32) -> Inst {
    inst(result, InstKind::Load(Type::I32, v(of), STR_LEN_OFF))
}

fn zext(result: u32, of: u32) -> Inst {
    inst(result, InstKind::ZExtI32ToI64(v(of)))
}

fn copy_str(result: u32, of: u32) -> Inst {
    inst(result, InstKind::Copy(Type::Str, v(of)))
}

fn calls_in(func: &Function, blk: usize) -> Vec<FuncId> {
    func.blocks[blk]
        .insts
        .iter()
        .filter_map(|i| match i.kind {
            InstKind::Call(fid, _) => Some(fid),
            _ => None,
        })
        .collect()
}

fn blocks_calling(func: &Function, fid: FuncId) -> Vec<usize> {
    (0..func.blocks.len())
        .filter(|&b| calls_in(func, b).contains(&fid))
        .collect()
}

fn cond_br(func: &Function, blk: usize) -> (BlockId, BlockId) {
    let Terminator::CondBr {
        then_blk, else_blk, ..
    } = func.blocks[blk].term
    else {
        panic!("block {blk} ends in a test");
    };
    (then_blk, else_blk)
}

#[test]
fn a_measured_concat_becomes_a_length_sum_with_the_kernel_on_the_slow_arm() {
    let mut m = module_with_blocks(
        vec![vec![
            concat(10, 1, 2),
            len_load(11, 10),
            zext(12, 11),
            drop_of(10),
        ]],
        Terminator::Ret(Some(v(12))),
    );
    let stats = fold_concat_lens(&mut m);
    assert_eq!(stats.folded, 1);
    let f = &m.funcs[BODY];
    assert_eq!(f.blocks.len(), 5);
    for (pos, b) in f.blocks.iter().enumerate() {
        assert_eq!(b.id, BlockId(pos as u32));
    }
    // head: the two Str operands null-tested → slow | measure
    let head = &f.blocks[0];
    assert!(calls_in(f, 0).is_empty());
    assert!(matches!(
        head.insts[0].kind,
        InstKind::ICmp(IPred::Eq, _, Operand::ConstPtrNull)
    ));
    assert_eq!(cond_br(f, 0), (BlockId(4), BlockId(1)));
    // measure: two lengths widened, the sum and the bound test → slow | fast
    let measure = &f.blocks[1];
    assert!(matches!(
        measure.insts.last().map(|i| &i.kind),
        Some(InstKind::ICmp(
            IPred::Sgt,
            _,
            Operand::ConstI64(STR_MAX_LEN)
        ))
    ));
    assert_eq!(cond_br(f, 1), (BlockId(4), BlockId(2)));
    // fast: the sum, then the rest of the block
    assert_eq!(f.blocks[2].insts.len(), 1);
    assert_eq!(f.blocks[2].term, Terminator::Br(BlockId(3)));
    let join = &f.blocks[3];
    assert_eq!(
        join.insts.len(),
        2,
        "the load and its widening, both copies"
    );
    assert_eq!(join.insts[0].result, Some(ValueId(11)));
    assert!(matches!(join.insts[0].kind, InstKind::Copy(Type::I64, _)));
    assert_eq!(f.values[11].ty, Type::I64);
    assert_eq!(join.insts[1].result, Some(ValueId(12)));
    assert_eq!(join.insts[1].kind, InstKind::Copy(Type::I64, v(11)));
    assert_eq!(join.term, Terminator::Ret(Some(v(12))));
    // slow: the concat and its drop as they were, measuring what the
    // kernel answered (its left operand past the longest string)
    assert_eq!(calls_in(f, 4), vec![STR_CONCAT, STR_DROP]);
    assert_eq!(f.blocks[4].term, Terminator::Br(BlockId(3)));
    let slow = &f.blocks[4].insts;
    assert_eq!(slow[1].kind, InstKind::Load(Type::I32, v(10), STR_LEN_OFF));
    let widened = slow[2].result.expect("the widening defines a value");
    assert_eq!(
        slow[4].kind,
        InstKind::Copy(Type::I64, Operand::Value(widened))
    );
}

#[test]
fn a_view_operand_reads_its_i64_length_and_a_later_drop_is_moved() {
    let mut m = module_with_blocks(
        vec![
            vec![
                call(SUBSTR_CONCAT_SUBSTR_STR, 10, &[1, 2]),
                len_load(11, 10),
            ],
            vec![zext(12, 11), drop_of(10)],
        ],
        Terminator::Ret(Some(v(12))),
    );
    assert_eq!(fold_concat_lens(&mut m).folded, 1);
    let f = &m.funcs[BODY];
    // only the Str side is null-tested; the view's length is an i64 load
    assert_eq!(f.blocks[0].insts.len(), 1);
    let measure = &f.blocks[1];
    assert!(matches!(
        measure.insts[0].kind,
        InstKind::Load(Type::I64, _, SUBSTR_LEN_OFF)
    ));
    assert!(matches!(
        measure.insts[1].kind,
        InstKind::Load(Type::I32, _, _)
    ));
    assert_eq!(
        blocks_calling(f, STR_DROP),
        vec![5],
        "the drop sits on the slow arm only"
    );
    assert_eq!(calls_in(f, 5), vec![SUBSTR_CONCAT_SUBSTR_STR, STR_DROP]);
}

#[test]
fn two_view_operands_need_no_null_test_and_measure_in_the_head() {
    let mut m = module_with_blocks(
        vec![vec![
            call(SUBSTR_CONCAT_SUBSTR_SUBSTR, 10, &[1, 2]),
            len_load(11, 10),
            zext(12, 11),
            drop_of(10),
        ]],
        Terminator::Ret(Some(v(12))),
    );
    assert_eq!(fold_concat_lens(&mut m).folded, 1);
    let f = &m.funcs[BODY];
    assert_eq!(f.blocks.len(), 4);
    assert!(matches!(
        f.blocks[0].insts[0].kind,
        InstKind::Load(Type::I64, _, SUBSTR_LEN_OFF)
    ));
    assert_eq!(cond_br(f, 0), (BlockId(3), BlockId(1)));
    assert_eq!(calls_in(f, 3), vec![SUBSTR_CONCAT_SUBSTR_SUBSTR, STR_DROP]);
}

#[test]
fn a_cell_copied_between_two_concat_sites_folds_with_both() {
    // the shape the loop versions leave: one concat's result is
    // copied into the cell the other concat defines, and that
    // cell is measured and dropped on either exit
    let mut m = module_with_blocks(
        vec![
            vec![concat(10, 1, 2), len_load(11, 10), zext(12, 11)],
            vec![drop_of(10)],
            vec![copy_str(13, 10)],
            vec![concat(13, 1, 2)],
            vec![len_load(14, 13), zext(15, 14), drop_of(13)],
        ],
        Terminator::Ret(Some(v(15))),
    );
    assert_eq!(fold_concat_lens(&mut m).folded, 2);
    let f = &m.funcs[BODY];
    assert_eq!(blocks_calling(f, STR_CONCAT).len(), 2);
    assert_eq!(
        blocks_calling(f, STR_DROP),
        blocks_calling(f, STR_CONCAT),
        "each drop sits right after its concat on a slow arm"
    );
    let copies = f
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::Copy(Type::I64, Operand::Value(_))))
        .count();
    // two fast arms, two slow arms, one copy between the cells, two
    // loads, two widenings
    assert_eq!(copies, 9);
    assert!(
        f.blocks
            .iter()
            .flat_map(|b| &b.insts)
            .all(|i| !matches!(i.kind, InstKind::Copy(Type::Str, _)))
    );
}

#[test]
fn a_concat_read_in_any_other_way_is_left_alone() {
    // the result feeds another concat
    let mut m = module_with_blocks(
        vec![vec![
            concat(10, 1, 2),
            len_load(11, 10),
            zext(12, 11),
            call(STR_CONCAT, 13, &[10, 2]),
            drop_of(10),
            drop_of(13),
        ]],
        Terminator::Ret(None),
    );
    assert_eq!(fold_concat_lens(&mut m).folded, 0);
    // the length is read without being widened
    let mut m = module_with_blocks(
        vec![vec![concat(10, 1, 2), len_load(11, 10), drop_of(10)]],
        Terminator::Ret(Some(v(11))),
    );
    assert_eq!(fold_concat_lens(&mut m).folded, 0);
    // the cell is also defined by something other than a concat
    let mut m = module_with_blocks(
        vec![vec![
            concat(10, 1, 2),
            len_load(11, 10),
            zext(12, 11),
            drop_of(10),
            copy_str(10, 1),
            drop_of(10),
        ]],
        Terminator::Ret(Some(v(12))),
    );
    assert_eq!(fold_concat_lens(&mut m).folded, 0);
}
