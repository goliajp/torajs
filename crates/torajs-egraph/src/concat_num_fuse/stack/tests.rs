use torajs_core::ssa::{
    Block, BlockId, FuncId, Function, Inst, InstKind, Module, Operand, StringId, StringLiteral,
    Terminator, Type, ValueId, ValueInfo,
};

use super::stack_temps;

const DROP: FuncId = FuncId(0);
const TO_STR: FuncId = FuncId(1);
const CONCAT: FuncId = FuncId(2);
const MATCH_LEN: FuncId = FuncId(3);
const MAP_SET: FuncId = FuncId(4);
const BODY: usize = 5;
const KERNELS: [&str; 5] = [
    "__torajs_str_drop",
    "__torajs_i64_to_str",
    "__torajs_str_concat_i64",
    "__torajs_str_match_len",
    "__torajs_map_set",
];

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

fn inst(result: Option<u32>, kind: InstKind) -> Inst {
    Inst {
        result: result.map(ValueId),
        kind,
        origin: None,
    }
}

fn call(result: u32, fid: FuncId, args: &[Operand]) -> Inst {
    inst(Some(result), InstKind::Call(fid, args.to_vec()))
}

fn void(fid: FuncId, args: &[Operand]) -> Inst {
    inst(None, InstKind::Call(fid, args.to_vec()))
}

fn drop_of(id: u32) -> Inst {
    void(DROP, &[v(id)])
}

fn lit(result: u32, sid: u32) -> Inst {
    inst(Some(result), InstKind::StaticStrRef(StringId(sid)))
}

fn len_of(result: u32, s: u32) -> Inst {
    inst(Some(result), InstKind::Load(Type::I64, v(s), 8))
}

/// Value 0 is the body's one parameter (a number, or a string to
/// copy in); literal 0 is short, literal 1 past the frame cap.
fn module(blocks: Vec<Vec<Inst>>, last: Terminator) -> Module {
    let n = blocks.len();
    let blocks = blocks
        .into_iter()
        .enumerate()
        .map(|(i, insts)| Block {
            id: BlockId(i as u32),
            insts,
            term: if i + 1 == n {
                last.clone()
            } else {
                Terminator::Br(BlockId(i as u32 + 1))
            },
        })
        .collect();
    let body = Function {
        name: "f".into(),
        params: vec![ValueId(0)],
        ret: Type::Str,
        blocks,
        values: (0..32)
            .map(|_| ValueInfo {
                ty: Type::Str,
                name: None,
            })
            .collect(),
        current_origin: None,
    };
    let mut funcs: Vec<Function> = KERNELS
        .iter()
        .map(|&name| Function::new(name, Type::Str))
        .collect();
    funcs.push(body);
    Module {
        funcs,
        strings: vec![
            StringLiteral::encode_from_str("hello world "),
            StringLiteral::encode_from_str(&"x".repeat(300)),
        ],
        ..Module::default()
    }
}

fn body(m: &Module) -> &Function {
    &m.funcs[BODY]
}

fn callee(m: &Module, i: &Inst) -> Option<String> {
    match &i.kind {
        InstKind::Call(fid, _) => Some(m.funcs[fid.0 as usize].name.clone()),
        _ => None,
    }
}

fn has_drop(m: &Module) -> bool {
    body(m)
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .any(|i| matches!(&i.kind, InstKind::Call(fid, _) if *fid == DROP))
}

#[test]
fn a_length_read_moves_to_the_frame() {
    let mut m = module(
        vec![vec![call(1, TO_STR, &[v(0)]), len_of(2, 1), drop_of(1)]],
        Terminator::Ret(None),
    );
    assert_eq!(stack_temps(&mut m), 1);
    let insts = &body(&m).blocks[0].insts;
    assert!(matches!(insts[0].kind, InstKind::AllocaBytes(40)));
    assert_eq!(
        callee(&m, &insts[1]).as_deref(),
        Some("__torajs_i64_to_str_into")
    );
    let InstKind::Call(_, args) = &insts[1].kind else {
        unreachable!()
    };
    assert_eq!(args, &vec![v(0), Operand::Value(insts[0].result.unwrap())]);
    assert!(!has_drop(&m));
}

