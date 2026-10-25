use super::tests::{any, body, cell, module, open};
use super::*;
use crate::ssa::FuncId;

/// Layouts 3 `{k: 1, tag}` and 4 `{k: 0}`: 4 is a prefix of 3 by
/// name, and their `k` is one slot under two cell types.
fn nested(m: &mut Module) {
    m.struct_layouts.push(vec![
        (PropKey::from("k"), Type::Obj(StructId(1))),
        (PropKey::from("tag"), Type::I64),
    ]);
    m.struct_layouts
        .push(vec![(PropKey::from("k"), Type::Obj(StructId(0)))]);
}

#[test]
fn a_shared_field_typed_differently_by_two_views_opens_both_types() {
    let mut m = module();
    nested(&mut m);
    body(&mut m, |f, entry| {
        let wide = cell(f, entry, 3);
        let slot = f.append_inst(
            entry,
            InstKind::Alloca(Type::Obj(StructId(4))),
            Type::Ptr,
            None,
        );
        f.append_void(entry, InstKind::Store(wide, Operand::Value(slot), 0));
    });
    assert_eq!(open(&m), vec![true, true, false, false, false]);
    // an array of the wider layout behind an array slot of the
    // narrower one: its elements are such slots
    let mut m = module();
    let narrow = m.intern_arr(Type::Obj(StructId(0)));
    let wide = m.intern_arr(Type::Obj(StructId(1)));
    body(&mut m, |f, entry| {
        let slot = f.append_inst(entry, InstKind::Alloca(Type::Arr(narrow)), Type::Ptr, None);
        let xs = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(wide), None);
        f.append_void(
            entry,
            InstKind::Store(Operand::Value(xs), Operand::Value(slot), 0),
        );
    });
    assert_eq!(open(&m), vec![true, true, false]);
}

/// A call's answer typed narrower than the callee's declared
/// return, and the parameter of a function entered through its
/// address, are paths from the wider layout.
#[test]
fn a_narrower_call_answer_and_an_address_taken_parameter_are_paths() {
    let opener = |m: &mut Module| {
        body(m, |f, entry| {
            let slot = f.append_inst(
                entry,
                InstKind::Alloca(Type::Obj(StructId(1))),
                Type::Ptr,
                None,
            );
            let v = any(f, entry);
            f.append_void(entry, InstKind::Store(v, Operand::Value(slot), 0));
        });
    };
    let mut m = module();
    let mut make = Function::new("make", Type::Obj(StructId(1)));
    let entry = make.add_block();
    let c = cell(&mut make, entry, 1);
    make.set_term(entry, Terminator::Ret(Some(c)));
    let make_id = FuncId(m.funcs.len() as u32);
    m.funcs.push(make);
    opener(&mut m);
    body(&mut m, |f, entry| {
        f.append_inst(
            entry,
            InstKind::Call(make_id, vec![]),
            Type::Obj(StructId(0)),
            None,
        );
    });
    assert_eq!(open(&m), vec![true, true, false]);
    let mut m = module();
    let mut take = Function::new("take", Type::Void);
    take.add_param(Type::Obj(StructId(0)), "a");
    let entry = take.add_block();
    take.set_term(entry, Terminator::Ret(None));
    let take_id = FuncId(m.funcs.len() as u32);
    m.funcs.push(take);
    opener(&mut m);
    assert_eq!(open(&m), vec![false, true, false]);
    body(&mut m, |f, entry| {
        f.append_inst(entry, InstKind::FnAddr(take_id), Type::Ptr, None);
    });
    assert_eq!(open(&m), vec![true, true, false]);
}

/// An address-taken function's cell parameter takes cells of every
/// extending layout (a path); its array parameter takes arrays of
/// them, whose elements are slots under both layouts (both open).
#[test]
fn a_blind_array_parameter_aligns_its_elements_with_every_extension() {
    let taken = |m: &mut Module, param: Type| {
        let mut take = Function::new("take", Type::Void);
        take.add_param(param, "xs");
        let entry = take.add_block();
        take.set_term(entry, Terminator::Ret(None));
        let take_id = FuncId(m.funcs.len() as u32);
        m.funcs.push(take);
        body(m, |f, entry| {
            f.append_inst(entry, InstKind::FnAddr(take_id), Type::Ptr, None);
        });
    };
    let mut m = module();
    taken(&mut m, Type::Obj(StructId(0)));
    assert_eq!(open(&m), vec![false, false, false]);
    let mut m = module();
    let xs = m.intern_arr(Type::Obj(StructId(0)));
    taken(&mut m, Type::Arr(xs));
    assert_eq!(open(&m), vec![true, true, false]);
    let mut m = module();
    let ys = m.intern_arr(Type::Obj(StructId(2)));
    taken(&mut m, Type::Arr(ys));
    assert_eq!(open(&m), vec![false, false, false]);
}

