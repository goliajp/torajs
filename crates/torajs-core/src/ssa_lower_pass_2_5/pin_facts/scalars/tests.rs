//! Hand-built clusters of class sid 0 `{toks, pos, depth, ratio}`:
//! which words each criterion keeps out.

use super::census;
use crate::ast::PropKey;
use crate::ssa::{
    ArrId, BlockId, DataGlobal, FuncId, Function, InstKind, Module, Operand, PinScalar, StructId,
    Terminator, Type,
};
use crate::ssa_lower::OBJ_HEADER_SIZE;

const CELL: Type = Type::Obj(StructId(0));
const POS: u64 = OBJ_HEADER_SIZE + 8;
const DEPTH: u64 = OBJ_HEADER_SIZE + 16;
const RATIO: u64 = OBJ_HEADER_SIZE + 24;

fn pos() -> PinScalar {
    PinScalar::Field {
        off: POS,
        ty: Type::I64,
    }
}

fn nodes() -> PinScalar {
    PinScalar::Global {
        sym: "nodes".into(),
        ty: Type::I64,
    }
}

fn module(funcs: Vec<Function>) -> Module {
    Module {
        struct_layouts: vec![vec![
            (PropKey::from("toks"), Type::Arr(ArrId(0))),
            (PropKey::from("pos"), Type::I64),
            (PropKey::from("depth"), Type::I64),
            (PropKey::from("ratio"), Type::F64),
        ]],
        arr_layouts: vec![Type::I64].into(),
        data_globals: vec![DataGlobal {
            name: "nodes".into(),
            ty: Type::I64,
        }],
        funcs,
        ..Default::default()
    }
}

/// A function of `params`, its body built by `build` over its
/// parameters, returning nothing.
fn func(
    name: &str,
    params: &[Type],
    build: impl FnOnce(&mut Function, BlockId, &[Operand]),
) -> Function {
    let mut f = Function::new(name, Type::Void);
    let ps: Vec<Operand> = (params.iter())
        .map(|t| Operand::Value(f.add_param(*t, "p")))
        .collect();
    let b = f.add_block();
    build(&mut f, b, &ps);
    f.set_term(b, Terminator::Ret(None));
    f
}

/// `cell.off += 1` at type `ty`.
fn bump(f: &mut Function, b: BlockId, cell: &Operand, off: u64, ty: Type) {
    let v = f.append_inst(b, InstKind::Load(ty, *cell, off), ty, None);
    f.append_void(b, InstKind::Store(Operand::Value(v), *cell, off));
}

/// `nodes += 1`.
fn count(f: &mut Function, b: BlockId) {
    let g = f.append_inst(b, InstKind::GlobalRef("nodes".into()), Type::Ptr, None);
    bump(f, b, &Operand::Value(g), 0, Type::I64);
}

/// Member 0 bumps `pos` and `nodes` and calls member 1 on its
/// receiver, then runs `more`; member 1 reads `pos`.
fn members(more: impl FnOnce(&mut Function, BlockId, &[Operand])) -> Vec<Function> {
    let m0 = func("__cm_P__expr", &[CELL], |f, b, ps| {
        bump(f, b, &ps[0], POS, Type::I64);
        count(f, b);
        f.append_void(b, InstKind::Call(FuncId(1), vec![ps[0]]));
        more(f, b, ps);
    });
    let m1 = func("__cm_P__term", &[CELL], |f, b, ps| {
        f.append_inst(b, InstKind::Load(Type::I64, ps[0], POS), Type::I64, None);
    });
    vec![m0, m1]
}

fn run(funcs: Vec<Function>) -> Vec<PinScalar> {
    let m = module(funcs);
    let skip = vec![false; m.funcs.len()];
    census(&m, StructId(0), &[FuncId(0), FuncId(1)], &skip, None, "t")
}

#[test]
fn the_cursor_and_the_counter_are_scalars_most_accessed_first() {
    assert_eq!(run(members(|_, _, _| {})), vec![pos(), nodes()]);
}

#[test]
fn a_helper_reading_the_field_keeps_it_in_memory() {
    let mut fs = members(|f, b, ps| {
        f.append_void(
            b,
            InstKind::Call(FuncId(2), vec![Operand::ConstI64(1), ps[0]]),
        );
    });
    fs.push(func("peek", &[Type::I64, CELL], |f, b, ps| {
        f.append_inst(b, InstKind::Load(Type::I64, ps[1], POS), Type::I64, None);
    }));
    assert_eq!(run(fs), vec![nodes()]);
}

#[test]
fn a_helper_entering_the_cluster_again_keeps_every_word_in_memory() {
    let mut fs = members(|f, b, ps| {
        f.append_void(
            b,
            InstKind::Call(FuncId(2), vec![Operand::ConstI64(1), ps[0]]),
        );
    });
    fs.push(func("nested", &[Type::I64, CELL], |f, b, ps| {
        f.append_void(b, InstKind::Call(FuncId(0), vec![ps[1]]));
    }));
    assert_eq!(run(fs), vec![]);
}

#[test]
fn a_member_taking_a_members_address_keeps_every_word_in_memory() {
    let fs = members(|f, b, _| {
        f.append_inst(b, InstKind::FnAddr(FuncId(1)), Type::Ptr, None);
    });
    assert_eq!(run(fs), vec![]);
}

#[test]
fn a_member_reading_another_cells_field_keeps_it_in_memory() {
    let m0 = func("__cm_P__expr", &[CELL, CELL], |f, b, ps| {
        bump(f, b, &ps[0], POS, Type::I64);
        count(f, b);
        f.append_inst(b, InstKind::Load(Type::I64, ps[1], POS), Type::I64, None);
    });
    let m1 = func("__cm_P__term", &[CELL], |_, _, _| {});
    assert_eq!(run(vec![m0, m1]), vec![nodes()]);
}

#[test]
fn a_helper_reading_the_global_keeps_it_in_memory() {
    let mut fs = members(|f, b, _| {
        f.append_void(b, InstKind::Call(FuncId(2), vec![]));
    });
    fs.push(func("seen", &[], |f, b, _| {
        let g = f.append_inst(b, InstKind::GlobalRef("nodes".into()), Type::Ptr, None);
        f.append_inst(
            b,
            InstKind::Load(Type::I64, Operand::Value(g), 0),
            Type::I64,
            None,
        );
    }));
    assert_eq!(run(fs), vec![pos()]);
}

#[test]
fn the_globals_address_passed_anywhere_keeps_it_in_memory() {
    // the function the address goes to does not run with the cluster:
    // the escape alone is enough
    let mut fs = members(|_, _, _| {});
    fs.push(func("main", &[], |f, b, _| {
        let g = f.append_inst(b, InstKind::GlobalRef("nodes".into()), Type::Ptr, None);
        f.append_void(b, InstKind::Call(FuncId(3), vec![Operand::Value(g)]));
    }));
    fs.push(func("sink", &[Type::Ptr], |_, _, _| {}));
    assert_eq!(run(fs), vec![pos()]);
}

#[test]
fn a_float_field_is_no_scalar_and_a_third_word_is_left_out() {
    let fs = members(|f, b, ps| {
        bump(f, b, &ps[0], RATIO, Type::F64);
        bump(f, b, &ps[0], DEPTH, Type::I64);
        bump(f, b, &ps[0], DEPTH, Type::I64);
    });
    let depth = PinScalar::Field {
        off: DEPTH,
        ty: Type::I64,
    };
    assert_eq!(run(fs), vec![depth, pos()]);
}
