//! Hand-built modules in which sid 0 is a value class held in place by
//! array layout 1: the shapes the inline lowering emits agree, and each
//! way a view or an inline array could be told from a cell is reported.

use crate::ssa::{ArrId, BlockId, FuncId, Function, IPred, InstKind, Operand, StructId, Type};

use super::super::tests::{found, func, module, reports};

const SORT: FuncId = FuncId(0);
const PUSH: FuncId = FuncId(1);
const RESERVE: FuncId = FuncId(3);
const DROP_SCALAR: FuncId = FuncId(4);
const NULLISH: FuncId = FuncId(5);
const EXTEND_GUARD: FuncId = FuncId(6);

fn inline_module() -> crate::ssa::Module {
    let mut m = module();
    m.arr_layouts = vec![Type::Obj(StructId(0)), Type::Inline(StructId(0))].into();
    for (name, params) in [
        ("__torajs_arr_reserve", &[Type::Ptr, Type::I64][..]),
        ("__torajs_arr_drop_scalar", &[Type::Ptr]),
        ("__torajs_heap_nullish_check", &[Type::Ptr]),
        ("__torajs_arr_extend_guard", &[Type::Ptr]),
    ] {
        let mut f = Function::new(name, Type::Void);
        for p in params {
            f.add_param(*p, "p");
        }
        m.funcs.push(f);
    }
    m
}

fn inline_array(f: &mut Function, b: BlockId) -> Operand {
    Operand::Value(f.append_inst(b, InstKind::AllocaBytes(40), Type::Arr(ArrId(1)), None))
}

/// The view of element 0 of `xs`, as `emit_elem_addr` spells it.
fn view(f: &mut Function, b: BlockId, xs: Operand) -> Operand {
    let data = f.append_inst(b, InstKind::Load(Type::Ptr, xs, 32), Type::Ptr, None);
    let off = Operand::ConstI64(0);
    Operand::Value(crate::ssa_lower::inline_elem::emit_elem_addr(
        f,
        b,
        Operand::Value(data),
        off,
        StructId(0),
    ))
}

#[test]
fn the_shapes_of_an_inline_array_agree() {
    let mut m = inline_module();
    func(&mut m, "main", Type::Void, |f, b| {
        let xs = inline_array(f, b);
        f.append_void(b, InstKind::Call(EXTEND_GUARD, vec![xs]));
        f.append_void(b, InstKind::Call(RESERVE, vec![xs, Operand::ConstI64(4)]));
        let t = view(f, b, xs);
        f.append_void(b, InstKind::Store(Operand::ConstF64(1.0), t, 8));
        // a let binding of the view, and the merge slot of a read past
        // the end, which holds the undefined cell typed as the view
        let slot = f.append_inst(
            b,
            InstKind::Alloca(Type::Inline(StructId(0))),
            Type::Ptr,
            None,
        );
        f.append_void(b, InstKind::Store(t, Operand::Value(slot), 0));
        let undef = f.append_inst(
            b,
            InstKind::GlobalRef("___TORAJS_UNDEF_CELL".into()),
            Type::Inline(StructId(0)),
            None,
        );
        f.append_void(
            b,
            InstKind::Store(Operand::Value(undef), Operand::Value(slot), 0),
        );
        let v = f.append_inst(
            b,
            InstKind::Load(Type::Inline(StructId(0)), Operand::Value(slot), 0),
            Type::Inline(StructId(0)),
            None,
        );
        let v = Operand::Value(v);
        // the receiver guard of a field read
        let cell = f.append_inst(
            b,
            InstKind::GlobalRef("___TORAJS_UNDEF_CELL".into()),
            Type::Ptr,
            None,
        );
        for other in [Operand::ConstPtrNull, Operand::Value(cell)] {
            f.append_inst(b, InstKind::ICmp(IPred::Eq, v, other), Type::Bool, None);
        }
        f.append_void(b, InstKind::Call(NULLISH, vec![v]));
        f.append_inst(b, InstKind::Load(Type::F64, v, 8), Type::F64, None);
        f.append_void(b, InstKind::Call(DROP_SCALAR, vec![xs]));
    });
    assert_eq!(found(&m), Vec::<String>::new());
}

#[test]
fn pushing_onto_an_inline_array_through_the_runtime_is_reported() {
    let mut m = inline_module();
    func(&mut m, "f", Type::Void, |f, b| {
        let xs = inline_array(f, b);
        f.append_void(b, InstKind::Call(PUSH, vec![xs, Operand::ConstI64(0)]));
    });
    reports(&m, "passed to __torajs_arr_push");
}

#[test]
fn a_runtime_entry_reading_inline_slots_is_reported() {
    let mut m = inline_module();
    func(&mut m, "f", Type::Void, |f, b| {
        let xs = inline_array(f, b);
        f.append_void(b, InstKind::Call(SORT, vec![xs]));
    });
    reports(&m, "passed to __torajs_arr_sort");
}

#[test]
fn a_view_handed_to_a_call_is_reported() {
    for callee in [SORT, PUSH] {
        let mut m = inline_module();
        func(&mut m, "f", Type::Void, |f, b| {
            let xs = inline_array(f, b);
            let t = view(f, b, xs);
            let args = if callee == PUSH { vec![xs, t] } else { vec![t] };
            f.append_void(b, InstKind::Call(callee, args));
        });
        let got = found(&m);
        assert!(got.iter().any(|d| d.contains("passed")), "{got:?}");
    }
}

#[test]
fn a_view_compared_with_another_is_reported() {
    let mut m = inline_module();
    func(&mut m, "f", Type::Void, |f, b| {
        let xs = inline_array(f, b);
        let a = view(f, b, xs);
        let c = view(f, b, xs);
        f.append_inst(b, InstKind::ICmp(IPred::Eq, a, c), Type::Bool, None);
    });
    reports(&m, "a view compared");
}

#[test]
fn a_view_stored_into_a_cell_slot_is_reported() {
    let mut m = inline_module();
    func(&mut m, "f", Type::Void, |f, b| {
        let xs = inline_array(f, b);
        let t = view(f, b, xs);
        let slot = f.append_inst(b, InstKind::Alloca(Type::Obj(StructId(0))), Type::Ptr, None);
        f.append_void(b, InstKind::Store(t, Operand::Value(slot), 0));
    });
    reports(&m, "a view stored");
}

#[test]
fn a_returned_or_boxed_view_is_reported() {
    let mut m = inline_module();
    func(&mut m, "f", Type::Inline(StructId(0)), |f, b| {
        let xs = inline_array(f, b);
        let t = view(f, b, xs);
        f.set_term(b, crate::ssa::Terminator::Ret(Some(t)));
    });
    reports(&m, "a view returned");
    let mut m = inline_module();
    func(&mut m, "f", Type::Void, |f, b| {
        let xs = inline_array(f, b);
        let t = view(f, b, xs);
        f.append_inst(b, InstKind::BoxAny(4, t), Type::Any, None);
    });
    reports(&m, "boxed");
}