#[test]
fn a_literal_prefix_sizes_the_slot_and_a_match_len_reads_it() {
    let mut m = module(
        vec![
            vec![lit(1, 0), call(2, CONCAT, &[v(1), v(0)])],
            vec![drop_of(2)],
        ],
        Terminator::Ret(None),
    );
    m.funcs[BODY].blocks[0]
        .insts
        .push(call(3, MATCH_LEN, &[v(2), v(0)]));
    assert_eq!(stack_temps(&mut m), 1);
    let insts = &body(&m).blocks[0].insts;
    // 16 + 12 + 20, rounded to 8
    assert!(matches!(insts[1].kind, InstKind::AllocaBytes(48)));
    assert_eq!(
        callee(&m, &insts[2]).as_deref(),
        Some("__torajs_str_concat_i64_into")
    );
    assert!(!has_drop(&m));
}

/// A value whose one def is the mint reads that mint's latest bytes
/// wherever it is read; the drop on a separate path goes too.
#[test]
fn a_single_def_value_is_read_across_blocks() {
    let mut m = module(
        vec![
            vec![call(1, TO_STR, &[v(0)])],
            vec![len_of(2, 1)],
            vec![call(3, MATCH_LEN, &[v(1), v(0)]), drop_of(1)],
        ],
        Terminator::Ret(None),
    );
    assert_eq!(stack_temps(&mut m), 1);
    assert!(!has_drop(&m));
}

#[test]
fn a_copy_carried_value_is_followed_to_its_drop() {
    let mut m = module(
        vec![
            vec![call(1, TO_STR, &[v(0)]), len_of(2, 1)],
            vec![inst(Some(3), InstKind::Copy(Type::Str, v(1)))],
            vec![drop_of(3)],
        ],
        Terminator::Ret(None),
    );
    assert_eq!(stack_temps(&mut m), 1);
    assert!(!has_drop(&m));
}

/// Each shape that may keep the cell, or read it where its bytes are
/// not known to be fresh, leaves it on the heap.
#[test]
fn anything_that_may_keep_it_stays_on_the_heap() {
    let escapes: Vec<(Vec<Vec<Inst>>, Terminator)> = vec![
        // a Map key
        (
            vec![vec![
                call(1, TO_STR, &[v(0)]),
                void(MAP_SET, &[v(0), v(1)]),
                drop_of(1),
            ]],
            Terminator::Ret(None),
        ),
        // the return value
        (
            vec![vec![call(1, TO_STR, &[v(0)])]],
            Terminator::Ret(Some(v(1))),
        ),
        // an array slot
        (
            vec![vec![
                call(1, TO_STR, &[v(0)]),
                inst(None, InstKind::Store(v(1), v(0), 16)),
            ]],
            Terminator::Ret(None),
        ),
        // a read in a later block of a value that also takes a copy
        (
            vec![
                vec![call(1, TO_STR, &[v(0)]), call(2, TO_STR, &[v(0)])],
                vec![inst(Some(1), InstKind::Copy(Type::Str, v(2))), len_of(3, 2)],
                vec![len_of(4, 1)],
            ],
            Terminator::Ret(None),
        ),
        // a read through a copy
        (
            vec![vec![
                call(1, TO_STR, &[v(0)]),
                inst(Some(2), InstKind::Copy(Type::Str, v(1))),
                len_of(3, 2),
            ]],
            Terminator::Ret(None),
        ),
        // a copy joining a heap string into the set
        (
            vec![
                vec![call(1, TO_STR, &[v(0)]), len_of(2, 1)],
                vec![inst(Some(1), InstKind::Copy(Type::Str, v(0)))],
            ],
            Terminator::Ret(None),
        ),
        // a prefix that is not a literal
        (
            vec![vec![call(1, CONCAT, &[v(0), v(0)]), drop_of(1)]],
            Terminator::Ret(None),
        ),
        // a literal too long for the frame
        (
            vec![vec![lit(1, 1), call(2, CONCAT, &[v(1), v(0)]), drop_of(2)]],
            Terminator::Ret(None),
        ),
        // a reader in a slot it does not only read
        (
            vec![vec![
                call(1, TO_STR, &[v(0)]),
                call(2, MATCH_LEN, &[v(0), v(1)]),
                drop_of(1),
            ]],
            Terminator::Ret(None),
        ),
    ];
    for (i, (blocks, last)) in escapes.into_iter().enumerate() {
        let mut m = module(blocks, last);
        let before = format!("{:?}", body(&m).blocks);
        assert_eq!(stack_temps(&mut m), 0, "case {i}");
        assert_eq!(format!("{:?}", body(&m).blocks), before, "case {i}");
    }
}

