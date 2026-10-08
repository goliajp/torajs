//! The checker's view of the witness table: a class instance fits an
//! interface that dispatches through witness slots exactly when the
//! class fills a row for it.
//!
//! The assignability lattice is a set of pure functions over the type
//! tables, called from every slot comparison in the checker; the table
//! rides beside them for the duration of a compile instead of being
//! threaded through each of those call sites. It is written once per
//! checker run, from the AST the run checks.

use std::cell::RefCell;
use std::rc::Rc;

use crate::check::Type;

thread_local! {
    static TABLE: RefCell<Rc<super::IfaceSlots>> = RefCell::new(Rc::default());
}

pub(crate) fn install(ast: &crate::ast::Ast) {
    TABLE.with(|t| *t.borrow_mut() = Rc::new(ast.iface_slots.clone()));
}

/// `Some(fits)` when `to` is an interface that dispatches through
/// witness slots and `from` a class instance, both as the checker
/// wrote them (unresolved); `None` leaves the pairing to the structural
/// rules. Arrays, nullables and promises are peeled pairwise first: the
/// structural rules resolve the class away before they recurse.
pub(crate) fn class_admit(to: &Type, from: &Type) -> Option<bool> {
    match (to, from) {
        (Type::Array(t), Type::Array(f))
        | (Type::Promise(t), Type::Promise(f))
        | (Type::Nullable(t), Type::Nullable(f)) => class_admit(t, f),
        (Type::Nullable(t), f) if !matches!(f, Type::Null | Type::Undefined) => class_admit(t, f),
        (Type::Struct(_), Type::ClassRef(c)) => {
            let c = c.split('<').next().unwrap_or(c);
            let table = TABLE.with(|t| t.borrow().clone());
            let iface = table.iface_of(to)?;
            Some(table.witnesses.get(iface).is_some_and(|cs| cs.contains(c)))
        }
        _ => None,
    }
}
