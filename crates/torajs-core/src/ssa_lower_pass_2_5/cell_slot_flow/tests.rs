use super::*;
use crate::ssa::{BlockId, DataGlobal, FuncId};
use crate::ssa_lower::{CLOSURE_CAP_BASE_OFF, OBJ_HEADER_SIZE};

pub(super) fn layout(names: &[&str]) -> Vec<(PropKey, Type)> {
    names
        .iter()
        .map(|n| (PropKey::from(*n), Type::I64))
        .collect()
}

/// Layouts 0 `{a}`, 1 `{a, b}` (extends 0), 2 `{x, y}`; funcs 0
/// and 1 are the cell and capture-box allocator declarations, 2 a
/// declaration answering `any`, 3 one answering a pointer, 4 and 5
/// the adapter's struct and array layout tests.
pub(super) fn module() -> Module {
    let mut m = Module {
        struct_layouts: vec![layout(&["a"]), layout(&["a", "b"]), layout(&["x", "y"])],
        ..Default::default()
    };
    for name in [OBJ_ALLOC, BOX_ALLOC] {
        let mut alloc = Function::new(name, Type::Ptr);
        alloc.add_param(Type::I64, "n");
        m.funcs.push(alloc);
    }
    m.funcs.push(Function::new("opaque", Type::Any));
    let mut helper = Function::new("helper", Type::Ptr);
    helper.add_param(Type::Ptr, "p");
    helper.add_param(Type::I64, "v");
    m.funcs.push(helper);
    for name in FITS {
        let mut fits = Function::new(name, Type::I64);
        fits.add_param(Type::Any, "v");
        fits.add_param(Type::I64, "tag");
        m.funcs.push(fits);
    }
    m
}

pub(super) fn open(m: &Module) -> Vec<bool> {
    open_sids(m, &EnvLayouts::new()).0
}

pub(super) fn body(m: &mut Module, build: impl FnOnce(&mut Function, BlockId)) -> FuncId {
    let mut f = Function::new("f", Type::Void);
    let entry = f.add_block();
    build(&mut f, entry);
    f.set_term(entry, Terminator::Ret(None));
    let id = FuncId(m.funcs.len() as u32);
    m.funcs.push(f);
    id
}

/// An `any` nothing in the module accounts for: a declaration's
/// answer.
pub(super) fn any(f: &mut Function, entry: BlockId) -> Operand {
    Operand::Value(f.append_inst(entry, InstKind::Call(FuncId(2), vec![]), Type::Any, None))
}

pub(super) fn cell(f: &mut Function, entry: BlockId, sid: u32) -> Operand {
    Operand::Value(f.append_inst(
        entry,
        InstKind::Call(FuncId(0), vec![Operand::ConstI64(40)]),
        Type::Obj(StructId(sid)),
        None,
    ))
}

/// An array cell of `arr` and its data pointer, read as lowering
/// reads it.
pub(super) fn arr_data(f: &mut Function, entry: BlockId, arr: ArrId) -> (ValueId, Operand) {
    let xs = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(arr), None);
    let data = f.append_inst(
        entry,
        InstKind::Load(Type::Ptr, Operand::Value(xs), ARR_DATA_PTR_OFF),
        Type::Ptr,
        None,
    );
    (xs, Operand::Value(data))
}

#[test]
fn an_any_stored_into_a_typed_let_slot_opens_it_and_its_prefixes() {
    let mut m = module();
    body(&mut m, |f, entry| {
        let slot = f.append_inst(
            entry,
            InstKind::Alloca(Type::Obj(StructId(1))),
            Type::Ptr,
            None,
        );
        let v = any(f, entry);
        f.append_void(entry, InstKind::Store(v, Operand::Value(slot), 0));
    });
    // no cell of layout 1 reaches a slot of its prefix 0
    assert_eq!(open(&m), vec![false, true, false]);
    // one read out of such a slot does: the prefix opens with it
    body(&mut m, |f, entry| {
        let wide = f.append_inst(
            entry,
            InstKind::Alloca(Type::Obj(StructId(1))),
            Type::Ptr,
            None,
        );
        let narrow = f.append_inst(
            entry,
            InstKind::Alloca(Type::Obj(StructId(0))),
            Type::Ptr,
            None,
        );
        let v = f.append_inst(
            entry,
            InstKind::Load(Type::Obj(StructId(1)), Operand::Value(wide), 0),
            Type::Obj(StructId(1)),
            None,
        );
        f.append_void(
            entry,
            InstKind::Store(Operand::Value(v), Operand::Value(narrow), 0),
        );
    });
    assert_eq!(open(&m), vec![true, true, false]);
}

