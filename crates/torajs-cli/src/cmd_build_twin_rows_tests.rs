//! `twin_rows_kept` over a hand-built module: class P (fields `toks`,
//! `pos`) has a method with a twin, class Q (field `kind`) has none,
//! and `main` makes an instance and uses it one way per case. Only P's
//! answer matters where P keeps its twin: Q has none to keep.

use torajs_core::ast::PropKey;
use torajs_core::ssa::{
    BlockId, ClassLayoutMeta, FieldMetaSpec, FuncId, Function, InstKind, MethodMetaSpec, Module,
    Operand, StructId, Terminator, Type, ValueId,
};

use crate::cmd_build_twin_rows::twin_rows_kept;

const P: StructId = StructId(0);
const Q: StructId = StructId(1);
/// `{ toks }`, a prefix of P's layout.
const TOKS: StructId = StructId(2);

fn row(fields: &[&str], twin: Option<u32>) -> ClassLayoutMeta {
    ClassLayoutMeta {
        class_name: "C".into(),
        child_offsets: Vec::new(),
        field_metadata: (fields.iter().enumerate())
            .map(|(i, n)| FieldMetaSpec {
                name: PropKey::from(*n),
                offset: 32 + 8 * i as u32,
                type_tag: 1,
            })
            .collect(),
        is_named: true,
        is_generic: false,
        methods: vec![MethodMetaSpec {
            name: PropKey::from("m"),
            adapter_fid: FuncId(0),
            this_free: false,
            twin_adapter_fid: twin.map(FuncId),
            twin_primary: false,
            declared_here: true,
            touches_private: false,
        }],
    }
}

struct Fixture {
    m: Module,
    f: Function,
    b: BlockId,
}

impl Fixture {
    fn new() -> Self {
        let mut m = Module::default();
        let field = |n: &str| (PropKey::from(n), Type::I64);
        m.struct_layouts = vec![
            vec![field("toks"), field("pos")],
            vec![field("kind")],
            vec![field("toks")],
        ];
        m.class_layouts = vec![row(&["toks", "pos"], Some(0)), row(&["kind"], None)];
        let mut f = Function::new("main", Type::Void);
        let b = f.add_block();
        Self { m, f, b }
    }

    fn call(&mut self, name: &str, args: Vec<Operand>, ty: Type) -> ValueId {
        let fid = match self.m.funcs.iter().position(|f| f.name == name) {
            Some(i) => FuncId(i as u32),
            None => self.m.add_function(Function::new(name, Type::Any)),
        };
        self.f
            .append_inst(self.b, InstKind::Call(fid, args), ty, None)
    }

    fn instance(&mut self, sid: StructId) -> Operand {
        Operand::Value(self.call("__torajs_obj_alloc", vec![], Type::Obj(sid)))
    }

    fn boxed(&mut self, sid: StructId) {
        let v = self.instance(sid);
        self.f
            .append_inst(self.b, InstKind::BoxAny(4, v), Type::Any, None);
    }

    fn kept(mut self) -> Vec<bool> {
        self.f.set_term(self.b, Terminator::Ret(None));
        self.m.add_function(self.f);
        twin_rows_kept(&self.m)
    }
}

#[test]
fn an_instance_kept_typed_drops_its_twins() {
    let mut fx = Fixture::new();
    let p = fx.instance(P);
    fx.call("__torajs_rc_dec", vec![p], Type::Void);
    assert_eq!(fx.kept(), vec![false, false]);
}

#[test]
fn a_boxed_instance_keeps_its_class_twins() {
    let mut fx = Fixture::new();
    fx.boxed(P);
    assert!(fx.kept()[0]);
}

#[test]
fn only_a_prefix_view_of_the_layout_keeps_them() {
    let mut fx = Fixture::new();
    fx.boxed(Q);
    assert!(!fx.kept()[0]);

    let mut fx = Fixture::new();
    fx.boxed(TOKS);
    assert!(fx.kept()[0]);
}

#[test]
fn a_road_to_a_prototype_or_a_hierarchy_keeps_every_twin() {
    let mut fx = Fixture::new();
    fx.call(
        "__torajs_anyv_class_get",
        vec![Operand::ConstI64(1)],
        Type::Any,
    );
    assert_eq!(fx.kept(), vec![true, true]);

    let mut fx = Fixture::new();
    fx.call("__torajs_proto_link_fresh", vec![], Type::Void);
    fx.boxed(Q);
    assert_eq!(fx.kept(), vec![true, true]);

    let mut fx = Fixture::new();
    fx.call("__torajs_proto_link_fresh", vec![], Type::Void);
    assert_eq!(fx.kept(), vec![false, false]);
}

/// A dropped row points at one loud-reject stub (fam 59, the "class
/// method twin" TypeError) appended once; a table that drops nothing
/// appends nothing.
#[test]
fn a_dropped_twin_column_points_at_the_reject_stub() {
    use crate::cmd_build_twin_rows::replacements;
    let mut funcs = Vec::new();
    assert_eq!(replacements(&[true, true], &mut funcs), vec![None, None]);
    assert!(funcs.is_empty());
    let got = replacements(&[true, false, false], &mut funcs);
    assert_eq!(got, vec![None, Some(0), Some(0)]);
    assert_eq!(funcs.len(), 1);
    assert_eq!(
        funcs[0].bytes,
        crate::cmd_build_dispatch_stubs::reject_stub_body(59).0
    );
}
