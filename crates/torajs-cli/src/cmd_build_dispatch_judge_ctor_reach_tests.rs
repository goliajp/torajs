//! `ctor_cell_reachable` over one keyed read whose key is or is not
//! pinned away from `"constructor"`, and whose answer is returned or
//! dropped.

use torajs_core::ssa::{Function, InstKind, Module, Operand, Terminator, Type, ValueId};

use crate::cmd_build_dispatch_judge_ctor_reach::ctor_cell_reachable;

/// `o[key]` through the keyed accessor read, `key` built by `make_key`;
/// the function returns the answer when `ret`.
fn keyed_read(ret: bool, make_key: impl FnOnce(&mut Module, &mut Function) -> ValueId) -> bool {
    read_with("__torajs_any_accessor_get", ret, make_key)
}

/// `o[key]` through the reader `reader`.
fn read_with(
    reader: &str,
    ret: bool,
    make_key: impl FnOnce(&mut Module, &mut Function) -> ValueId,
) -> bool {
    let mut m = Module::default();
    let mut f = Function::new("user", Type::Any);
    let b = f.add_block();
    let o = f.add_param(Type::Any, "o");
    let key = make_key(&mut m, &mut f);
    let read = m.add_function(Function::new(reader, Type::Any));
    let answer = f.append_inst(
        b,
        InstKind::Call(read, vec![Operand::Value(o), Operand::Value(key)]),
        Type::Any,
        None,
    );
    f.set_term(b, Terminator::Ret(ret.then_some(Operand::Value(answer))));
    m.add_function(f);
    ctor_cell_reachable(&m, &vec![true; m.funcs.len()], false)
}

#[test]
fn a_runtime_string_key_may_read_the_constructor() {
    assert!(keyed_read(true, |_, f| f.add_param(Type::Any, "k")));
    // the answer is dropped: no static is ever looked up on it
    assert!(!keyed_read(false, |_, f| f.add_param(Type::Any, "k")));
}

#[test]
fn a_symbol_key_and_its_box_never_name_the_constructor() {
    assert!(!keyed_read(true, |_, f| f.add_param(Type::Symbol, "s")));
    assert!(!keyed_read(true, |_, f| {
        let s = f.add_param(Type::Symbol, "s");
        f.append_inst(
            torajs_core::ssa::BlockId(0),
            InstKind::BoxAny(4, Operand::Value(s)),
            Type::Any,
            None,
        )
    }));
}

#[test]
fn a_runtime_key_descriptor_bag_keeps_the_statics_even_dropped() {
    // the bag holds the constructor under `value`
    let gopd = "__torajs_anyv_get_property_descriptor";
    assert!(read_with(gopd, false, |_, f| f.add_param(Type::Any, "k")));
}

/// `o[k]` through the pair read, its value loaded from the slot and
/// returned when `ret`; the slot also handed to a call when `hand_out`.
fn pair_read(ret: bool, hand_out: bool) -> bool {
    let mut m = Module::default();
    let mut f = Function::new("user", Type::Any);
    let b = f.add_block();
    let o = f.add_param(Type::Any, "o");
    let k = f.add_param(Type::Any, "k");
    let slot = f.add_param(Type::Ptr, "slot");
    let read = m.add_function(Function::new("__torajs_any_member_get_pair", Type::I64));
    let args = vec![Operand::Value(o), Operand::Value(k), Operand::Value(slot)];
    f.append_inst(b, InstKind::Call(read, args), Type::I64, None);
    let load = InstKind::Load(Type::I64, Operand::Value(slot), 0);
    let value = f.append_inst(b, load, Type::I64, None);
    if hand_out {
        let user = m.add_function(Function::new("user_fn", Type::Any));
        let call = InstKind::Call(user, vec![Operand::Value(slot)]);
        f.append_inst(b, call, Type::Any, None);
    }
    f.set_term(b, Terminator::Ret(ret.then_some(Operand::Value(value))));
    m.add_function(f);
    ctor_cell_reachable(&m, &vec![true; m.funcs.len()], false)
}

#[test]
fn a_runtime_key_pair_read_answers_through_its_slot() {
    // the value comes back through the slot: its load is the answer
    assert!(pair_read(true, false));
    assert!(!pair_read(false, false));
    // a slot read some other way keeps the statics
    assert!(pair_read(false, true));
}
