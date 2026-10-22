//! Hand-built modules: where a cell comes from through the stack slots,
//! which instructions can change an array, and how calls between the
//! methods of a class group them.

use std::collections::HashSet;

use super::Watch;
use super::cluster::Clusters;
use super::grow::site;
use super::roots::{Root, Roots};
use super::sole_names;
use crate::ast::PropKey;
use crate::ssa::{
    ArrId, BlockId, FuncId, Function, InstKind, Module, Operand, StructId, Terminator, Type,
};

/// Layout 0 `{toks: Arr(0)}`, a class; func 0 allocates a cell, func 1
/// is a runtime call that can do anything, func 2 one that tests.
fn module() -> Module {
    let mut m = Module {
        struct_layouts: vec![vec![(PropKey::from("toks"), Type::Arr(ArrId(0)))]],
        arr_layouts: vec![Type::I64].into(),
        ..Default::default()
    };
    for name in [
        "__torajs_obj_alloc",
        "__torajs_arr_sort",
        "__torajs_heap_nullish_check",
    ] {
        let mut f = Function::new(name, Type::Ptr);
        f.add_param(Type::Ptr, "p");
        m.funcs.push(f);
    }
    m
}

const ALLOC: FuncId = FuncId(0);
const SORT: FuncId = FuncId(1);
const CHECK: FuncId = FuncId(2);

fn method(name: &str, build: impl FnOnce(&mut Function, BlockId, Operand)) -> Function {
    let mut f = Function::new(name, Type::Void);
    let this = f.add_param(Type::Obj(StructId(0)), "this");
    let b = f.add_block();
    build(&mut f, b, Operand::Value(this));
    f
}

/// `this` parked in a slot and read back, as lowering spells a method.
fn parked(f: &mut Function, b: BlockId, this: Operand) -> Operand {
    let slot = f.append_inst(b, InstKind::Alloca(Type::Obj(StructId(0))), Type::Ptr, None);
    f.append_void(b, InstKind::Store(this, Operand::Value(slot), 0));
    let v = InstKind::Load(Type::Obj(StructId(0)), Operand::Value(slot), 0);
    Operand::Value(f.append_inst(b, v, Type::Obj(StructId(0)), None))
}

#[test]
fn a_slot_holding_only_the_receiver_reads_as_the_receiver() {
    let mut read = Operand::ConstPtrNull;
    let f = method("m", |f, b, this| read = parked(f, b, this));
    assert_eq!(Roots::of(&f, Some(ALLOC)).root(&read), Root::Receiver);
}

#[test]
fn a_slot_holding_a_fresh_cell_after_null_reads_as_fresh() {
    let mut read = Operand::ConstPtrNull;
    let f = method("m", |f, b, _| {
        let cell = InstKind::Call(ALLOC, vec![Operand::ConstI64(40)]);
        let cell = Operand::Value(f.append_inst(b, cell, Type::Obj(StructId(0)), None));
        let slot = f.append_inst(b, InstKind::Alloca(Type::Obj(StructId(0))), Type::Ptr, None);
        f.append_void(
            b,
            InstKind::Store(Operand::ConstPtrNull, Operand::Value(slot), 0),
        );
        f.append_void(b, InstKind::Store(cell, Operand::Value(slot), 0));
        let v = InstKind::Load(Type::Obj(StructId(0)), Operand::Value(slot), 0);
        read = Operand::Value(f.append_inst(b, v, Type::Obj(StructId(0)), None));
    });
    assert_eq!(Roots::of(&f, Some(ALLOC)).root(&read), Root::Fresh);
}

#[test]
fn a_slot_whose_address_escapes_or_holds_two_roots_is_other() {
    let mut reads = Vec::new();
    let f = method("m", |f, b, this| {
        let r = parked(f, b, this);
        let InstKind::Load(_, slot, _) = f.blocks[0].insts[2].kind.clone() else {
            unreachable!()
        };
        f.append_void(b, InstKind::Call(CHECK, vec![slot]));
        reads.push(r);
        let cell = InstKind::Call(ALLOC, vec![Operand::ConstI64(40)]);
        let cell = Operand::Value(f.append_inst(b, cell, Type::Obj(StructId(0)), None));
        let two = f.append_inst(b, InstKind::Alloca(Type::Obj(StructId(0))), Type::Ptr, None);
        f.append_void(b, InstKind::Store(this, Operand::Value(two), 0));
        f.append_void(b, InstKind::Store(cell, Operand::Value(two), 0));
        let v = InstKind::Load(Type::Obj(StructId(0)), Operand::Value(two), 0);
        reads.push(Operand::Value(f.append_inst(
            b,
            v,
            Type::Obj(StructId(0)),
            None,
        )));
    });
    let roots = Roots::of(&f, Some(ALLOC));
    assert!(reads.iter().all(|r| roots.root(r) == Root::Other));
}