/// A cell read through a declared slot of a wider layout carries
/// that layout's openness to the narrower type it is read as.
#[test]
fn a_narrowing_read_of_a_wider_slot_is_a_path() {
    let mut m = module();
    body(&mut m, |f, entry| {
        let slot = f.append_inst(
            entry,
            InstKind::Alloca(Type::Obj(StructId(1))),
            Type::Ptr,
            None,
        );
        let v = any(f, entry);
        f.append_void(entry, InstKind::Store(v, Operand::Value(slot), 0));
        f.append_inst(
            entry,
            InstKind::Load(Type::Obj(StructId(0)), Operand::Value(slot), 0),
            Type::Obj(StructId(0)),
            None,
        );
    });
    assert_eq!(open(&m), vec![true, true, false]);
}

#[test]
fn a_cell_of_the_slots_own_or_wider_layout_keeps_it_closed() {
    let mut m = module();
    body(&mut m, |f, entry| {
        let slot = f.append_inst(
            entry,
            InstKind::Alloca(Type::Obj(StructId(0))),
            Type::Ptr,
            None,
        );
        let wide = cell(f, entry, 1);
        f.append_void(entry, InstKind::Store(wide, Operand::Value(slot), 0));
        f.append_void(
            entry,
            InstKind::Store(Operand::ConstPtrNull, Operand::Value(slot), 0),
        );
    });
    assert_eq!(open(&m), vec![false, false, false]);
}

#[test]
fn a_foreign_layout_in_a_field_a_global_and_an_element_opens_each() {
    let mut m = module();
    m.struct_layouts
        .push(vec![(PropKey::from("child"), Type::Obj(StructId(2)))]);
    m.data_globals.push(DataGlobal {
        name: "g".into(),
        ty: Type::Obj(StructId(1)),
    });
    let arr = m.intern_arr(Type::Obj(StructId(0)));
    body(&mut m, |f, entry| {
        let holder = cell(f, entry, 3);
        let foreign = cell(f, entry, 1);
        f.append_void(entry, InstKind::Store(foreign, holder, OBJ_HEADER_SIZE));
        let g = f.append_inst(entry, InstKind::GlobalRef("g".into()), Type::Ptr, None);
        let v = any(f, entry);
        f.append_void(entry, InstKind::Store(v, Operand::Value(g), 0));
        let (_, data) = arr_data(f, entry, arr);
        let v = any(f, entry);
        f.append_void(entry, InstKind::StoreDyn(v, data, Operand::ConstI64(0)));
    });
    assert_eq!(open(&m), vec![true, true, true, false]);
}

