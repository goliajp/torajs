//! `ctor_cell_reachable` over one keyed read whose key is or is not
//! pinned away from `"constructor"`, and whose answer is returned or
//! dropped.

use torajs_core::ssa::{Function, InstKind, Module, Operand, Terminator, Type, ValueId};

use crate::cmd_build_dispatch_judge_ctor_reach::ctor_cell_reachable;

/// `o[key]` through the keyed accessor read, `key` built by `make_key`;
/// the function returns the answer when `ret`.
fn keyed_read(ret: bool, make_key: impl FnOnce(&mut Module, &mut Function) -> ValueId) -> bool {
    let mut m = Module::default();
    let mut f = Function::new("user", Type::Any);
    let b = f.add_block();
    let o = f.add_param(Type::Any, "o");
    let key = make_key(&mut m, &mut f);
    let read = m.add_function(Function::new("__torajs_any_accessor_get", Type::Any));
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
