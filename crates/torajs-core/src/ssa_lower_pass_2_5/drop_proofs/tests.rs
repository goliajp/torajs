//! Hand-built modules: which uses of a cell expose its layout, and
//! which functions run only through a class method row.

use super::asleep::row_only;
use super::exposed::exposed_sids;
use crate::ast::PropKey;
use crate::ssa::{
    BlockId, ClassLayoutMeta, FuncId, Function, IPred, InstKind, MethodMetaSpec, Module, Operand,
    StructId, Terminator, Type,
};

/// Layout 0 `{left: 0}` (a tree), layout 1 `{held: 0}` (a container of
/// one). Func 0 releases a cell, func 1 takes a pointer and reads it.
fn module() -> Module {
    let mut m = Module {
        struct_layouts: vec![
            vec![(PropKey::from("left"), Type::Obj(StructId(0)))],
            vec![(PropKey::from("held"), Type::Obj(StructId(0)))],
        ],
        ..Default::default()
    };
    for name in ["__torajs_obj_drop_sized", "__torajs_reads_ptr"] {
        let mut f = Function::new(name, Type::Void);
        f.add_param(Type::Ptr, "p");
        m.funcs.push(f);
    }
    m
}

const DROP: FuncId = FuncId(0);
const READS: FuncId = FuncId(1);

fn func(m: &mut Module, name: &str, ret: Type, build: impl FnOnce(&mut Function, BlockId)) {
    let mut f = Function::new(name, ret);
    let entry = f.add_block();
    build(&mut f, entry);
    if f.blocks[entry.0 as usize].term == Terminator::Unreachable {
        f.set_term(entry, Terminator::Ret(None));
    }
    m.funcs.push(f);
}

fn cell(f: &mut Function, b: BlockId, sid: u32) -> Operand {
    Operand::Value(f.append_inst(b, InstKind::AllocaBytes(32), Type::Obj(StructId(sid)), None))
}

fn exposed(m: &Module) -> Vec<bool> {
    exposed_sids(m, &vec![false; m.funcs.len()])
}

#[test]
fn typed_uses_keep_a_cell_to_typed_code() {
    let mut m = module();
    func(&mut m, "main", Type::Void, |f, b| {
        let c = cell(f, b, 0);
        let d = cell(f, b, 1);
        f.append_void(b, InstKind::Store(c, d, 32));
        let l = f.append_inst(
            b,
            InstKind::Load(Type::Obj(StructId(0)), c, 32),
            Type::Obj(StructId(0)),
            None,
        );
        f.append_inst(
            b,
            InstKind::ICmp(IPred::Eq, Operand::Value(l), c),
            Type::Bool,
            None,
        );
        f.append_void(b, InstKind::RcInc(c));
        f.append_void(b, InstKind::Call(DROP, vec![c]));
    });
    assert_eq!(exposed(&m), vec![false, false]);
}

#[test]
fn boxing_or_a_reading_entry_exposes_the_layout() {
    for boxed in [true, false] {
        let mut m = module();
        func(&mut m, "main", Type::Void, |f, b| {
            let c = cell(f, b, 0);
            if boxed {
                f.append_inst(b, InstKind::BoxAny(4, c), Type::Any, None);
            } else {
                f.append_void(b, InstKind::Call(READS, vec![c]));
            }
        });
        assert_eq!(exposed(&m), vec![true, false], "boxed: {boxed}");
    }
}

#[test]
fn a_crossed_container_exposes_what_it_holds() {
    let mut m = module();
    func(&mut m, "main", Type::Void, |f, b| {
        let d = cell(f, b, 1);
        f.append_inst(b, InstKind::BoxAny(4, d), Type::Any, None);
    });
    assert_eq!(exposed(&m), vec![true, true]);
}

#[test]
fn returned_as_another_type_or_passed_to_one_exposes() {
    let mut m = module();
    func(&mut m, "takes_any", Type::Void, |f, _| {
        f.add_param(Type::Any, "v");
    });
    func(&mut m, "as_ptr", Type::Ptr, |f, b| {
        let c = cell(f, b, 0);
        f.set_term(b, Terminator::Ret(Some(c)));
    });
    assert_eq!(exposed(&m), vec![true, false]);
    let mut m = module();
    func(&mut m, "takes_ptr", Type::Void, |f, _| {
        f.add_param(Type::Ptr, "p");
    });
    func(&mut m, "main", Type::Void, |f, b| {
        let c = cell(f, b, 1);
        f.append_void(b, InstKind::Call(FuncId(2), vec![c]));
    });
    assert_eq!(exposed(&m), vec![true, true]);
}

#[test]
fn a_skipped_function_exposes_nothing() {
    let mut m = module();
    func(&mut m, "main", Type::Void, |f, b| {
        let c = cell(f, b, 0);
        f.append_inst(b, InstKind::BoxAny(4, c), Type::Any, None);
    });
    let mut skip = vec![false; m.funcs.len()];
    skip[2] = true;
    assert_eq!(exposed_sids(&m, &skip), vec![false, false]);
}

/// A class row holding `adapter` (and the twin's adapter `twin`).
fn row(adapter: u32, twin: Option<u32>) -> ClassLayoutMeta {
    ClassLayoutMeta {
        class_name: "C".into(),
        child_offsets: Vec::new(),
        field_metadata: Vec::new(),
        is_named: true,
        is_generic: false,
        methods: vec![MethodMetaSpec {
            name: PropKey::from("m"),
            adapter_fid: FuncId(adapter),
            this_free: false,
            twin_adapter_fid: twin.map(FuncId),
            twin_primary: false,
            declared_here: true,
            touches_private: false,
        }],
    }
}

#[test]
fn what_only_a_method_row_enters_is_asleep() {
    let mut m = module();
    // 2 main, 3 the mono method, 4 its adapter, 5 the twin's adapter,
    // 6 the twin, 7 a helper only the twin calls
    func(&mut m, "main", Type::Void, |f, b| {
        f.append_void(b, InstKind::Call(FuncId(3), vec![]));
    });
    func(&mut m, "__cm_C__m", Type::Void, |_, _| {});
    func(&mut m, "__boxed___cm_C__m", Type::Any, |f, b| {
        f.append_void(b, InstKind::Call(FuncId(3), vec![]));
    });
    func(&mut m, "__boxed___cmany_C__m", Type::Any, |f, b| {
        f.append_void(b, InstKind::Call(FuncId(6), vec![]));
    });
    func(&mut m, "__cmany_C__m", Type::Any, |f, b| {
        f.append_void(b, InstKind::Call(FuncId(7), vec![]));
    });
    func(&mut m, "helper", Type::Void, |_, _| {});
    m.class_layouts.push(row(4, Some(5)));
    let asleep = row_only(&m);
    assert_eq!(
        asleep,
        vec![false, false, false, false, true, true, true, true]
    );
    // an instruction that takes the adapter's address wakes it
    func(&mut m, "reify", Type::Void, |f, b| {
        f.append_inst(b, InstKind::BoxedEntryAddr(FuncId(5)), Type::Ptr, None);
    });
    let asleep = row_only(&m);
    assert_eq!(&asleep[4..8], &[true, false, false, false]);
}
