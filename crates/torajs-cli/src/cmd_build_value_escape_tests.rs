//! The escape walk over hand-built functions: one RegExp value per
//! case, and the walk says whether it can leave the typed lanes.

use torajs_core::ssa::{
    BinOp, BlockId, FuncId, Function, IPred, InstKind, Module, Operand, Terminator, Type, ValueId,
};

use crate::cmd_build_value_escape::escapes;

struct Fixture {
    m: Module,
    f: Function,
    b: BlockId,
}

impl Fixture {
    fn new(ret: Type) -> Self {
        let mut f = Function::new("user", ret);
        let b = f.add_block();
        Self {
            m: Module::default(),
            f,
            b,
        }
    }

    fn ext(&mut self, name: &str) -> FuncId {
        if let Some(i) = self.m.funcs.iter().position(|f| f.name == name) {
            return FuncId(i as u32);
        }
        self.m.add_function(Function::new(name, Type::Any))
    }

    fn inst(&mut self, kind: InstKind, ty: Type) -> ValueId {
        self.f.append_inst(self.b, kind, ty, None)
    }

    fn call(&mut self, name: &str, args: Vec<Operand>, ty: Type) -> ValueId {
        let fid = self.ext(name);
        self.inst(InstKind::Call(fid, args), ty)
    }

    /// A RegExp minted by the typed compile kernel.
    fn regexp(&mut self) -> Operand {
        Operand::Value(self.call("__torajs_regex_compile", vec![], Type::RegExp))
    }

    /// A user fn taking one parameter of `ty`.
    fn user_fn(&mut self, ty: Type) -> FuncId {
        let mut g = Function::new("callee", Type::Void);
        g.add_param(ty, "p");
        let b = g.add_block();
        g.set_term(b, Terminator::Ret(None));
        self.m.add_function(g)
    }

    /// Whether the walk reports the RegExp escaping.
    fn escapes(mut self) -> bool {
        self.f.set_term(self.b, Terminator::Ret(None));
        self.m.add_function(self.f);
        !escapes(
            &self.m,
            |t| (t == Type::RegExp).then_some(()),
            |name| name.starts_with("__torajs_regex"),
        )
        .is_empty()
    }
}

#[test]
fn a_regexp_kept_in_a_typed_slot_and_handed_to_its_kernels_stays() {
    let mut fx = Fixture::new(Type::Void);
    let re = fx.regexp();
    let slot = fx.inst(InstKind::Alloca(Type::RegExp), Type::Ptr);
    fx.inst(InstKind::Store(re, Operand::Value(slot), 0), Type::Void);
    let back = fx.inst(
        InstKind::Load(Type::RegExp, Operand::Value(slot), 0),
        Type::RegExp,
    );
    fx.call(
        "__torajs_regex_test",
        vec![Operand::Value(back)],
        Type::Bool,
    );
    let callee = fx.user_fn(Type::RegExp);
    fx.inst(InstKind::Call(callee, vec![re]), Type::Void);
    assert!(!fx.escapes());
}

#[test]
fn a_box_lets_it_out() {
    let mut fx = Fixture::new(Type::Void);
    let re = fx.regexp();
    fx.inst(InstKind::BoxAny(4, re), Type::Any);
    assert!(fx.escapes());
}

#[test]
fn a_store_through_a_heap_pointer_lets_it_out() {
    let mut fx = Fixture::new(Type::Void);
    let re = fx.regexp();
    let cell = fx.call("__torajs_obj_alloc", vec![], Type::Ptr);
    fx.inst(InstKind::Store(re, Operand::Value(cell), 24), Type::Void);
    assert!(fx.escapes());
}

#[test]
fn a_slot_read_back_as_another_type_lets_it_out() {
    let mut fx = Fixture::new(Type::Void);
    let re = fx.regexp();
    let slot = fx.inst(InstKind::Alloca(Type::RegExp), Type::Ptr);
    fx.inst(InstKind::Store(re, Operand::Value(slot), 0), Type::Void);
    fx.inst(
        InstKind::Load(Type::I64, Operand::Value(slot), 0),
        Type::I64,
    );
    assert!(fx.escapes());
}

#[test]
fn an_untyped_parameter_or_a_foreign_kernel_lets_it_out() {
    let mut fx = Fixture::new(Type::Void);
    let re = fx.regexp();
    let callee = fx.user_fn(Type::Any);
    fx.inst(InstKind::Call(callee, vec![re]), Type::Void);
    assert!(fx.escapes());

    let mut fx = Fixture::new(Type::Void);
    let re = fx.regexp();
    fx.call(
        "__torajs_arr_push",
        vec![Operand::ConstI64(0), re],
        Type::Void,
    );
    assert!(fx.escapes());
}

#[test]
fn an_integer_view_may_only_be_compared() {
    let mut fx = Fixture::new(Type::Void);
    let re = fx.regexp();
    let word = fx.inst(InstKind::PtrToInt(re), Type::I64);
    fx.inst(
        InstKind::ICmp(IPred::Ne, Operand::Value(word), Operand::ConstI64(0)),
        Type::Bool,
    );
    assert!(!fx.escapes());

    let mut fx = Fixture::new(Type::Void);
    let re = fx.regexp();
    let word = fx.inst(InstKind::PtrToInt(re), Type::I64);
    fx.inst(
        InstKind::BinOp(BinOp::Add, Operand::Value(word), Operand::ConstI64(8)),
        Type::I64,
    );
    assert!(fx.escapes());
}

#[test]
fn a_return_keeps_it_only_under_its_own_type() {
    let mut fx = Fixture::new(Type::RegExp);
    let re = fx.regexp();
    fx.f.set_term(fx.b, Terminator::Ret(Some(re)));
    fx.m.add_function(fx.f);
    let typed = escapes(&fx.m, |t| (t == Type::RegExp).then_some(()), |_| true);
    assert!(typed.is_empty());

    let mut fx = Fixture::new(Type::Ptr);
    let re = fx.regexp();
    fx.f.set_term(fx.b, Terminator::Ret(Some(re)));
    fx.m.add_function(fx.f);
    let untyped = escapes(&fx.m, |t| (t == Type::RegExp).then_some(()), |_| true);
    assert_eq!(untyped.len(), 1);
}
