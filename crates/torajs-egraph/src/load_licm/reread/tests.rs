use super::*;
use torajs_core::ssa::{Block, FuncId, IPred};

const ALLOC: u32 = 0;
const GROW: u32 = 1;
const PUSH: u32 = 2;
const MASK: i64 = 0x8310 << 48;

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

fn call(f: u32, args: Vec<Operand>) -> Inst {
    mk(None, InstKind::Call(FuncId(f), args))
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

/// `entry` defines %0 and jumps into the loop · b1 (header): %1 = len
/// · %2 = 0 u< %1 · b2 / b5 | b2: %3 = data · `body` · %4 = %3 == null
/// · b3 / b4 | b3: `cold` · b1 | b4: b1 | b5: ret — a typed store loop
/// whose out-of-range arm names the array.
fn store_loop(entry: Vec<Block>, body: Vec<Inst>, cold: Inst) -> Module {
    let mut b2 = vec![mk(Some(3), InstKind::Load(Type::Ptr, v(0), 32))];
    b2.extend(body);
    b2.push(mk(
        Some(4),
        InstKind::ICmp(IPred::Eq, v(3), Operand::ConstPtrNull),
    ));
    let mut blocks = entry;
    blocks.extend([
        block(
            1,
            vec![
                mk(Some(1), InstKind::Load(Type::I64, v(0), 8)),
                mk(
                    Some(2),
                    InstKind::ICmp(IPred::Ult, Operand::ConstI64(0), v(1)),
                ),
            ],
            cond(2, 2, 5),
        ),
        block(2, b2, cond(4, 3, 4)),
        block(3, vec![cold], Terminator::Br(BlockId(1))),
        block(4, vec![], Terminator::Br(BlockId(1))),
        block(5, vec![], Terminator::Ret(None)),
    ]);
    blocks.sort_by_key(|b| b.id.0);
    let ty = |t| ValueInfo { ty: t, name: None };
    let mut values = vec![
        ty(Type::Ptr),
        ty(Type::I64),
        ty(Type::Bool),
        ty(Type::Ptr),
        ty(Type::Bool),
    ];
    values.extend((5..10).map(|_| ty(Type::I64)));
    Module {
        funcs: vec![
            decl("__torajs_arr_alloc"),
            decl("__torajs_arr_typed_set_grow"),
            decl("__torajs_arr_push_unchecked"),
            Function {
                name: "main".into(),
                params: vec![],
                ret: Type::Void,
                blocks,
                values,
                current_origin: None,
            },
        ],
        ..Default::default()
    }
}

fn heap_entry() -> Vec<Block> {
    vec![block(
        0,
        vec![mk(
            Some(0),
            InstKind::Call(FuncId(ALLOC), vec![Operand::ConstI64(8)]),
        )],
        Terminator::Br(BlockId(1)),
    )]
}

fn grow() -> Inst {
    let z = Operand::ConstI64(0);
    call(GROW, vec![v(0), z, z, z])
}

fn loads(b: &Block, off: u64) -> usize {
    b.insts
        .iter()
        .filter(|i| matches!(i.kind, InstKind::Load(_, Operand::Value(ValueId(0)), o) if o == off))
        .count()
}

#[test]
fn a_grow_arm_reads_the_cached_words_again() {
    let mut m = store_loop(heap_entry(), vec![], grow());
    let s = reread_header_words(&mut m, None);
    assert_eq!(
        (s.cells_promoted, s.rereads_placed, s.preheaders_made),
        (2, 1, 0)
    );
    assert_eq!(s.reads_direct, 2);
    let f = &m.funcs[3];
    // the entry reads both words once, the loop only after the grow
    assert_eq!((loads(&f.blocks[0], 8), loads(&f.blocks[0], 32)), (1, 1));
    assert_eq!(loads(&f.blocks[1], 8) + loads(&f.blocks[2], 32), 0);
    assert_eq!((loads(&f.blocks[3], 8), loads(&f.blocks[3], 32)), (1, 1));
    assert!(matches!(f.blocks[3].insts[0].kind, InstKind::Call(..)));
    // the compare now reads the cell
    let InstKind::ICmp(_, _, Operand::Value(c)) = f.blocks[1].insts[0].kind else {
        panic!("the bounds compare stays");
    };
    assert!(f.blocks[0].insts.iter().any(|i| i.result == Some(c)));
}

#[test]
fn any_other_call_on_the_array_keeps_the_loads() {
    let push = call(PUSH, vec![v(0), Operand::ConstI64(1)]);
    let mut m = store_loop(heap_entry(), vec![push], grow());
    assert_eq!(reread_header_words(&mut m, None).cells_promoted, 0);
    // and a loop with no kernel call is load_licm's
    let mut m = store_loop(heap_entry(), vec![], mk(None, InstKind::Identity(v(1))));
    assert_eq!(reread_header_words(&mut m, None).cells_promoted, 0);
}

#[test]
fn two_ways_in_get_a_preheader() {
    let entry = vec![
        block(
            0,
            vec![mk(
                Some(0),
                InstKind::Call(FuncId(ALLOC), vec![Operand::ConstI64(8)]),
            )],
            Terminator::CondBr {
                cond: Operand::ConstBool(true),
                then_blk: BlockId(1),
                else_blk: BlockId(6),
            },
        ),
        block(6, vec![], Terminator::Br(BlockId(1))),
    ];
    let mut m = store_loop(entry, vec![], grow());
    assert_eq!(reread_header_words(&mut m, None).preheaders_made, 1);
    let f = &m.funcs[3];
    let pre = f.blocks.len() as u32 - 1;
    assert_eq!(f.blocks[pre as usize].term, Terminator::Br(BlockId(1)));
    assert_eq!(loads(&f.blocks[pre as usize], 8), 1);
    let Terminator::CondBr { then_blk, .. } = f.blocks[0].term else {
        panic!("entry still branches");
    };
    assert_eq!(then_blk, BlockId(pre));
    assert_eq!(f.blocks[6].term, Terminator::Br(BlockId(pre)));
}

/// `%5 = load A+0 · %6 = and %5, mask` in the loop body.
fn flag_test(mask: i64) -> Vec<Inst> {
    vec![
        mk(Some(5), InstKind::Load(Type::I64, v(0), 0)),
        mk(
            Some(6),
            InstKind::BinOp(BinOp::And, v(5), Operand::ConstI64(mask)),
        ),
        mk(
            Some(7),
            InstKind::ICmp(IPred::Ne, v(6), Operand::ConstI64(0)),
        ),
    ]
}

#[test]
fn a_flag_test_is_cached_as_its_mask() {
    let mut m = store_loop(heap_entry(), flag_test(MASK), grow());
    assert_eq!(reread_header_words(&mut m, None).cells_promoted, 3);
    let f = &m.funcs[3];
    assert_eq!(loads(&f.blocks[2], 0), 0);
    let masks = |b: &Block| {
        b.insts
            .iter()
            .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::And, _, Operand::ConstI64(k)) if k == MASK))
            .count()
    };
    assert_eq!(
        (
            masks(&f.blocks[0]),
            masks(&f.blocks[2]),
            masks(&f.blocks[3])
        ),
        (1, 0, 1)
    );
    // the refcount half, or a bit the cycle collector flips, keeps the
    // flags word in memory; the length and data pointer still move
    for mask in [MASK | 1, 1 << 53] {
        let mut m = store_loop(heap_entry(), flag_test(mask), grow());
        assert_eq!(reread_header_words(&mut m, None).cells_promoted, 2);
        assert_eq!(loads(&m.funcs[3].blocks[2], 0), 1);
    }
    // a store into the flags half of the word dirties the whole word
    let mut body = flag_test(MASK);
    body.push(mk(None, InstKind::Store(Operand::ConstI64(1), v(0), 6)));
    let mut m = store_loop(heap_entry(), body, grow());
    reread_header_words(&mut m, None);
    assert_eq!(loads(&m.funcs[3].blocks[2], 0), 1);
}

