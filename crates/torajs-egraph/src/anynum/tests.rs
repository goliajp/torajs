use super::*;
use crate::anynum::expand::{expand_number_nodes, is_number_node};
use torajs_core::ssa::{
    BinOp, Block, FPred, FuncId, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
};

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

fn inst(result: u32, kind: InstKind) -> Inst {
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

fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}

fn cond(c: u32, t: u32, e: u32) -> Terminator {
    Terminator::CondBr {
        cond: v(c),
        then_blk: BlockId(t),
        else_blk: BlockId(e),
    }
}

/// `let total: any = 0; for (i = 0; i < n; i++) total = total + x()`
/// as the lowering leaves it after φ promotion: the addend is tested,
/// decoded and added, the sum boxed into the cell; a non-number
/// addend takes the kernel call.
///
/// ```text
/// bb0: %0 = call n()            ; f64 bound
///      %1 = box_int32 0
///      %2 = copy any %1         ; total
///      %3 = copy i64 0          ; i
///      br bb1
/// bb1: %4 = sitofp %3
///      %5 = fcmp olt %4, %0
///      cond_br %5, bb2, bb6
/// bb2: %6 = call x()           ; any addend
///      %7 = any_is_number %6
///      cond_br %7, bb3, bb4
/// bb3: %8 = any_is_int32 %6
///      %9 = any_to_f64 %6, %8
///      %10 = any_is_int32 %2
///      %11 = any_to_f64 %2, %10
///      %12 = fadd %11, %9
///      %13 = box_f64 %12
///      %2 = copy any %13
///      br bb5
/// bb4: %14 = call add(%2, %6)
///      %2 = copy any %14
///      br bb5
/// bb5: rc_dec_any %6
///      %15 = add %3, 1
///      %3 = copy i64 %15
///      br bb1
/// bb6: call print(%2)
///      ret
/// ```
fn sum_loop(extra_body: Vec<Inst>) -> Module {
    let tys = [
        Type::F64,
        Type::Any,
        Type::Any,
        Type::I64,
        Type::F64,
        Type::Bool,
        Type::Any,
        Type::Bool,
        Type::Bool,
        Type::F64,
        Type::Bool,
        Type::F64,
        Type::F64,
        Type::Any,
        Type::Any,
        Type::I64,
    ];
    let values = tys
        .iter()
        .map(|ty| ValueInfo {
            ty: *ty,
            name: None,
        })
        .collect();
    let mut body = vec![
        inst(8, InstKind::AnyIsInt32(v(6))),
        inst(9, InstKind::AnyToF64(v(6), v(8))),
        inst(10, InstKind::AnyIsInt32(v(2))),
        inst(11, InstKind::AnyToF64(v(2), v(10))),
        inst(12, InstKind::BinOp(BinOp::FAdd, v(11), v(9))),
        inst(13, InstKind::BoxF64(v(12))),
        inst(2, InstKind::Copy(Type::Any, v(13))),
    ];
    body.extend(extra_body);
    let blocks = vec![
        block(
            0,
            vec![
                inst(0, InstKind::Call(FuncId(0), vec![])),
                inst(1, InstKind::BoxInt32(Operand::ConstI64(0))),
                inst(2, InstKind::Copy(Type::Any, v(1))),
                inst(3, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
            ],
            Terminator::Br(BlockId(1)),
        ),
        block(
            1,
            vec![
                inst(4, InstKind::SiToFp(v(3))),
                inst(5, InstKind::FCmp(FPred::Olt, v(4), v(0))),
            ],
            cond(5, 2, 6),
        ),
        block(
            2,
            vec![
                inst(6, InstKind::Call(FuncId(0), vec![])),
                inst(7, InstKind::AnyIsNumber(v(6))),
            ],
            cond(7, 3, 4),
        ),
        block(3, body, Terminator::Br(BlockId(5))),
        block(
            4,
            vec![
                inst(14, InstKind::Call(FuncId(0), vec![v(2), v(6)])),
                inst(2, InstKind::Copy(Type::Any, v(14))),
            ],
            Terminator::Br(BlockId(5)),
        ),
        block(
            5,
            vec![
                void(InstKind::RcDecAny(v(6))),
                inst(15, InstKind::BinOp(BinOp::Add, v(3), Operand::ConstI64(1))),
                inst(3, InstKind::Copy(Type::I64, v(15))),
            ],
            Terminator::Br(BlockId(1)),
        ),
        block(
            6,
            vec![void(InstKind::Call(FuncId(0), vec![v(2)]))],
            Terminator::Ret(None),
        ),
    ];
    let mut m = Module::default();
    m.funcs.push(torajs_core::ssa::Function {
        name: "ext".into(),
        params: vec![],
        ret: Type::Any,
        blocks: vec![],
        values: vec![],
        current_origin: None,
    });
    m.funcs.push(torajs_core::ssa::Function {
        name: "f".into(),
        params: vec![],
        ret: Type::Void,
        blocks,
        values,
        current_origin: None,
    });
    m
}

fn kinds(f: &torajs_core::ssa::Function) -> Vec<&InstKind> {
    f.blocks
        .iter()
        .flat_map(|b| b.insts.iter().map(|i| &i.kind))
        .collect()
}

#[test]
fn the_sum_runs_in_lanes_and_boxes_back_on_every_way_out() {
    let mut m = sum_loop(vec![]);
    let stats = unbox_loop_cells(&mut m);
    assert_eq!(stats.loops_versioned, 1);
    assert_eq!(stats.cells_unboxed, 1);
    let f = &m.funcs[1];
    // the copy's sum lands in the f64 lane straight from the fadd
    let fadds: Vec<ValueId> = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::FAdd, ..)))
        .filter_map(|i| i.result)
        .collect();
    assert_eq!(fadds.len(), 2, "the slow loop keeps its own fadd");
    let laned = f.blocks.iter().flat_map(|b| b.insts.iter()).any(
        |i| matches!(i.kind, InstKind::Copy(Type::F64, Operand::Value(s)) if fadds.contains(&s)),
    );
    assert!(laned);
    // the copy reads no box of its cell: no any_to_f64 of a cell is left
    // on a lane-fed fadd, and the boxes back are selects on the int flag
    let selects = kinds(f)
        .iter()
        .filter(|k| matches!(k, InstKind::Select(Type::Any, ..)))
        .count();
    // the side exit into the kernel call and the loop exit
    assert_eq!(selects, 2, "{:?}", kinds(f));
    // the reader after the loop reads an out cell, not the loop cell
    let print = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .find(
            |i| matches!(&i.kind, InstKind::Call(_, args) if args.len() == 1 && i.result.is_none()),
        )
        .expect("the print");
    assert!(!matches!(print.kind, InstKind::Call(_, ref a) if a[0] == v(2)));
    // the kernel call is still there, once
    let kernels = kinds(f)
        .iter()
        .filter(|k| matches!(k, InstKind::Call(_, args) if args.len() == 2))
        .count();
    assert_eq!(kernels, 1);
    // what remains boxed expands
    expand_number_nodes(&mut m);
    assert!(kinds(&m.funcs[1]).iter().all(|k| !is_number_node(k)));
}

