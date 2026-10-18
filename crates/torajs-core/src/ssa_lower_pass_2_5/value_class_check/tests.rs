//! Hand-built modules in which sid 0 is a value class: each way the
//! SSA can let a cell or an array of it escape is reported, and the
//! shapes lowering emits for a value class today are not.

use std::collections::HashSet;

use super::*;
use crate::ast::PropKey;
use crate::ssa::{ArrId, BlockId, IPred};

const SORT: FuncId = FuncId(0);
const PUSH: FuncId = FuncId(1);
const TAKES_ANY: FuncId = FuncId(2);

/// Layout 0 `{kind, value}` (the value class), layout 1 `{x}`; array
/// layout 0 holds cells of layout 0. Funcs 0 and 1 are runtime entries
/// taking the array through a pointer: one that reads its slots, and
/// the element push, which an inline array replaces; func 2 takes an
/// `any`.
pub(super) fn module() -> Module {
    let mut m = Module {
        struct_layouts: vec![
            vec![
                (PropKey::from("kind"), Type::F64),
                (PropKey::from("value"), Type::F64),
            ],
            vec![(PropKey::from("x"), Type::F64)],
        ],
        arr_layouts: vec![Type::Obj(StructId(0))].into(),
        ..Default::default()
    };
    let mut sort = Function::new("__torajs_arr_sort", Type::Void);
    sort.add_param(Type::Ptr, "arr");
    m.funcs.push(sort);
    let mut push = Function::new("__torajs_arr_push", Type::Void);
    push.add_param(Type::Ptr, "arr");
    push.add_param(Type::I64, "elem");
    m.funcs.push(push);
    let mut any = Function::new("takes_any", Type::Void);
    any.add_param(Type::Any, "v");
    m.funcs.push(any);
    m
}

/// A function `name` returning `ret`, its body built by `build`.
pub(super) fn func(
    m: &mut Module,
    name: &str,
    ret: Type,
    build: impl FnOnce(&mut Function, BlockId),
) {
    let mut f = Function::new(name, ret);
    let entry = f.add_block();
    build(&mut f, entry);
    if f.blocks[entry.0 as usize].term == Terminator::Unreachable {
        f.set_term(entry, Terminator::Ret(None));
    }
    m.funcs.push(f);
}

fn cell(f: &mut Function, b: BlockId, sid: u32) -> Operand {
    Operand::Value(f.append_inst(b, InstKind::AllocaBytes(48), Type::Obj(StructId(sid)), None))
}

fn array(f: &mut Function, b: BlockId) -> Operand {
    Operand::Value(f.append_inst(b, InstKind::AllocaBytes(40), Type::Arr(ArrId(0)), None))
}

pub(super) fn found(m: &Module) -> Vec<String> {
    let classes = HashSet::from([StructId(0)]);
    (disagreements(m, &classes, &[false, false], &|_| false)
        .0
        .into_iter())
    .map(|(_, d)| d)
    .collect()
}

pub(super) fn reports(m: &Module, what: &str) {
    let got = found(m);
    assert!(
        got.len() == 1 && got[0].contains(what),
        "expected one `{what}`: {got:?}"
    );
}

#[test]
fn the_shapes_of_a_value_class_today_agree() {
    let mut m = module();
    func(&mut m, "__new_Token", Type::Obj(StructId(0)), |f, b| {
        let c = cell(f, b, 0);
        f.set_term(b, Terminator::Ret(Some(c)));
    });
    func(&mut m, "main", Type::Void, |f, b| {
        let xs = array(f, b);
        let c = cell(f, b, 0);
        f.append_void(b, InstKind::Call(PUSH, vec![xs, c]));
        let slot = f.append_inst(b, InstKind::Alloca(Type::Obj(StructId(0))), Type::Ptr, None);
        f.append_void(b, InstKind::Store(c, Operand::Value(slot), 0));
        // the release of a cell tests it against null and the undefined
        // sentinel, which lowering types as the cell
        let undef = f.append_inst(
            b,
            InstKind::GlobalRef("___TORAJS_UNDEF_CELL".into()),
            Type::Obj(StructId(0)),
            None,
        );
        for other in [Operand::ConstPtrNull, Operand::Value(undef)] {
            f.append_inst(b, InstKind::ICmp(IPred::Eq, c, other), Type::Bool, None);
        }
    });
    assert_eq!(found(&m), Vec::<String>::new());
}

#[test]
fn boxing_a_cell_is_reported() {
    let mut m = module();
    func(&mut m, "f", Type::Void, |f, b| {
        let c = cell(f, b, 0);
        f.append_inst(b, InstKind::BoxAny(4, c), Type::Any, None);
    });
    reports(&m, "boxed");
}

#[test]
fn passing_a_cell_as_any_is_reported() {
    let mut m = module();
    func(&mut m, "f", Type::Void, |f, b| {
        let c = cell(f, b, 0);
        f.append_void(b, InstKind::Call(TAKES_ANY, vec![c]));
    });
    reports(&m, "passed");
}

#[test]
fn a_runtime_entry_reading_the_slots_is_reported() {
    let mut m = module();
    func(&mut m, "f", Type::Void, |f, b| {
        let xs = array(f, b);
        f.append_void(b, InstKind::Call(SORT, vec![xs]));
    });
    reports(&m, "passed");
}

#[test]
fn comparing_two_cells_is_reported() {
    let mut m = module();
    func(&mut m, "f", Type::Void, |f, b| {
        let a = cell(f, b, 0);
        let c = cell(f, b, 0);
        f.append_inst(b, InstKind::ICmp(IPred::Eq, a, c), Type::Bool, None);
    });
    reports(&m, "compared");
}

#[test]
fn storing_a_cell_into_another_slot_is_reported() {
    let mut m = module();
    func(&mut m, "f", Type::Void, |f, b| {
        let c = cell(f, b, 0);
        let other = cell(f, b, 1);
        f.append_void(b, InstKind::Store(c, other, 0));
    });
    reports(&m, "stored");
}

#[test]
fn returning_a_cell_from_anything_but_its_factory_is_reported() {
    let mut m = module();
    func(&mut m, "f", Type::Obj(StructId(0)), |f, b| {
        let c = cell(f, b, 0);
        f.set_term(b, Terminator::Ret(Some(c)));
    });
    reports(&m, "returned");
}

#[test]
fn an_open_slot_is_reported() {
    let m = module();
    let classes = HashSet::from([StructId(0)]);
    let got = disagreements(&m, &classes, &[true, false], &|_| false).0;
    assert_eq!(got, vec![(StructId(0), "sid 0 / open slot".to_string())]);
}

fn disagreeing() -> (Module, ProgramFacts) {
    let mut m = module();
    func(&mut m, "f", Type::Void, |f, b| {
        let c = cell(f, b, 0);
        f.append_inst(b, InstKind::PtrToInt(c), Type::I64, None);
    });
    let facts = ProgramFacts {
        value_classes: HashSet::from([StructId(0)]),
        ..Default::default()
    };
    (m, facts)
}

#[test]
fn a_disagreement_names_the_class_to_hold_as_cells() {
    let (m, facts) = disagreeing();
    let got = report(&m, &Ast::default(), &facts, &[false, false], false);
    assert_eq!(got, vec![StructId(0)]);
}

#[test]
#[should_panic(expected = "internal error: the value-class census and the SSA disagree")]
fn a_strict_disagreement_fails_the_compile() {
    let (m, facts) = disagreeing();
    report(&m, &Ast::default(), &facts, &[false, false], true);
}
