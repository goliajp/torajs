use super::*;
use torajs_core::ssa::{IPred, ValueInfo};

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

fn add(r: u32, x: Operand, c: i64) -> Inst {
    inst(r, InstKind::BinOp(BinOp::Add, x, Operand::ConstI64(c)))
}

/// A scan of the string `%0` by the counter cell `%2` up to `%1`; the
/// body `<body>` runs before the step.
///
/// ```text
/// bb0: %2 = copy 0 ; br bb1
/// bb1: %3 = icmp slt %2, %1 ; cond_br %3, bb2, bb3
/// bb2: <body> ; %4 = add %2, 1 ; %2 = copy %4 ; br bb1
/// bb3: ret 0
/// ```
fn scan(body: Vec<Inst>) -> Function {
    let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
        id: BlockId(id),
        insts,
        term,
    };
    let mut body = body;
    body.push(add(4, v(2), 1));
    body.push(inst(2, InstKind::Copy(Type::I64, v(4))));
    let test = Terminator::CondBr {
        cond: v(3),
        then_blk: BlockId(2),
        else_blk: BlockId(3),
    };
    Function {
        name: "f".into(),
        params: vec![ValueId(0), ValueId(1)],
        ret: Type::I64,
        blocks: vec![
            block(
                0,
                vec![inst(2, InstKind::Copy(Type::I64, Operand::ConstI64(0)))],
                Terminator::Br(BlockId(1)),
            ),
            block(
                1,
                vec![inst(3, InstKind::ICmp(IPred::Slt, v(2), v(1)))],
                test,
            ),
            block(2, body, Terminator::Br(BlockId(1))),
            block(3, vec![], Terminator::Ret(Some(Operand::ConstI64(0)))),
        ],
        values: (0..20)
            .map(|i| ValueInfo {
                ty: match i {
                    0 => Type::Str,
                    3 | 6 => Type::Bool,
                    14 => Type::Ptr,
                    _ => Type::I64,
                },
                name: None,
            })
            .collect(),
        current_origin: None,
    }
}

fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}

fn cond(c: u32, then_blk: u32, else_blk: u32) -> Terminator {
    Terminator::CondBr {
        cond: v(c),
        then_blk: BlockId(then_blk),
        else_blk: BlockId(else_blk),
    }
}

fn br(to: u32) -> Terminator {
    Terminator::Br(BlockId(to))
}

/// The block holding `base = add <of>, <c>`, by position.
fn base_block(f: &Function, of: Operand, c: i64) -> Vec<usize> {
    let add = InstKind::BinOp(BinOp::Add, of, Operand::ConstI64(c));
    (f.blocks.iter().enumerate())
        .filter(|(_, b)| b.insts.iter().any(|i| i.kind == add))
        .map(|(n, _)| n)
        .collect()
}

fn run(f: Function) -> (Function, OffsetBaseStats) {
    let mut m = Module {
        funcs: vec![f],
        ..Default::default()
    };
    let stats = fold_offset_bases(&mut m);
    (m.funcs.remove(0), stats)
}

#[test]
fn both_encodings_read_off_one_base_ahead_of_the_loop() {
    let f = scan(vec![
        add(10, v(2), 16),
        inst(11, InstKind::LoadU8Dyn(v(0), v(10))),
        add(12, v(2), 8),
        inst(13, InstKind::LoadU16Dyn(v(0), v(12))),
    ]);
    let (f, stats) = run(f);
    assert_eq!((stats.folded_reads, stats.bases), (2, 1));
    let last = f.blocks[0].insts.last().unwrap();
    assert_eq!(
        last.kind,
        InstKind::BinOp(BinOp::Add, v(0), Operand::ConstI64(16))
    );
    let base = last.result.unwrap();
    assert_eq!(f.values[base.0 as usize].ty, Type::Ptr);
    let b = Operand::Value(base);
    let body: Vec<&InstKind> = f.blocks[2].insts.iter().map(|i| &i.kind).collect();
    assert_eq!(*body[0], InstKind::LoadU8Dyn(b, v(2)));
    assert_eq!(*body[1], InstKind::LoadU16Dyn(b, v(2)));
    assert_eq!(body.len(), 4, "{body:?}");
}

