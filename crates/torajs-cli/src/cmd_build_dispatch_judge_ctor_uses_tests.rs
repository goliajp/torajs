//! `ctor_read_escape` over hand-built functions: each reads
//! `o.constructor` through the member-get pair and then does one
//! thing with the answer. `runtime_key_escape` the same way over a
//! dictionary read keyed at run time.

use std::collections::HashSet;

use torajs_core::ssa::{
    BinOp, BlockId, FuncId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
};

use crate::cmd_build_dispatch_judge_ctor_uses::{
    ctor_key_ids, ctor_read_escape, runtime_key_escape,
};

struct Fixture {
    m: Module,
    f: Function,
    b: BlockId,
}

impl Fixture {
    fn new() -> Self {
        let mut f = Function::new("user", Type::Any);
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

    /// `o.constructor` as the any lowering spells it: tag + value,
    /// boxed and retained.
    fn ctor_read(&mut self, o: ValueId) -> ValueId {
        let key = self.lit("constructor");
        let args = vec![Operand::Value(o), Operand::Value(key)];
        let tag = self.call("__torajs_any_member_get_tag", args.clone(), Type::I64);
        let val = self.call("__torajs_any_member_get_value", args, Type::I64);
        let pair = vec![Operand::Value(tag), Operand::Value(val)];
        let boxed = self.call("__torajs_anyv_box_from_pair", pair, Type::Any);
        self.call(
            "__torajs_anyv_retain",
            vec![Operand::Value(boxed)],
            Type::Any,
        )
    }

    fn escape(mut self, ret: Option<ValueId>) -> Option<String> {
        self.f
            .set_term(self.b, Terminator::Ret(ret.map(Operand::Value)));
        let ids = ctor_key_ids(&self.m);
        ctor_read_escape(&self.m, &self.f, &ids)
    }
}

fn start() -> (Fixture, ValueId, ValueId) {
    let mut x = Fixture::new();
    let o = x.f.add_param(Type::Any, "o");
    let c = x.ctor_read(o);
    (x, o, c)
}

#[test]
fn name_read_and_compare_stay_benign() {
    let (mut x, o, c) = start();
    x.call("__torajs_any_name_get", vec![Operand::Value(c)], Type::Any);
    x.call(
        "__torajs_anyv_strict_eq",
        vec![Operand::Value(c), Operand::Value(o)],
        Type::Bool,
    );
    let name = x.lit("name");
    x.call(
        "__torajs_any_member_get_tag",
        vec![Operand::Value(c), Operand::Value(name)],
        Type::I64,
    );
    x.f.append_void(x.b, InstKind::RcDecAny(Operand::Value(c)));
    assert_eq!(x.escape(None), None);
}

#[test]
fn a_static_key_read_off_the_answer_escapes() {
    let (mut x, _, c) = start();
    let keys = x.lit("keys");
    x.call(
        "__torajs_any_member_get_value",
        vec![Operand::Value(c), Operand::Value(keys)],
        Type::I64,
    );
    assert!(x.escape(None).is_some());
}

#[test]
fn an_argument_or_a_return_escapes() {
    let (mut x, _, c) = start();
    x.call("user_fn", vec![Operand::Value(c)], Type::Any);
    assert!(x.escape(None).is_some());
    let (x, _, c) = start();
    assert!(x.escape(Some(c)).is_some());
}

#[test]
fn a_phi_copy_is_followed_to_its_uses() {
    let (mut x, o, c) = start();
    let merged = x.f.append_inst(
        x.b,
        InstKind::Copy(Type::Any, Operand::Value(c)),
        Type::Any,
        None,
    );
    // the other predecessor's def of the same φ value
    x.f.blocks[0].insts.push(Inst {
        result: Some(merged),
        kind: InstKind::Copy(Type::Any, Operand::Value(o)),
        origin: None,
    });
    x.call(
        "__torajs_any_method_call",
        vec![Operand::Value(merged), Operand::ConstI64(68)],
        Type::Any,
    );
    assert!(x.escape(None).is_some());
}

#[test]
fn header_reads_off_the_cell_pointer_stay_benign() {
    let (mut x, _, c) = start();
    let raw = x.call("__torajs_anyv_cell_ptr", vec![Operand::Value(c)], Type::I64);
    let p = x.f.append_inst(
        x.b,
        InstKind::IntToPtr(Operand::Value(raw)),
        Type::Ptr,
        None,
    );
    x.f.append_inst(
        x.b,
        InstKind::Load(Type::I32, Operand::Value(p), 4),
        Type::I32,
        None,
    );
    x.call(
        "__torajs_error_name_get",
        vec![Operand::Value(p)],
        Type::Str,
    );
    assert_eq!(x.escape(None), None);
    // a pointer-typed load stays derived from the cell
    let (mut x, _, c) = start();
    let raw = x.call("__torajs_anyv_cell_ptr", vec![Operand::Value(c)], Type::Ptr);
    let field = x.f.append_inst(
        x.b,
        InstKind::Load(Type::Any, Operand::Value(raw), 24),
        Type::Any,
        None,
    );
    x.call("user_fn", vec![Operand::Value(field)], Type::Any);
    assert!(x.escape(None).is_some());
}

#[test]
fn the_key_outside_a_read_or_a_write_escapes() {
    let mut x = Fixture::new();
    let o = x.f.add_param(Type::Any, "o");
    let key = x.lit("constructor");
    let args = vec![Operand::Value(o), Operand::Value(key)];
    x.call("__torajs_reflect_get", args, Type::Any);
    assert!(x.escape(None).is_some());
    let mut x = Fixture::new();
    let o = x.f.add_param(Type::Any, "o");
    let key = x.lit("constructor");
    let write = vec![
        Operand::Value(o),
        Operand::Value(key),
        Operand::ConstI64(2),
        Operand::ConstI64(1),
        Operand::ConstI64(0),
    ];
    x.call("__torajs_any_member_set", write, Type::Void);
    assert_eq!(x.escape(None), None);
}

#[test]
fn a_descriptor_bag_read_escapes() {
    // the bag's `value` is the constructor, so `.value` off it is not
    // a benign fixed-key read
    let mut x = Fixture::new();
    let o = x.f.add_param(Type::Any, "o");
    let key = x.lit("constructor");
    let args = vec![Operand::Value(o), Operand::Value(key)];
    let bag = x.call("__torajs_anyv_get_property_descriptor", args, Type::Any);
    let value = x.lit("value");
    x.call(
        "__torajs_any_member_get_value",
        vec![Operand::Value(bag), Operand::Value(value)],
        Type::I64,
    );
    assert!(x.escape(None).is_some());
}

/// `d[k]` with `k` a parameter: the dictionary read the typed lane
/// emits, keyed by a string computed at run time.
fn runtime_read() -> (Fixture, ValueId) {
    let mut x = Fixture::new();
    let d = x.f.add_param(Type::Any, "d");
    let k = x.f.add_param(Type::Str, "k");
    let args = vec![Operand::Value(d), Operand::Value(k)];
    let v = x.call("__torajs_dynobj_dict_get", args, Type::Any);
    (x, v)
}

fn runtime_escape(mut x: Fixture, v: ValueId, ret: ValueId) -> Option<String> {
    x.f.set_term(x.b, Terminator::Ret(Some(Operand::Value(ret))));
    runtime_key_escape(&x.m, &x.f, HashSet::from([v]))
}

fn bin(x: &mut Fixture, op: BinOp, l: ValueId, r: Operand) -> ValueId {
    let kind = InstKind::BinOp(op, Operand::Value(l), r);
    x.f.append_inst(x.b, kind, Type::I64, None)
}

#[test]
fn a_runtime_key_answer_added_or_stringified_stays_benign() {
    let (mut x, v) = runtime_read();
    let tag = x.call(
        "__torajs_anyv_unbox_tag",
        vec![Operand::Value(v)],
        Type::I64,
    );
    let val = x.call(
        "__torajs_anyv_unbox_value",
        vec![Operand::Value(v)],
        Type::I64,
    );
    let pair = vec![Operand::Value(tag), Operand::Value(val)];
    let mut add = pair.clone();
    add.extend([Operand::ConstI64(2), Operand::ConstI64(1)]);
    let sum = x.call("__torajs_anyv_add_pair", add, Type::Any);
    let settle = vec![Operand::Value(v), Operand::Value(val)];
    x.call("__torajs_anyv_unbox_settle", settle, Type::Void);
    let s = x.call("__torajs_anyv_to_str_pair_prim", pair, Type::Str);
    x.call("user_fn", vec![Operand::Value(s)], Type::Any);
    assert_eq!(runtime_escape(x, v, sum), None);
}

#[test]
fn a_runtime_key_answer_decoded_as_a_number_stays_benign() {
    // the int32 payload: the low 32 bits, sign-extended
    let (mut x, v) = runtime_read();
    let hi = bin(&mut x, BinOp::Shl, v, Operand::ConstI64(32));
    let lo = bin(&mut x, BinOp::AShr, hi, Operand::ConstI64(32));
    let n = bin(&mut x, BinOp::Add, lo, Operand::ConstI64(1));
    assert_eq!(runtime_escape(x, v, n), None);
    let (mut x, v) = runtime_read();
    let n = bin(&mut x, BinOp::FAdd, v, Operand::Value(v));
    assert_eq!(runtime_escape(x, v, n), None);
}

#[test]
fn a_runtime_key_answer_kept_as_a_cell_escapes() {
    // masking keeps the payload, which is the cell address
    let (mut x, v) = runtime_read();
    let p = bin(&mut x, BinOp::And, v, Operand::ConstI64(0xffff_ffff_ffff));
    assert!(runtime_escape(x, v, p).is_some());
    let (mut x, v) = runtime_read();
    let p = bin(&mut x, BinOp::Shl, v, Operand::ConstI64(16));
    assert!(runtime_escape(x, v, p).is_some());
    let (mut x, v) = runtime_read();
    x.call("user_fn", vec![Operand::Value(v)], Type::Any);
    let n = bin(&mut x, BinOp::Add, v, Operand::ConstI64(0));
    assert!(runtime_escape(x, v, n).is_some());
}
