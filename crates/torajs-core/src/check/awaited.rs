//! The type of `await e` (§27.7.5.3 Await; TS `Awaited<T>`).
//!
//! A promise answers its inner type: it is never fulfilled with a
//! thenable (it adopts one instead), unless that type declares a
//! `then`, whose settlement no type tells. A primitive answers
//! itself (§27.2.1.3.2 step 8: a resolution that is not an Object
//! fulfills). Any other object answers `any`: whether it is a
//! thenable is decided by a `then` it may carry at run time (an
//! expando property, a subclass method), and what that `then`
//! settles with has no type.

use super::{Checker, Type};
use crate::ast::Ast;

impl Checker {
    pub(super) fn awaited(&self, ast: &Ast, operand: Type) -> Type {
        let has_fn = |f: &str| self.globals.contains_key(f);
        match operand {
            Type::Promise(inner) if may_be_thenable(&inner, ast, &has_fn) => Type::Any,
            Type::Promise(inner) => *inner,
            t if settles_as_itself(&t) => t,
            _ => Type::Any,
        }
    }
}

/// The types no object value comes in.
fn settles_as_itself(ty: &Type) -> bool {
    match ty {
        Type::Number
        | Type::String
        | Type::Boolean
        | Type::Void
        | Type::Never
        | Type::BigInt
        | Type::Symbol
        | Type::Null
        | Type::Undefined
        | Type::Literal(_)
        | Type::Any
        | Type::TypeVar(_) => true,
        Type::Nullable(t) => settles_as_itself(t),
        Type::Union(ts) => ts.iter().all(settles_as_itself),
        _ => false,
    }
}

/// Whether a value of `ty` may carry a `then` member by its type: an
/// object type that has one, or an instance of a class that declares
/// one, inherits one, or has a subclass that declares one (such an
/// instance can sit under the class's type). `has_fn` answers whether
/// a function of that name exists (a class method is `__cm_<C>__then`).
pub(crate) fn may_be_thenable(ty: &Type, ast: &Ast, has_fn: &dyn Fn(&str) -> bool) -> bool {
    match ty {
        Type::Struct(fields) => fields.iter().any(|(k, _)| k.lossy() == "then"),
        Type::ClassRef(c) => {
            let kin = |k: &str| extends(ast, k, c) || extends(ast, c, k);
            std::iter::once(c.as_str())
                .chain(ast.class_parents.keys().map(String::as_str))
                .any(|k| kin(k) && has_fn(&format!("__cm_{k}__then")))
        }
        Type::Nullable(t) => may_be_thenable(t, ast, has_fn),
        Type::Union(ts) => ts.iter().any(|t| may_be_thenable(t, ast, has_fn)),
        _ => false,
    }
}

/// Is `class` `base` or one of its subclasses?
fn extends(ast: &Ast, class: &str, base: &str) -> bool {
    let mut cur = Some(class);
    while let Some(c) = cur {
        if c == base {
            return true;
        }
        cur = ast.class_parents.get(c).and_then(|p| p.as_deref());
    }
    false
}
