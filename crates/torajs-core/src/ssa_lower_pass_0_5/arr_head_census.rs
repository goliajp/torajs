//! Whole-program census: can any array in this program end up with a
//! non-zero deque head?
//!
//! The runtime moves an array's head only in `shift` (`unshift` on a
//! head with slack); every other kernel folds the head back to zero
//! or builds a fresh array with a zero head. So the head of every
//! array is pinned at zero unless something in the program can reach
//! those two methods, and then a typed element address is
//! `data + idx * stride` at every access site, whatever the receiver's
//! spelling — which is what the fn-local escape walk in
//! `ssa_lower_deque_escape` can only prove for an identifier that is
//! never handed out.
//!
//! The census is syntactic and over-approximates every road to the
//! method: its name as a member or a string literal (any-lane
//! dispatch interns the name at compile time), a bare identifier of
//! that name (a `with` body resolves it at run time), the reflection
//! surfaces that hand out a method by a computed name, and a
//! computed non-numeric key on a receiver that could be an array.
//! Any hit means "not pinned"; the cost of a false hit is the
//! head-aware address arithmetic the program paid before. The bodies
//! of method twins no foreign receiver can enter are left out
//! ([`twin_roads`]): their `any`-typed copies of the method's reads
//! never run.

mod twin_roads;

pub(crate) use twin_roads::{ExprMask, TwinBodies, unreachable_twin_exprs};

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;

pub(crate) fn program_pins_arr_head(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    dead_twins: &ExprMask,
) -> bool {
    !ast.exprs.iter().enumerate().any(|(i, e)| {
        !dead_twins.contains(ExprId(i as u32)) && expr_may_reach_head_mutation(ast, expr_types, e)
    })
}

fn expr_may_reach_head_mutation(ast: &Ast, expr_types: &HashMap<ExprId, Type>, e: &Expr) -> bool {
    match e {
        Expr::Ident(n) => matches!(
            n.as_str(),
            "shift" | "unshift" | "Reflect" | "eval" | "Function"
        ),
        Expr::Member { name, .. } | Expr::OptChain { name, .. } => matches!(
            name.as_str(),
            "shift"
                | "unshift"
                | "getOwnPropertyDescriptor"
                | "getOwnPropertyDescriptors"
                | "getOwnPropertyNames"
        ),
        Expr::String(s) => s == "shift" || s == "unshift",
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            !key_cannot_name_method(expr_types.get(index))
                && !receiver_cannot_be_array(ast, expr_types.get(obj))
        }
        _ => false,
    }
}

/// ToPropertyKey of a value of this type never spells a method name.
fn key_cannot_name_method(t: Option<&Type>) -> bool {
    matches!(
        t,
        Some(
            Type::Number
                | Type::Boolean
                | Type::BigInt
                | Type::Null
                | Type::Undefined
                | Type::Symbol
        )
    )
}

/// A value of this type is never an array (nor a class extending one),
/// so a computed key on it cannot reach `Array.prototype`.
fn receiver_cannot_be_array(ast: &Ast, t: Option<&Type>) -> bool {
    match t {
        Some(Type::ClassRef(name)) => !class_extends_array(ast, name),
        Some(Type::Nullable(inner)) => receiver_cannot_be_array(ast, Some(inner)),
        Some(
            Type::Struct(_)
            | Type::String
            | Type::Number
            | Type::Boolean
            | Type::BigInt
            | Type::Date
            | Type::RegExp
            | Type::Symbol
            | Type::Promise(_)
            | Type::Map(..)
            | Type::Set
            | Type::MapIter(..)
            | Type::ArrIter
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::Null
            | Type::Undefined
            | Type::Void
            | Type::Never,
        ) => true,
        _ => false,
    }
}

fn class_extends_array(ast: &Ast, name: &str) -> bool {
    let mut cur = name;
    // a parent chain is finite, but a malformed cycle must not spin
    for _ in 0..ast.class_parents.len() + 1 {
        match ast.class_parents.get(cur) {
            Some(Some(parent)) if parent == "Array" => return true,
            Some(Some(parent)) => cur = parent,
            _ => return false,
        }
    }
    true
}
