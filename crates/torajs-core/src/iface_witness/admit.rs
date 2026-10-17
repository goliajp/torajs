//! The checker's view of the witness table: a class instance fits an
//! interface whose rows it fills, and every class instance the checker
//! lets into an interface — or interface value into another object
//! type — is noted, so that `settle` keeps slots only where one flows.
//!
//! The assignability lattice is a set of pure functions over the type
//! tables, called from every slot comparison in the checker; the table
//! and the notes ride beside them for the duration of a compile instead
//! of being threaded through each of those call sites. The table is
//! written once per checker run, from the AST the run checks; the notes
//! gather over every run of one compile (`begin_compile` clears them).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::check::Type;

/// What the checker let through, over one compile.
#[derive(Debug, Clone, Default)]
pub(super) struct Flows {
    /// (interface, class): an instance of the class was admitted where
    /// a value of the interface is expected
    pub classes: BTreeSet<(String, String)>,
    /// (interface, target): a value of the interface was admitted where
    /// another interface (`Some`) or another object type (`None`) is
    /// expected
    pub edges: BTreeSet<(String, Option<String>)>,
}

thread_local! {
    static TABLE: RefCell<Rc<super::IfaceSlots>> = RefCell::new(Rc::default());
    static FLOWS: RefCell<Flows> = RefCell::new(Flows::default());
}

pub(crate) fn install(ast: &crate::ast::Ast) {
    TABLE.with(|t| *t.borrow_mut() = Rc::new(ast.iface_slots.clone()));
}

pub(crate) fn begin_compile() {
    FLOWS.with(|f| *f.borrow_mut() = Flows::default());
}

pub(super) fn table() -> Rc<super::IfaceSlots> {
    TABLE.with(|t| t.borrow().clone())
}

pub(super) fn flows() -> Flows {
    FLOWS.with(|f| f.borrow().clone())
}

/// The interface `t` names: its type, or its name as a placeholder.
fn iface(table: &super::IfaceSlots, t: &Type) -> Option<String> {
    match t {
        Type::Struct(_) => table.iface_of(t).map(str::to_string),
        Type::ClassRef(n) if table.types.contains_key(n) => Some(n.clone()),
        _ => None,
    }
}

/// `Some(true)` when `to` is an interface whose rows `from`, a class
/// instance, fills (both as the checker wrote them, unresolved); `None`
/// leaves the pairing to the structural rules, which admit a class
/// that lays the interface's fields out first. Arrays, nullables and
/// promises are peeled pairwise first: the structural rules resolve
/// the class away before they recurse.
pub(crate) fn class_admit(to: &Type, from: &Type) -> Option<bool> {
    match (to, from) {
        (Type::Array(t), Type::Array(f))
        | (Type::Promise(t), Type::Promise(f))
        | (Type::Nullable(t), Type::Nullable(f)) => class_admit(t, f),
        (Type::Nullable(t), f) if !matches!(f, Type::Null | Type::Undefined) => class_admit(t, f),
        (Type::Struct(_) | Type::ClassRef(_), Type::ClassRef(c)) => {
            let c = c.split('<').next().unwrap_or(c);
            let table = table();
            let i = iface(&table, to)?;
            if !table.witnesses.get(&i).is_some_and(|cs| cs.contains(c))
                || table.loose.get(&i).is_some_and(|cs| cs.contains(c))
            {
                return None;
            }
            note_flow(to, from, true);
            Some(true)
        }
        _ => None,
    }
}

/// Note what an admitted pairing lets flow: a class instance (or an
/// `any` value) into an interface, or an interface value into another
/// object type.
pub(crate) fn note_flow(to: &Type, from: &Type, fits: bool) {
    let table = table();
    if !fits || table.types.is_empty() {
        return;
    }
    match (to, from) {
        (Type::Array(t), Type::Array(f))
        | (Type::Promise(t), Type::Promise(f))
        | (Type::Nullable(t), Type::Nullable(f)) => return note_flow(t, f, true),
        (Type::Nullable(t), f) if !matches!(f, Type::Null | Type::Undefined) => {
            return note_flow(t, f, true);
        }
        _ => {}
    }
    let to_iface = iface(&table, to);
    // an `any` value may hold an instance of any class that fills the
    // interface's rows: each of them flows, so the cast at the write
    // admits it through the witness slots instead of refusing it
    if matches!(from, Type::Any) {
        let Some(i) = to_iface else { return };
        let Some(ws) = table.witnesses.get(&i) else {
            return;
        };
        let loose = table.loose.get(&i);
        FLOWS.with(|f| {
            let mut f = f.borrow_mut();
            for c in ws.iter().filter(|c| !loose.is_some_and(|l| l.contains(*c))) {
                f.classes.insert((i.clone(), c.clone()));
            }
        });
        return;
    }
    let flow = match from {
        Type::ClassRef(c) if !table.types.contains_key(c) => {
            let Some(i) = to_iface else { return };
            let c = c.split('<').next().unwrap_or(c).to_string();
            FLOWS.with(|f| f.borrow_mut().classes.insert((i, c)));
            return;
        }
        _ => iface(&table, from),
    };
    let Some(a) = flow else { return };
    // a class-typed target compares by the class's own layout, which a
    // resolved class type shares with any interface of its shape
    let object_target = matches!(to, Type::Struct(_)) || to_iface.is_some();
    if object_target && to_iface.as_ref() != Some(&a) {
        FLOWS.with(|f| f.borrow_mut().edges.insert((a, to_iface)));
    }
}
