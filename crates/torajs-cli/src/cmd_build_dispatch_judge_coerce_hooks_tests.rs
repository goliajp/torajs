//! `hooks_fixed` over one hand-built function that writes a property
//! one way.

use torajs_core::ssa::{
    BinOp, BlockId, DOUBLE_ENCODE_OFFSET, FPred, FuncId, Function, InstKind, Module, Operand,
    TAG_TYPE_NUMBER, Terminator, Type, ValueId,
};

use crate::cmd_build_dispatch_judge_coerce_hooks::hooks_fixed;

struct Fixture {
    m: Module,
    f: Function,
    b: BlockId,
    o: ValueId,
}

impl Fixture {
    fn new() -> Self {
        let mut f = Function::new("user", Type::Any);
        let b = f.add_block();
        let o = f.add_param(Type::Any, "o");
        Self {
            m: Module::default(),
            f,
            b,
            o,
        }
    }

    fn ext(&mut self, name: &str) -> FuncId {
        if let Some(i) = self.m.funcs.iter().position(|f| f.name == name) {
            return FuncId(i as u32);
        }
        self.m.add_function(Function::new(name, Type::Any))
    }

    fn lit(&mut self, s: &str) -> ValueId {
        let id = self.m.intern_string(s.as_bytes().to_vec());
        self.f
            .append_inst(self.b, InstKind::StaticStrRef(id), Type::Str, None)
    }

    fn call(&mut self, name: &str, args: Vec<Operand>, ty: Type) -> ValueId {
        let fid = self.ext(name);
        self.f
            .append_inst(self.b, InstKind::Call(fid, args), ty, None)
    }

    fn inst(&mut self, kind: InstKind, ty: Type) -> ValueId {
        self.f.append_inst(self.b, kind, ty, None)
    }

    /// `o[key] = value` through the dictionary write.
    fn dict_set(&mut self, key: ValueId, value: ValueId) {
        let args = vec![
            Operand::Value(self.o),
            Operand::Value(key),
            Operand::Value(value),
            Operand::ConstBool(true),
        ];
        self.call("__torajs_dynobj_dict_set_entry", args, Type::Void);
    }

    fn fixed(mut self) -> bool {
        if matches!(
            self.f.blocks[self.b.0 as usize].term,
            Terminator::Unreachable
        ) {
            self.f.set_term(self.b, Terminator::Ret(None));
        }
        self.m.add_function(self.f);
        hooks_fixed(&self.m, &vec![true; self.m.funcs.len()], false)
    }
}

/// `o.<key> = v` through the member write.
fn member_set(key: &str) -> bool {
    let mut x = Fixture::new();
    let k = x.lit(key);
    let v = x.f.add_param(Type::Any, "v");
    let args = vec![
        Operand::Value(x.o),
        Operand::Value(k),
        Operand::ConstI64(4),
        Operand::Value(v),
        Operand::ConstI64(0),
    ];
    x.call("__torajs_any_member_set", args, Type::Void);
    x.fixed()
}

#[test]
fn a_write_under_a_hook_key_installs() {
    assert!(member_set("x"));
    assert!(!member_set("valueOf"));
    assert!(!member_set("toString"));
    assert!(!member_set("__proto__"));
}

#[test]
fn a_hook_key_read_installs_nothing() {
    let mut x = Fixture::new();
    let k = x.lit("valueOf");
    let args = vec![Operand::Value(x.o), Operand::Value(k)];
    x.call("__torajs_any_member_get_tag", args, Type::I64);
    assert!(x.fixed());
}

#[test]
fn bulk_definers_and_relinks_install() {
    for name in [
        "__torajs_anyv_assign",
        "__torajs_anyv_set_prototype_of",
        "__torajs_reflect_set_prototype_of",
        "__torajs_dynobj_define_from_desc",
        "__torajs_dynobj_define_properties_from",
    ] {
        let mut x = Fixture::new();
        let v = x.f.add_param(Type::Any, "v");
        x.call(
            name,
            vec![Operand::Value(x.o), Operand::Value(v)],
            Type::Void,
        );
        assert!(!x.fixed(), "{name}");
    }
}

#[test]
fn annex_b_definers_install() {
    let call = |mid: i64| {
        let mut x = Fixture::new();
        let args = vec![Operand::Value(x.o), Operand::ConstI64(mid)];
        x.call("__torajs_any_method_call", args, Type::Any);
        x.fixed()
    };
    assert!(call(torajs_rc::ANY_METHOD_MATCH));
    assert!(!call(torajs_rc::ANY_METHOD_DEFINE_GETTER));
    assert!(!call(torajs_rc::ANY_METHOD_DEFINE_SETTER));
}

#[test]
fn a_runtime_key_write_of_a_primitive_installs_nothing() {
    // `d[k] = d[k] + 1`: the pair add answers a primitive
    let mut x = Fixture::new();
    let k = x.f.add_param(Type::Str, "k");
    let sum = x.call(
        "__torajs_anyv_add_pair",
        vec![Operand::ConstI64(2), Operand::ConstI64(1)],
        Type::Any,
    );
    let merged = x.inst(InstKind::Copy(Type::Any, Operand::Value(sum)), Type::Any);
    x.dict_set(k, merged);
    assert!(x.fixed());
    // the int32 box
    let mut x = Fixture::new();
    let k = x.f.add_param(Type::Str, "k");
    let i = x.f.add_param(Type::I64, "i");
    let tag = Operand::ConstI64(TAG_TYPE_NUMBER as i64);
    let bits = x.inst(
        InstKind::BinOp(BinOp::Or, Operand::Value(i), tag),
        Type::I64,
    );
    let boxed = x.inst(InstKind::IntToPtr(Operand::Value(bits)), Type::Any);
    x.dict_set(k, boxed);
    assert!(x.fixed());
}

#[test]
fn a_runtime_key_write_of_any_value_installs() {
    let mut x = Fixture::new();
    let k = x.f.add_param(Type::Str, "k");
    let v = x.f.add_param(Type::Any, "v");
    x.dict_set(k, v);
    assert!(!x.fixed());
}

/// `d[k] = box_f64(x)` as the box expands: the encode on the false
/// arm of `x != x` when `nan_tested`, in the entry block otherwise.
fn double_box(nan_tested: bool) -> bool {
    let mut x = Fixture::new();
    let k = x.f.add_param(Type::Str, "k");
    let d = x.f.add_param(Type::F64, "x");
    let raw = x.inst(InstKind::BitCastF64ToI64(Operand::Value(d)), Type::I64);
    if nan_tested {
        let nan = x.inst(
            InstKind::FCmp(FPred::Une, Operand::Value(d), Operand::Value(d)),
            Type::Bool,
        );
        let (is_nan, boxing) = (x.f.add_block(), x.f.add_block());
        x.f.set_term(
            x.b,
            Terminator::CondBr {
                cond: Operand::Value(nan),
                then_blk: is_nan,
                else_blk: boxing,
            },
        );
        x.f.set_term(is_nan, Terminator::Ret(None));
        x.b = boxing;
    }
    let offset = Operand::ConstI64(DOUBLE_ENCODE_OFFSET as i64);
    let bits = x.inst(
        InstKind::BinOp(BinOp::Add, Operand::Value(raw), offset),
        Type::I64,
    );
    let boxed = x.inst(InstKind::IntToPtr(Operand::Value(bits)), Type::Any);
    x.dict_set(k, boxed);
    x.fixed()
}

#[test]
fn a_double_box_is_a_primitive_only_past_the_nan_test() {
    assert!(double_box(true));
    // a NaN payload can wrap into the cell range
    assert!(!double_box(false));
}