#[test]
fn a_sum_read_otherwise_stays() {
    let f = scan(vec![
        add(10, v(2), 16),
        inst(11, InstKind::LoadU8Dyn(v(0), v(10))),
        inst(12, InstKind::Copy(Type::I64, v(10))),
    ]);
    let before = f.clone();
    let (f, stats) = run(f);
    assert_eq!(stats.folded_reads, 0);
    assert_eq!(f, before);
}

#[test]
fn a_base_the_loop_defines_stays() {
    let f = scan(vec![
        inst(14, InstKind::Load(Type::Ptr, v(0), 16)),
        add(10, v(2), 16),
        inst(11, InstKind::LoadU8Dyn(v(14), v(10))),
    ]);
    let before = f.clone();
    let (f, _) = run(f);
    assert_eq!(f, before);
}

#[test]
fn a_counter_written_between_the_sum_and_the_read_stays() {
    let f = scan(vec![
        add(10, v(2), 16),
        inst(2, InstKind::Copy(Type::I64, Operand::ConstI64(5))),
        inst(11, InstKind::LoadU8Dyn(v(0), v(10))),
    ]);
    let before = f.clone();
    let (f, _) = run(f);
    assert_eq!(f, before);
}

#[test]
fn a_read_outside_any_loop_stays() {
    let mut f = scan(vec![]);
    f.blocks[3].insts = vec![
        add(10, v(1), 16),
        inst(11, InstKind::LoadU8Dyn(v(0), v(10))),
    ];
    let before = f.clone();
    let (f, _) = run(f);
    assert_eq!(f, before);
}

#[test]
fn a_base_defined_ahead_of_the_loop_moves() {
    let mut f = scan(vec![
        add(10, v(2), 16),
        inst(11, InstKind::LoadU8Dyn(v(14), v(10))),
    ]);
    f.blocks[0]
        .insts
        .insert(0, inst(14, InstKind::Load(Type::Ptr, v(0), 24)));
    let (f, stats) = run(f);
    assert_eq!(stats.folded_reads, 1);
    let last = f.blocks[0].insts.last().unwrap();
    assert_eq!(
        last.kind,
        InstKind::BinOp(BinOp::Add, v(14), Operand::ConstI64(16))
    );
}

#[test]
fn blocks_out_of_id_order_place_the_base_by_id() {
    let mut f = scan(vec![
        add(10, v(2), 16),
        inst(11, InstKind::LoadU8Dyn(v(0), v(10))),
    ]);
    // the entry and the exit trade ids: positions no longer match ids
    f.blocks[0].id = BlockId(3);
    f.blocks[3].id = BlockId(0);
    f.blocks[1].term = Terminator::CondBr {
        cond: v(3),
        then_blk: BlockId(2),
        else_blk: BlockId(0),
    };
    let (f, stats) = run(f);
    assert_eq!(stats.folded_reads, 1);
    let entry = &f.blocks[0];
    assert_eq!(
        entry.insts.last().unwrap().kind,
        InstKind::BinOp(BinOp::Add, v(0), Operand::ConstI64(16))
    );
}

#[test]
fn a_loop_entered_from_a_neighbour_loop_header_gets_its_own_preheader() {
    // bb1 is the first loop's header and the second loop's immediate
    // dominator: a base there would be computed every first-loop trip
    let mut f = scan(vec![]);
    f.blocks[0]
        .insts
        .push(inst(7, InstKind::Copy(Type::I64, Operand::ConstI64(0))));
    f.blocks[1].term = cond(3, 2, 5);
    f.blocks.push(block(
        5,
        vec![inst(6, InstKind::ICmp(IPred::Slt, v(7), v(1)))],
        cond(6, 6, 3),
    ));
    f.blocks.push(block(
        6,
        vec![
            add(10, v(7), 16),
            inst(11, InstKind::LoadU8Dyn(v(0), v(10))),
            add(8, v(7), 1),
            inst(7, InstKind::Copy(Type::I64, v(8))),
        ],
        br(5),
    ));
    let (f, stats) = run(f);
    assert_eq!((stats.folded_reads, stats.preheaders), (1, 1));
    let at = base_block(&f, v(0), 16);
    assert_eq!(at, vec![6], "{:?}", f.blocks);
    let pre = &f.blocks[6];
    assert_eq!(pre.term, br(5));
    assert_eq!(f.blocks[1].term, cond(3, 2, pre.id.0));
}

