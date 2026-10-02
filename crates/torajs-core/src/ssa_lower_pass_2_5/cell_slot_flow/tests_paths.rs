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