#[test]
fn a_runtime_call_off_the_list_changes_arrays_and_a_test_does_not() {
    let m = module();
    let sorts = method("m", |f, b, this| {
        f.append_void(b, InstKind::Call(CHECK, vec![this]));
    });
    assert_eq!(site(&m, &sorts), None);
    let sorts = method("m", |f, b, this| {
        f.append_void(b, InstKind::Call(SORT, vec![this]));
    });
    assert_eq!(
        site(&m, &sorts).as_deref(),
        Some("a runtime call to __torajs_arr_sort")
    );
}

#[test]
fn a_store_into_an_array_header_changes_arrays() {
    let m = module();
    let f = method("m", |f, b, this| {
        let toks = InstKind::Load(Type::Arr(ArrId(0)), this, 32);
        let toks = Operand::Value(f.append_inst(b, toks, Type::Arr(ArrId(0)), None));
        f.append_void(b, InstKind::Store(Operand::ConstI64(0), toks, 8));
    });
    assert_eq!(site(&m, &f).as_deref(), Some("a header store"));
}

#[test]
fn a_call_passing_another_receiver_puts_both_ends_out() {
    let mut m = module();
    // 3 calls 4 with its own receiver; 4 calls 5 with a fresh cell
    m.funcs.push(method("__cm_P__a", |f, b, this| {
        let r = parked(f, b, this);
        f.append_void(b, InstKind::Call(FuncId(4), vec![r]));
    }));
    m.funcs.push(method("__cm_P__b", |f, b, _| {
        let cell = InstKind::Call(ALLOC, vec![Operand::ConstI64(40)]);
        let cell = Operand::Value(f.append_inst(b, cell, Type::Obj(StructId(0)), None));
        f.append_void(b, InstKind::Call(FuncId(5), vec![cell]));
    }));
    m.funcs.push(method("__cm_P__c", |_, _, _| {}));
    m.funcs.push(method("__cm_P__d", |f, b, this| {
        f.append_void(b, InstKind::Call(FuncId(3), vec![this]));
    }));
    let watch = Watch::of(&m, &HashSet::from([0])).expect("a class holds an array");
    let skip = vec![false; m.funcs.len()];
    let clusters = Clusters::new(&m, &watch, &skip, Some(ALLOC));
    let (groups, out) = clusters.group(StructId(0));
    assert_eq!(out, [FuncId(4), FuncId(5)]);
    assert_eq!(groups, [vec![FuncId(3), FuncId(6)]]);
}

#[test]
fn the_species_guard_counts_as_a_change() {
    // it can run a user `constructor` getter, which can push anywhere
    let mut m = module();
    let mut g = Function::new("__torajs_arr_species_guard", Type::Bool);
    g.add_param(Type::Ptr, "a");
    m.funcs.push(g);
    let f = method("m", |f, b, this| {
        f.append_void(b, InstKind::Call(FuncId(3), vec![this]));
    });
    assert_eq!(
        site(&m, &f).as_deref(),
        Some("a runtime call to __torajs_arr_species_guard")
    );
}

/// `this.toks` parked in a slot and read back as `ty`.
fn park_toks(ty: Type) -> Function {
    method("m", |f, b, this| {
        let toks = InstKind::Load(Type::Arr(ArrId(0)), this, 32);
        let toks = Operand::Value(f.append_inst(b, toks, Type::Arr(ArrId(0)), None));
        let slot = f.append_inst(b, InstKind::Alloca(ty), Type::Ptr, None);
        f.append_void(b, InstKind::Store(toks, Operand::Value(slot), 0));
        f.append_inst(b, InstKind::Load(ty, Operand::Value(slot), 0), ty, None);
    })
}

#[test]
fn an_array_parked_and_read_back_as_bits_leaves_the_lanes() {
    let m = module();
    assert_eq!(site(&m, &park_toks(Type::Arr(ArrId(0)))), None);
    assert_eq!(
        site(&m, &park_toks(Type::I64)).as_deref(),
        Some("an array parked untyped")
    );
}

#[test]
fn an_array_returned_as_something_else_leaves_the_lanes() {
    let m = module();
    let ret = |ty: Type| {
        let mut f = Function::new("m", ty);
        let this = f.add_param(Type::Obj(StructId(0)), "this");
        let b = f.add_block();
        let toks = InstKind::Load(Type::Arr(ArrId(0)), Operand::Value(this), 32);
        let toks = f.append_inst(b, toks, Type::Arr(ArrId(0)), None);
        f.set_term(b, Terminator::Ret(Some(Operand::Value(toks))));
        f
    };
    assert_eq!(site(&m, &ret(Type::Arr(ArrId(0)))), None);
    assert_eq!(
        site(&m, &ret(Type::Ptr)).as_deref(),
        Some("an array returned untyped")
    );
}

#[test]
fn a_sid_two_classes_reach_has_no_name() {
    let names = sole_names([("__hc0_P", 3), ("__hc1_P", 3), ("Q", 4)]);
    assert_eq!(names.get(&3), None);
    assert_eq!(names.get(&4).map(String::as_str), Some("Q"));
}