#[test]
fn the_four_element_channels_open_the_element_layout() {
    let arr_of = |m: &mut Module, sid: u32| m.intern_arr(Type::Obj(StructId(sid)));
    // a literal's element store, through the data pointer
    let mut m = module();
    let arr = arr_of(&mut m, 2);
    body(&mut m, |f, entry| {
        let (_, data) = arr_data(f, entry, arr);
        let v = any(f, entry);
        f.append_void(entry, InstKind::Store(v, data, 8));
    });
    assert_eq!(open(&m), vec![false, false, true]);
    // a runtime push of an `any` into a typed array
    let mut m = module();
    let arr = arr_of(&mut m, 2);
    body(&mut m, |f, entry| {
        let (xs, _) = arr_data(f, entry, arr);
        let v = any(f, entry);
        f.append_inst(
            entry,
            InstKind::Call(FuncId(3), vec![Operand::Value(xs), v]),
            Type::Ptr,
            None,
        );
    });
    assert_eq!(open(&m), vec![false, false, true]);
    // a typed array built by a runtime call out of an `any[]`:
    // the argument is the array of `any` elements itself, as the
    // typed declaration's conversion passes it
    let mut m = module();
    let arr = arr_of(&mut m, 2);
    let any_arr = m.intern_arr(Type::Any);
    body(&mut m, |f, entry| {
        let v = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(any_arr), None);
        f.append_inst(
            entry,
            InstKind::Call(FuncId(3), vec![Operand::Value(v), Operand::ConstI64(4)]),
            Type::Arr(arr),
            None,
        );
    });
    assert_eq!(open(&m), vec![false, false, true]);
    // the same call handed a fitting typed array (a slice) keeps
    // the element layout exact
    let mut m = module();
    let arr = arr_of(&mut m, 2);
    body(&mut m, |f, entry| {
        let v = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(arr), None);
        f.append_inst(
            entry,
            InstKind::Call(FuncId(3), vec![Operand::Value(v), Operand::ConstI64(4)]),
            Type::Arr(arr),
            None,
        );
    });
    assert_eq!(open(&m), vec![false, false, false]);
    // an array of another layout stored into an array-typed slot
    let mut m = module();
    let arr = arr_of(&mut m, 2);
    let other = arr_of(&mut m, 0);
    body(&mut m, |f, entry| {
        let slot = f.append_inst(entry, InstKind::Alloca(Type::Arr(arr)), Type::Ptr, None);
        let ys = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(other), None);
        f.append_void(
            entry,
            InstKind::Store(Operand::Value(ys), Operand::Value(slot), 0),
        );
    });
    assert_eq!(open(&m), vec![false, false, true]);
}

#[test]
fn a_runtime_call_over_typed_arrays_and_scalars_keeps_them_closed() {
    let mut m = module();
    let arr = m.intern_arr(Type::Obj(StructId(0)));
    body(&mut m, |f, entry| {
        let (xs, _) = arr_data(f, entry, arr);
        let wide = cell(f, entry, 1);
        f.append_inst(
            entry,
            InstKind::Call(FuncId(3), vec![Operand::Value(xs), wide]),
            Type::Ptr,
            None,
        );
        let n = f.append_inst(
            entry,
            InstKind::Call(FuncId(3), vec![Operand::Value(xs), Operand::ConstI64(3)]),
            Type::I64,
            None,
        );
        f.append_inst(
            entry,
            InstKind::Call(FuncId(3), vec![Operand::Value(xs), Operand::Value(n)]),
            Type::Arr(arr),
            None,
        );
    });
    assert_eq!(open(&m), vec![false, false, false]);
}

#[test]
fn a_typed_parameter_and_a_declared_return_open_on_an_any() {
    let mut m = module();
    let mut callee = Function::new("take", Type::Obj(StructId(2)));
    callee.add_param(Type::Obj(StructId(1)), "b");
    let entry = callee.add_block();
    let v = any(&mut callee, entry);
    callee.set_term(entry, Terminator::Ret(Some(v)));
    let take = FuncId(m.funcs.len() as u32);
    m.funcs.push(callee);
    body(&mut m, |f, entry| {
        let v = any(f, entry);
        f.append_void(entry, InstKind::Call(take, vec![v]));
    });
    assert_eq!(open(&m), vec![false, true, true]);
}

#[test]
fn a_decode_out_of_any_typed_as_a_struct_opens_it() {
    let mut m = module();
    body(&mut m, |f, entry| {
        f.append_inst(
            entry,
            InstKind::IntToPtr(Operand::ConstI64(0)),
            Type::Obj(StructId(2)),
            None,
        );
    });
    assert_eq!(open(&m), vec![false, false, true]);
}