const VIEW_CONCAT: FuncId = FuncId(BODY as u32 + 1);
const CHAR_CODE_AT: FuncId = FuncId(BODY as u32 + 2);

/// [`module`] with the view concat and a reader declared after the
/// body, and a one-byte literal `|` as literal 2.
fn view_module(insts: Vec<Inst>) -> Module {
    let mut m = module(vec![insts], Terminator::Ret(None));
    for name in [
        "__torajs_substr_concat_substr_str",
        "__torajs_str_char_code_at",
    ] {
        m.funcs.push(Function::new(name, Type::Str));
    }
    m.strings.push(StringLiteral::encode_from_str("|"));
    m
}

#[test]
fn the_view_literal_slot_is_the_largest_pooled_payload() {
    // torajs-str's STR_DATA_OFF + STR_POOL_PAYLOAD, which its own
    // concat_lit tests pin to the same number
    assert_eq!(super::VIEW_LIT_CELL, 16 + 64);
}

#[test]
fn a_view_and_literal_read_only_by_char_code_at_moves_and_keeps_its_drop() {
    let mut m = view_module(vec![
        lit(2, 2),
        call(3, VIEW_CONCAT, &[v(0), v(2)]),
        call(4, CHAR_CODE_AT, &[v(3), Operand::ConstI64(0)]),
        drop_of(3),
    ]);
    assert_eq!(stack_temps(&mut m), 1);
    let insts = &body(&m).blocks[0].insts;
    assert!(
        !insts
            .iter()
            .any(|i| matches!(i.kind, InstKind::StaticStrRef(_))),
        "the literal travels as immediates"
    );
    assert!(matches!(insts[0].kind, InstKind::AllocaBytes(80)));
    assert_eq!(
        callee(&m, &insts[1]).as_deref(),
        Some("__torajs_substr_concat_lit_into")
    );
    let InstKind::Call(_, args) = &insts[1].kind else {
        unreachable!()
    };
    let meta = 1 << 4 | 1 << 1 | 1 << 12;
    assert_eq!(
        args,
        &vec![
            v(0),
            Operand::ConstI64(i64::from(b'|')),
            Operand::ConstI64(meta),
            Operand::Value(insts[0].result.unwrap()),
        ]
    );
    assert!(has_drop(&m), "a longer view answers a heap cell");
}

#[test]
fn a_stored_view_and_literal_stays_on_the_heap() {
    let mut m = view_module(vec![
        lit(2, 2),
        call(3, VIEW_CONCAT, &[v(0), v(2)]),
        inst(None, InstKind::Store(v(3), v(0), 16)),
    ]);
    assert_eq!(stack_temps(&mut m), 0);
    assert_eq!(
        callee(&m, &body(&m).blocks[0].insts[1]).as_deref(),
        Some("__torajs_substr_concat_substr_str")
    );
}
