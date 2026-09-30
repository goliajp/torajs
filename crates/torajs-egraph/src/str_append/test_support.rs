//! Modules for the append tests: every kernel declared at a fixed
//! index, then a body function holding the instructions under test.

use torajs_core::ssa::{
    Block, BlockId, FuncId, Function, Inst, InstKind, Module, Operand, StringId, Terminator, Type,
    ValueId, ValueInfo,
};

pub(crate) const STR_CONCAT: FuncId = FuncId(0);
pub(crate) const STR_DROP: FuncId = FuncId(1);
pub(crate) const STR_APPEND: FuncId = FuncId(2);
pub(crate) const SUBSTR_CONCAT_SUBSTR_STR: FuncId = FuncId(3);
pub(crate) const SUBSTR_CONCAT_STR_SUBSTR: FuncId = FuncId(4);
pub(crate) const SUBSTR_CONCAT_SUBSTR_SUBSTR: FuncId = FuncId(5);
pub(crate) const STR_APPEND_SUBSTR: FuncId = FuncId(6);
pub(crate) const SUBSTR_DROP: FuncId = FuncId(7);
/// Index of the body function in `module_with`.
pub(crate) const BODY: usize = 8;

const KERNELS: [&str; 8] = [
    "__torajs_str_concat",
    "__torajs_str_drop",
    "__torajs_str_append",
    "__torajs_substr_concat_substr_str",
    "__torajs_substr_concat_str_substr",
    "__torajs_substr_concat_substr_substr",
    "__torajs_str_append_substr",
    "__torajs_substr_drop",
];

pub(crate) fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

fn decl(name: &str) -> Function {
    Function {
        name: name.into(),
        params: vec![],
        ret: Type::Str,
        blocks: vec![],
        values: vec![],
        current_origin: None,
    }
}

/// The kernels plus a body of one block holding `insts`.
pub(crate) fn module_with(insts: Vec<Inst>) -> Module {
    module_with_blocks(vec![insts], Terminator::Ret(None))
}

/// The kernels plus a body whose blocks each branch to the next; the
/// last one ends in `last`.
pub(crate) fn module_with_blocks(blocks: Vec<Vec<Inst>>, last: Terminator) -> Module {
    let values = (0..32)
        .map(|_| ValueInfo {
            ty: Type::Str,
            name: None,
        })
        .collect();
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
        params: vec![],
        ret: Type::Str,
        blocks,
        values,
        current_origin: None,
    };
    let mut funcs: Vec<Function> = KERNELS.iter().map(|n| decl(n)).collect();
    funcs.push(body);
    Module {
        funcs,
        ..Module::default()
    }
}

pub(crate) fn call(fid: FuncId, result: u32, args: &[u32]) -> Inst {
    Inst {
        result: Some(ValueId(result)),
        kind: InstKind::Call(fid, args.iter().map(|&a| v(a)).collect()),
        origin: None,
    }
}

pub(crate) fn concat(result: u32, left: u32, right: u32) -> Inst {
    call(STR_CONCAT, result, &[left, right])
}

pub(crate) fn drop_of(x: u32) -> Inst {
    Inst {
        result: None,
        kind: InstKind::Call(STR_DROP, vec![v(x)]),
        origin: None,
    }
}

pub(crate) fn static_str(result: u32) -> Inst {
    Inst {
        result: Some(ValueId(result)),
        kind: InstKind::StaticStrRef(StringId(0)),
        origin: None,
    }
}

/// `(callee, result, args)` of every call in the body's block `blk`,
/// in order — the shape a test asserts on.
pub(crate) fn calls(m: &Module, blk: usize) -> Vec<(FuncId, Option<u32>, Vec<u32>)> {
    m.funcs[BODY].blocks[blk]
        .insts
        .iter()
        .filter_map(|i| match &i.kind {
            InstKind::Call(fid, args) => Some((
                *fid,
                i.result.map(|r| r.0),
                args.iter()
                    .map(|a| match a {
                        Operand::Value(x) => x.0,
                        _ => u32::MAX,
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}