#[test]
fn a_parameter_the_function_rewrites_stays() {
    let mut f = scan(vec![
        add(10, v(2), 16),
        inst(11, InstKind::LoadU8Dyn(v(0), v(10))),
    ]);
    f.blocks[3]
        .insts
        .push(inst(0, InstKind::Copy(Type::Str, v(0))));
    let before = f.clone();
    let (f, _) = run(f);
    assert_eq!(f, before);
}

#[test]
fn a_base_the_outer_loop_defines_goes_ahead_of_the_inner_loop() {
    // bb2 defines the base in the outer loop and enters the inner one
    let mut f = scan(vec![]);
    f.blocks[2] = block(
        2,
        vec![
            inst(14, InstKind::Load(Type::Ptr, v(0), 24)),
            inst(5, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
        ],
        br(4),
    );
    f.blocks.push(block(
        4,
        vec![inst(6, InstKind::ICmp(IPred::Slt, v(5), v(1)))],
        cond(6, 5, 6),
    ));
    f.blocks.push(block(
        5,
        vec![
            add(10, v(5), 16),
            inst(11, InstKind::LoadU8Dyn(v(14), v(10))),
            add(7, v(5), 1),
            inst(5, InstKind::Copy(Type::I64, v(7))),
        ],
        br(4),
    ));
    f.blocks.push(block(
        6,
        vec![add(4, v(2), 1), inst(2, InstKind::Copy(Type::I64, v(4)))],
        br(1),
    ));
    let (f, stats) = run(f);
    assert_eq!((stats.folded_reads, stats.preheaders), (1, 0));
    assert_eq!(base_block(&f, v(14), 16), vec![2]);
}

/// The sum in the body's first block, the read in the second.
fn split_body(i: Inst) -> Function {
    let mut f = scan(vec![]);
    let step = std::mem::take(&mut f.blocks[2].insts);
    let mut first = vec![];
    let counter = i.result.unwrap();
    first.push(i);
    first.push(add(10, Operand::Value(counter), 16));
    f.blocks[2] = block(2, first, br(4));
    let mut second = vec![inst(11, InstKind::LoadU8Dyn(v(0), v(10)))];
    second.extend(step);
    f.blocks.push(block(4, second, br(1)));
    f
}

#[test]
fn a_counter_of_one_definition_moves_across_blocks() {
    let f = split_body(inst(
        15,
        InstKind::BinOp(BinOp::And, v(2), Operand::ConstI64(15)),
    ));
    let (f, stats) = run(f);
    assert_eq!(stats.folded_reads, 1);
    let read = &f.blocks[4].insts[0].kind;
    let base = f.blocks[0].insts.last().unwrap().result.unwrap();
    assert_eq!(*read, InstKind::LoadU8Dyn(Operand::Value(base), v(15)));
}

#[test]
fn a_counter_cell_stays_across_blocks() {
    // %2 is written twice: across the block boundary nothing says it
    // holds the same value at the read as at the sum
    let f = split_body(inst(2, InstKind::Copy(Type::I64, v(2))));
    let before = f.clone();
    let (f, _) = run(f);
    assert_eq!(f, before);
}

#[test]
fn a_base_of_another_type_stays() {
    let mut f = scan(vec![
        add(10, v(2), 16),
        inst(11, InstKind::LoadU8Dyn(v(1), v(10))),
    ]);
    f.values[1].ty = Type::I64;
    let before = f.clone();
    let (f, _) = run(f);
    assert_eq!(f, before);
}