#[test]
fn a_slot_behind_a_pointer_is_typed_by_what_is_loaded_from_it() {
    let mut m = module();
    body(&mut m, |f, entry| {
        let v = any(f, entry);
        let bx = f.append_inst(entry, InstKind::Call(FuncId(1), vec![v]), Type::Ptr, None);
        f.append_inst(
            entry,
            InstKind::Load(Type::Obj(StructId(2)), Operand::Value(bx), 0),
            Type::Obj(StructId(2)),
            None,
        );
        let frame = f.append_inst(entry, InstKind::AllocaBytes(16), Type::Ptr, None);
        f.append_inst(
            entry,
            InstKind::Load(Type::Obj(StructId(0)), Operand::Value(frame), 8),
            Type::Obj(StructId(0)),
            None,
        );
        let own = cell(f, entry, 1);
        f.append_void(entry, InstKind::Store(own, Operand::Value(frame), 8));
    });
    assert_eq!(open(&m), vec![false, false, true]);
    body(&mut m, |f, entry| {
        let frame = f.append_inst(entry, InstKind::AllocaBytes(16), Type::Ptr, None);
        f.append_inst(
            entry,
            InstKind::Load(Type::Obj(StructId(1)), Operand::Value(frame), 0),
            Type::Obj(StructId(1)),
            None,
        );
        let v = any(f, entry);
        f.append_void(entry, InstKind::Store(v, Operand::Value(frame), 0));
    });
    assert_eq!(open(&m), vec![true, true, true]);
}

/// A boxed capture of layout 2 neither function reads as a cell:
/// the box is typed by the closure's capture list on both sides.
#[test]
fn a_capture_box_is_typed_by_the_closures_capture_list() {
    let mut m = module();
    // the closure body: loads the box from its env and writes an
    // `any` into it
    let mut clo = Function::new("__closure_0", Type::Void);
    let p0 = clo.add_param(Type::Ptr, "__env");
    let entry = clo.add_block();
    let slot = clo.append_inst(entry, InstKind::Alloca(Type::Ptr), Type::Ptr, None);
    clo.append_void(
        entry,
        InstKind::Store(Operand::Value(p0), Operand::Value(slot), 0),
    );
    let env = clo.append_inst(
        entry,
        InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
        Type::Ptr,
        None,
    );
    let bx = clo.append_inst(
        entry,
        InstKind::Load(Type::Ptr, Operand::Value(env), CLOSURE_CAP_BASE_OFF),
        Type::Ptr,
        None,
    );
    let v = any(&mut clo, entry);
    clo.append_void(entry, InstKind::Store(v, Operand::Value(bx), 0));
    clo.set_term(entry, Terminator::Ret(None));
    let clo_id = FuncId(m.funcs.len() as u32);
    m.funcs.push(clo);
    let mut envs = EnvLayouts::new();
    envs.insert(clo_id, vec![(Type::Obj(StructId(2)), true)]);
    assert_eq!(open_sids(&m, &envs).0, vec![false, false, true]);
    // the declaring side: boxes an `any` payload and stores the
    // box into the env it builds for the closure
    let mut m = module();
    m.funcs.push(Function::new("__closure_0", Type::Void));
    body(&mut m, |f, entry| {
        let v = any(f, entry);
        let bx = f.append_inst(entry, InstKind::Call(FuncId(1), vec![v]), Type::Ptr, None);
        let env = f.append_inst(
            entry,
            InstKind::Call(FuncId(0), vec![Operand::ConstI64(56)]),
            Type::Ptr,
            None,
        );
        let fa = f.append_inst(entry, InstKind::FnAddr(clo_id), Type::Ptr, None);
        f.append_void(
            entry,
            InstKind::Store(Operand::Value(fa), Operand::Value(env), CLOSURE_FN_ADDR_OFF),
        );
        f.append_void(
            entry,
            InstKind::Store(
                Operand::Value(bx),
                Operand::Value(env),
                CLOSURE_CAP_BASE_OFF,
            ),
        );
    });
    assert_eq!(open_sids(&m, &envs).0, vec![false, false, true]);
    assert_eq!(open(&m), vec![false, false, false]);
}
