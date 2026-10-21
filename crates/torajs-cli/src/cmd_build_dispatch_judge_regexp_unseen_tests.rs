//! `unseen_protocol_call` over hand-built functions: each makes one
//! keyed read or call, and the scan says whether a protocol method id
//! could hide behind it.

use torajs_core::ssa::{
    BlockId, FuncId, Function, InstKind, Module, Operand, SigId, Type, ValueId,
};

use crate::cmd_build_dispatch_judge_regexp_unseen::unseen_protocol_call;

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

    /// `recv[key]()` with no arguments.
    fn index_call(&mut self, recv: ValueId, key: ValueId) {
        let args = vec![
            Operand::Value(recv),
            Operand::Value(key),
            Operand::ConstPtrNull,
            Operand::ConstPtrNull,
            Operand::ConstI64(0),
        ];
        self.call("__torajs_any_index_method_call", args, Type::Any);
    }

    /// A well-known symbol by its `__torajs_symbol_well_known` index,
    /// boxed the way a key is.
    fn well_known_key(&mut self, idx: i64) -> ValueId {
        let sym = self.call(
            "__torajs_symbol_well_known",
            vec![Operand::ConstI64(idx)],
            Type::Symbol,
        );
        let pair = vec![Operand::ConstI64(4), Operand::Value(sym)];
        self.call("__torajs_anyv_box_from_pair", pair, Type::Any)
    }

    fn unseen(mut self) -> bool {
        self.m.add_function(self.f);
        unseen_protocol_call(&self.m, &vec![true; self.m.funcs.len()], false)
    }
}

#[test]
fn a_number_or_a_non_protocol_symbol_key_is_pinned() {
    let mut x = Fixture::new();
    let recv = x.f.add_param(Type::Any, "o");
    let zero = x.f.append_inst(
        x.b,
        InstKind::BoxAny(2, Operand::ConstI64(0)),
        Type::Any,
        None,
    );
    x.index_call(recv, zero);
    // Symbol.iterator
    let it = x.well_known_key(5);
    x.index_call(recv, it);
    assert!(!x.unseen());
}

#[test]
fn a_protocol_symbol_is_seen_wherever_it_is_read() {
    let mut x = Fixture::new();
    // Symbol.matchAll
    x.well_known_key(7);
    assert!(x.unseen());
}

#[test]
fn a_runtime_key_on_an_any_receiver_hides_the_id() {
    let mut x = Fixture::new();
    let recv = x.f.add_param(Type::Any, "o");
    let key = x.f.add_param(Type::Any, "k");
    x.index_call(recv, key);
    assert!(x.unseen());
}

#[test]
fn a_runtime_key_on_a_function_receiver_reads_no_protocol_method() {
    let mut x = Fixture::new();
    let fv = x.f.add_param(Type::Closure(SigId(0)), "f");
    let recv = x.f.append_inst(
        x.b,
        InstKind::BoxAny(4, Operand::Value(fv)),
        Type::Any,
        None,
    );
    let key = x.f.add_param(Type::Any, "k");
    x.call(
        "__torajs_any_index_get_keyed",
        vec![Operand::Value(recv), Operand::Value(key)],
        Type::Any,
    );
    assert!(!x.unseen());
}

#[test]
fn a_protocol_name_is_seen_unless_it_names_an_observed_call() {
    let mut x = Fixture::new();
    let recv = x.f.add_param(Type::Any, "s");
    let name = x.lit("split");
    let args = vec![
        Operand::Value(recv),
        Operand::ConstI64(torajs_rc::ANY_METHOD_SPLIT),
        Operand::Value(name),
    ];
    x.call("__torajs_any_method_call", args, Type::Any);
    assert!(!x.unseen());

    let mut x = Fixture::new();
    let recv = x.f.add_param(Type::Any, "s");
    let name = x.lit("split");
    x.call(
        "__torajs_any_member_get_value",
        vec![Operand::Value(recv), Operand::Value(name)],
        Type::I64,
    );
    assert!(x.unseen());
}

#[test]
fn a_proxy_walks_keys_of_its_own() {
    let mut x = Fixture::new();
    x.call("__torajs_proxy_new", Vec::new(), Type::Any);
    assert!(x.unseen());
}
