//! The computed-key half of [`ShadowSet::exec_written`]: a write whose
//! key the program does not spell, decided once the checker has typed
//! the receiver and the key.
//!
//! `(r as any)[k] = f` may give a RegExp an own `exec` (or `flags`,
//! `lastIndex`, a protocol symbol) that §22.2.7.1 RegExpExec and the
//! protocol algorithms read, and the syntactic scan cannot tell it from
//! an array store. The types can: a receiver that cannot hold a RegExp
//! (an array, a string, a map) or a key that cannot be a string or a
//! symbol (a number) writes nothing a RegExp would see.

use std::collections::HashMap;

use super::ShadowSet;
use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;

impl ShadowSet {
    /// Does a checker run that answered `types` need another run with
    /// [`Self::exec_written`] set? Only when no write named `exec` set
    /// it already, some computed-key write may land on a RegExp, and
    /// some String method call with a RegExp argument still took the
    /// typed lane.
    pub(crate) fn needs_typed_exec_run(&self, ast: &Ast, types: &HashMap<ExprId, Type>) -> bool {
        !self.exec_written && typed_regexp_arg_call(ast, types) && computed_regexp_write(ast, types)
    }

    /// Carry the run-to-run seed: a set `prev` marked written stays so.
    pub(crate) fn carry_exec_written(&mut self, prev: &ShadowSet) {
        self.exec_written |= prev.exec_written;
    }

    /// Mark the set written for the next run ([`Self::needs_typed_exec_run`]).
    pub(crate) fn seed_exec_written(&mut self) {
        self.exec_written = true;
    }
}

/// A `s.<method>(re, …)` whose callee the run did not stand down.
fn typed_regexp_arg_call(ast: &Ast, types: &HashMap<ExprId, Type>) -> bool {
    ast.exprs.iter().any(|e| {
        let (Expr::Call { callee, args } | Expr::OptCall { callee, args }) = e else {
            return false;
        };
        matches!(ast.get_expr(*callee), Expr::Member { name, .. } if matches!(
            name.as_str(),
            "match" | "matchAll" | "replace" | "replaceAll" | "search" | "split"
        )) && !matches!(types.get(callee), Some(Type::Any))
            && args
                .first()
                .is_some_and(|a| matches!(types.get(a), Some(Type::RegExp)))
    })
}

/// A store or update `o[k]` whose receiver may hold a RegExp under a
/// key that may be a string or a symbol. An untyped operand counts.
fn computed_regexp_write(ast: &Ast, types: &HashMap<ExprId, Type>) -> bool {
    ast.exprs.iter().any(|e| {
        let target = match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => *target,
            _ => return false,
        };
        let Expr::Index { obj, index } = ast.get_expr(target) else {
            return false;
        };
        types.get(obj).is_none_or(may_hold_regexp)
            && types.get(index).is_none_or(|t| !is_numeric(t))
    })
}

/// Whether a value of type `t` may be a RegExp: anything but a
/// primitive or a builtin that is not one. A struct counts, since an
/// instance of a class extending RegExp is a RegExp cell.
fn may_hold_regexp(t: &Type) -> bool {
    match t {
        Type::Number
        | Type::String
        | Type::Boolean
        | Type::BigInt
        | Type::Symbol
        | Type::Literal(_)
        | Type::Null
        | Type::Undefined
        | Type::Void
        | Type::Never
        | Type::Array(_)
        | Type::Map(..)
        | Type::Set
        | Type::WeakMap
        | Type::WeakSet
        | Type::WeakRef
        | Type::MapIter(_)
        | Type::ArrIter
        | Type::Date
        | Type::Promise(_)
        | Type::Function(..) => false,
        Type::Nullable(inner) => may_hold_regexp(inner),
        Type::Union(members) => members.iter().any(may_hold_regexp),
        _ => true,
    }
}

fn is_numeric(t: &Type) -> bool {
    match t {
        Type::Number => true,
        Type::Literal(crate::check::Lit::Num(_)) => true,
        Type::Union(members) => members.iter().all(is_numeric),
        _ => false,
    }
}
