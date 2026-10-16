//! `ctor_cell_reachable` over one keyed read whose key is or is not
//! pinned away from `"constructor"`.

use torajs_core::ssa::{Function, InstKind, Module, Operand, Type, ValueId};

use crate::cmd_build_dispatch_judge_ctor_reach::ctor_cell_reachable;

/// `o[key]` through the keyed accessor read, `key` built by `make_key`.
fn keyed_read(make_key: impl FnOnce(&mut Module, &mut Function) -> ValueId) -> bool {
    let mut m = Module::default();
    let mut f = Function::new("user", Type::Any);
    let b = f.add_block();
    let o = f.add_param(Type::Any, "o");
    let key = make_key(&mut m, &mut f);
    let read = m.add_function(Function::new("__torajs_any_accessor_get", Type::Any));
    f.append_inst(
        b,
        InstKind::Call(read, vec![Operand::Value(o), Operand::Value(key)]),
        Type::Any,
        None,
    );
    m.add_function(f);
    ctor_cell_reachable(&m, false)
}

#[test]
fn a_runtime_string_key_may_read_the_constructor() {
    assert!(keyed_read(|_, f| f.add_param(Type::Any, "k")));
}

#[test]
fn a_symbol_key_and_its_box_never_name_the_constructor() {
    assert!(!keyed_read(|_, f| f.add_param(Type::Symbol, "s")));
    assert!(!keyed_read(|_, f| {
        let s = f.add_param(Type::Symbol, "s");
        f.append_inst(
            torajs_core::ssa::BlockId(0),
            InstKind::BoxAny(4, Operand::Value(s)),
            Type::Any,
            None,
        )
    }));
}