/// `%0 = alloca_bytes 56` born as lowering writes an array literal:
/// header, capacity, props, data pointer `%0 + data`, length.
fn frame_birth(data: i64) -> Vec<Block> {
    let store = |v: Operand, off| mk(None, InstKind::Store(v, Operand::Value(ValueId(0)), off));
    let birth = vec![
        mk(Some(0), InstKind::AllocaBytes(56)),
        store(Operand::ConstI64(1 << 50), 0),
        store(Operand::ConstI64(2), 16),
        store(Operand::ConstI64(0), 24),
        mk(
            Some(8),
            InstKind::BinOp(BinOp::Add, v(0), Operand::ConstI64(data)),
        ),
        store(v(8), 32),
        store(Operand::ConstI64(2), 8),
    ];
    vec![block(0, birth, Terminator::Br(BlockId(1)))]
}

#[test]
fn a_frame_cell_literal_is_tracked() {
    let promoted = |data: i64| {
        let mut m = store_loop(frame_birth(data), vec![], grow());
        m.funcs[3].values[0].ty = Type::Arr(torajs_core::ssa::ArrId(0));
        reread_header_words(&mut m, None).cells_promoted
    };
    assert_eq!(promoted(40), 2);
    // an address into the cell other than its inline slots escapes
    assert_eq!(promoted(48), 0);
}