#[test]
fn a_use_the_lanes_cannot_answer_leaves_the_loop_alone() {
    // the body hands the cell to a call every iteration
    let mut m = sum_loop(vec![void(InstKind::Call(FuncId(0), vec![v(2)]))]);
    let before = m.funcs[1].blocks.len();
    let stats = unbox_loop_cells(&mut m);
    assert_eq!(stats.loops_versioned, 0);
    assert_eq!(stats.skipped, 1);
    assert_eq!(m.funcs[1].blocks.len(), before);
}

#[test]
fn a_web_cell_only_the_slow_part_defines_leaves_the_web() {
    // the kernel call's answer reaches the sum through a second cell
    // (`%16`) that, in the loop, only the slow block defines
    let mut m = sum_loop(vec![]);
    let f = &mut m.funcs[1];
    f.values.push(ValueInfo {
        ty: Type::Any,
        name: None,
    });
    f.blocks[0]
        .insts
        .push(inst(16, InstKind::Copy(Type::Any, v(1))));
    f.blocks[4].insts = vec![
        inst(14, InstKind::Call(FuncId(0), vec![v(2), v(6)])),
        inst(16, InstKind::Copy(Type::Any, v(14))),
        inst(2, InstKind::Copy(Type::Any, v(16))),
    ];
    let stats = unbox_loop_cells(&mut m);
    assert_eq!(stats.loops_versioned, 1);
    assert_eq!(stats.cells_unboxed, 1);
}

#[test]
fn a_cell_a_call_stores_straight_into_is_an_opaque_def() {
    // the fast block's last def of the cell is a call's answer, not a
    // copy: the block is slow, and the fast part keeps no sum
    let mut m = sum_loop(vec![inst(2, InstKind::Call(FuncId(0), vec![]))]);
    let before = m.funcs[1].blocks.len();
    let stats = unbox_loop_cells(&mut m);
    assert_eq!(stats.loops_versioned, 0);
    assert_eq!(m.funcs[1].blocks.len(), before);
}

#[test]
fn a_fast_part_that_never_reads_the_lanes_is_not_copied() {
    // `total = x + x`: the cell is stored every iteration, never read
    let mut m = sum_loop(vec![]);
    let f = &mut m.funcs[1];
    f.blocks[3].insts.retain(|i| {
        !matches!(i.kind, InstKind::AnyIsInt32(Operand::Value(ValueId(2))))
            && !matches!(i.kind, InstKind::AnyToF64(Operand::Value(ValueId(2)), _))
    });
    for i in f.blocks[3].insts.iter_mut() {
        if let InstKind::BinOp(BinOp::FAdd, a, _) = &mut i.kind {
            *a = v(9);
        }
    }
    let before = m.funcs[1].blocks.len();
    let stats = unbox_loop_cells(&mut m);
    assert_eq!(stats.loops_versioned, 0);
    assert_eq!(m.funcs[1].blocks.len(), before);
}
