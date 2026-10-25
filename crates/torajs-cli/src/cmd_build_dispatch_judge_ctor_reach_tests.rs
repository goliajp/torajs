//! `ctor_cell_reachable` over one keyed read whose key is or is not
//! pinned away from `"constructor"`, and whose answer is returned or
//! dropped; `hooks_fixed` over the reads and writes whose role turns
//! on whether their key is a literal.

use torajs_core::ssa::{BlockId, Function, InstKind, Module, Operand, Terminator, Type, ValueId};

use crate::cmd_build_dispatch_judge_coerce_hooks::{DEFINER_KEYS, HOOK_KEYS, hooks_fixed};
use crate::cmd_build_dispatch_judge_ctor_reach::ctor_cell_reachable;
use crate::cmd_build_dispatch_judge_ctor_uses::{
    SUPER_PROP_CALL, SUPER_PROP_GET, SUPER_PROP_OPT_CALL,
};

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
            BlockId(0),
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
/// returned when `ret`; the slot also handed to a call when
/// `hand_out`, and a parameter rather than a stack slot unless `stack`.
fn pair_read_in(ret: bool, hand_out: bool, stack: bool) -> bool {
    let mut m = Module::default();
    let mut f = Function::new("user", Type::Any);
    let b = f.add_block();
    let o = f.add_param(Type::Any, "o");
    let k = f.add_param(Type::Any, "k");
    let slot = if stack {
        f.append_inst(b, InstKind::Alloca(Type::I64), Type::Ptr, None)
    } else {
        f.add_param(Type::Ptr, "slot")
    };
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

fn pair_read(ret: bool, hand_out: bool) -> bool {
    pair_read_in(ret, hand_out, true)
}

#[test]
fn a_pair_slot_is_followed_only_as_a_stack_slot() {
    // a slot the function did not allocate may be read elsewhere
    assert!(pair_read_in(false, false, false));
}

#[test]
fn a_runtime_key_pair_read_answers_through_its_slot() {
    // the value comes back through the slot: its load is the answer
    assert!(pair_read(true, false));
    assert!(!pair_read(false, false));
    // a slot read some other way keeps the statics
    assert!(pair_read(false, true));
}

/// A boxed string literal, the key a `super.<name>` site passes.
fn boxed_lit(m: &mut Module, f: &mut Function, s: &str) -> ValueId {
    let id = m.intern_string(s.as_bytes().to_vec());
    let lit = f.append_inst(BlockId(0), InstKind::StaticStrRef(id), Type::Str, None);
    let boxer = m.add_function(Function::new("__torajs_anyv_box_str_slot", Type::Any));
    f.append_inst(
        BlockId(0),
        InstKind::Call(boxer, vec![Operand::Value(lit)]),
        Type::Any,
        None,
    )
}

#[test]
fn a_super_read_with_a_runtime_key_may_read_the_constructor() {
    for reader in [SUPER_PROP_GET, SUPER_PROP_CALL, SUPER_PROP_OPT_CALL] {
        assert!(read_with(reader, true, |_, f| f.add_param(Type::Any, "k")));
        assert!(!read_with(reader, false, |_, f| f.add_param(Type::Any, "k")));
        assert!(
            !read_with(reader, true, |m, f| boxed_lit(m, f, "m")),
            "{reader}"
        );
    }
}

/// `hooks_fixed` over one call of `entry(o, …, key)`, `key` at `at`.
fn hooks_with(
    entry: &str,
    at: usize,
    make_key: impl FnOnce(&mut Module, &mut Function) -> ValueId,
) -> bool {
    let mut m = Module::default();
    let mut f = Function::new("user", Type::Any);
    let b = f.add_block();
    let o = f.add_param(Type::Any, "o");
    let key = make_key(&mut m, &mut f);
    let callee = m.add_function(Function::new(entry, Type::Any));
    let mut args = vec![Operand::Value(o); at];
    args.push(Operand::Value(key));
    f.append_inst(b, InstKind::Call(callee, args), Type::Any, None);
    f.set_term(b, Terminator::Ret(None));
    m.add_function(f);
    hooks_fixed(&m, &vec![true; m.funcs.len()], false)
}

#[test]
fn a_super_call_with_a_runtime_key_may_call_a_definer() {
    // `super[k](name, getter)` with `k` = "__defineGetter__"
    for call in [SUPER_PROP_CALL, SUPER_PROP_OPT_CALL] {
        assert!(!hooks_with(call, 1, |_, f| f.add_param(Type::Any, "k")));
        assert!(hooks_with(call, 1, |m, f| boxed_lit(m, f, "m")), "{call}");
        assert!(!hooks_with(call, 1, |m, f| boxed_lit(m, f, "valueOf")));
        assert!(!hooks_with(call, 1, |m, f| boxed_lit(
            m,
            f,
            "__defineGetter__"
        )));
    }
    // the read's answer is followed by the reach scan
    assert!(hooks_with(SUPER_PROP_GET, 1, |_, f| f.add_param(Type::Any, "k")));
}

#[test]
fn a_returned_object_takes_the_mint_elements_by_name() {
    let carry = "__torajs_ctor_ret_carry";
    let lit = |s: &'static str| {
        move |m: &mut Module, f: &mut Function| {
            let id = m.intern_string(s.as_bytes().to_vec());
            f.append_inst(BlockId(0), InstKind::StaticStrRef(id), Type::Str, None)
        }
    };
    assert!(hooks_with(carry, 2, lit("x")));
    assert!(!hooks_with(carry, 2, lit("valueOf")));
    assert!(!hooks_with(carry, 2, |_, f| f.add_param(Type::Str, "k")));
}

#[test]
fn an_any_constant_key_is_fixed() {
    // `super.v`: a key of at most five bytes is boxed in place
    let short = torajs_core::short_str_encode::encode_short_str_literal(b"v").expect("short");
    let constant = |_: &mut Module, f: &mut Function| {
        let c = InstKind::IntToPtr(Operand::ConstI64(short as i64));
        f.append_inst(BlockId(0), c, Type::Any, None)
    };
    assert!(!read_with(SUPER_PROP_GET, true, constant));
    assert!(hooks_with(SUPER_PROP_CALL, 1, constant));
    // no key a constant box can hold names a hook, a definer or
    // the constructor
    let cap = torajs_core::short_str_encode::SHORT_STR_CAP;
    for k in HOOK_KEYS
        .iter()
        .chain(&DEFINER_KEYS)
        .chain(&[&b"constructor"[..]])
    {
        assert!(k.len() > cap, "{k:?}");
    }
}