/// A boxed adapter nothing references never runs: the `any` it
/// hands a typed parameter crosses nothing. Once its address is
/// taken it does.
#[test]
fn an_unreferenced_boxed_adapter_crosses_nothing() {
    let mut m = module();
    let mut callee = Function::new("take", Type::Void);
    callee.add_param(Type::Obj(StructId(2)), "b");
    let entry = callee.add_block();
    callee.set_term(entry, Terminator::Ret(None));
    let take = FuncId(m.funcs.len() as u32);
    m.funcs.push(callee);
    let mut adapter = Function::new("__boxed_take", Type::Void);
    let entry = adapter.add_block();
    let v = any(&mut adapter, entry);
    adapter.append_void(entry, InstKind::Call(take, vec![v]));
    adapter.set_term(entry, Terminator::Ret(None));
    let adapter_id = FuncId(m.funcs.len() as u32);
    m.funcs.push(adapter);
    assert_eq!(open(&m), vec![false, false, false]);
    body(&mut m, |f, entry| {
        f.append_inst(entry, InstKind::FnAddr(adapter_id), Type::Ptr, None);
    });
    assert_eq!(open(&m), vec![false, false, true]);
}

/// A boxed adapter decodes an `any` into a cell or array slot only
/// behind its layout test; the decode is exact. The same decode with
/// no test, or outside an adapter, opens.
#[test]
fn a_decode_behind_an_adapters_layout_test_is_exact() {
    let arr_of = |m: &mut Module| m.intern_arr(Type::Obj(StructId(2)));
    let build = |m: &mut Module, name: &str, slot: Type, test: Option<FuncId>| {
        let mut callee = Function::new("take", Type::Void);
        callee.add_param(slot, "b");
        let entry = callee.add_block();
        callee.set_term(entry, Terminator::Ret(None));
        let take = FuncId(m.funcs.len() as u32);
        m.funcs.push(callee);
        let mut adapter = Function::new(name, Type::Void);
        let entry = adapter.add_block();
        let av = any(&mut adapter, entry);
        if let Some(test) = test {
            adapter.append_inst(
                entry,
                InstKind::Call(test, vec![av, Operand::ConstI64(7)]),
                Type::I64,
                None,
            );
        }
        let bits = adapter.append_inst(entry, InstKind::PtrToInt(av), Type::I64, None);
        let ptr = adapter.append_inst(entry, InstKind::IntToPtr(Operand::Value(bits)), slot, None);
        adapter.append_void(entry, InstKind::Call(take, vec![Operand::Value(ptr)]));
        adapter.set_term(entry, Terminator::Ret(None));
        let adapter_id = FuncId(m.funcs.len() as u32);
        m.funcs.push(adapter);
        body(m, |f, entry| {
            f.append_inst(entry, InstKind::FnAddr(adapter_id), Type::Ptr, None);
        });
    };
    let mut m = module();
    build(
        &mut m,
        "__boxed_take",
        Type::Obj(StructId(2)),
        Some(FuncId(4)),
    );
    assert_eq!(open(&m), vec![false, false, false]);
    // a cell decoded into a base-typed slot may be of an extending
    // layout: the decode is a path from it
    let mut m = module();
    build(
        &mut m,
        "__boxed_take",
        Type::Obj(StructId(0)),
        Some(FuncId(4)),
    );
    let (open_base, widens) = open_sids(&m, &EnvLayouts::new());
    assert_eq!(open_base, vec![false, false, false]);
    assert!(widens.contains(&(StructId(1), StructId(0))));
    let mut m = module();
    let a = arr_of(&mut m);
    build(&mut m, "__boxed_take", Type::Arr(a), Some(FuncId(5)));
    assert_eq!(open(&m), vec![false, false, false]);
    // the test proves the elements extend the slot's layout, not the
    // caller's static element type: with an extending layout around,
    // the elements are slots under both and both open
    let mut m = module();
    let base = m.intern_arr(Type::Obj(StructId(0)));
    build(&mut m, "__boxed_take", Type::Arr(base), Some(FuncId(5)));
    assert_eq!(open(&m), vec![true, true, false]);
    let mut m = module();
    build(&mut m, "__boxed_take", Type::Obj(StructId(2)), None);
    assert_eq!(open(&m), vec![false, false, true]);
    let mut m = module();
    let a = arr_of(&mut m);
    build(&mut m, "plain", Type::Arr(a), Some(FuncId(5)));
    assert_eq!(open(&m), vec![false, false, true]);
}
