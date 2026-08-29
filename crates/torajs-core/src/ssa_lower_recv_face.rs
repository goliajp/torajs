//! The receiver of a typed method call, however it is spelled.
//!
//! Each typed dispatch lane asks the same two questions before it
//! claims a call — *is this receiver the shape I serve*, and *what
//! operand do I dispatch on* — and each answered the first with its
//! own private table of AST shapes: an identifier reads the local
//! table, a member / index / call / new reads the checker's
//! `expr_types`, and everything else answers None. Written three
//! times, the tables drifted the way tables do: none of them listed
//! `Expr::As`, so `(o.m as Map<string, number>).get("k")` — and the
//! same spelling for Set, WeakMap and WeakSet — reached the
//! dispatcher cascade's terminal panic, `unsupported member call
//! shape: get`. A cast receiver is not exotic; it is how a value
//! comes back out of `any`.
//!
//! Answering it in one place also made the second half visible.
//! A lane dispatches on the operand `lower_expr` produces, and for
//! a cast off an `any` source that operand used to stay a NaN-box:
//! the lane would have read a Map header off a boxed word. That is
//! fixed where it belongs, in `lower_as_cast` — the typed tier
//! materializes the face an annotation names, which it already did
//! for the primitives.
//!
//! Only the shapes that name ONE heap layout are resolved here.
//! `Array<T>` and function types need an interned element type or
//! signature to name their SSA type, which is a different question
//! and keeps its own lanes.

use crate::ast::{Expr, ExprId};
use crate::check as check_mod;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// The receiver's static SSA type. An identifier answers from the
/// local table; every other spelling answers from the checker, which
/// is where a cast's annotation lands.
pub(crate) fn static_ty(ctx: &LowerCtx<'_>, obj: ExprId) -> Option<Type> {
    if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        return ctx.locals.get(n).map(|info| info.ty);
    }
    monomorphic_face(ctx.expr_types.get(&obj)?)
}

/// The `check::Type`s that name one heap layout outright.
pub(crate) fn monomorphic_face(t: &check_mod::Type) -> Option<Type> {
    Some(match t {
        check_mod::Type::Map => Type::Map,
        check_mod::Type::Set => Type::Set,
        check_mod::Type::WeakMap => Type::WeakMap,
        check_mod::Type::WeakSet => Type::WeakSet,
        check_mod::Type::WeakRef => Type::WeakRef,
        check_mod::Type::Date => Type::Date,
        check_mod::Type::RegExp => Type::RegExp,
        check_mod::Type::Symbol => Type::Symbol,
        check_mod::Type::MapIter => Type::MapIter,
        check_mod::Type::ArrIter => Type::ArrIter,
        check_mod::Type::Promise(_) => Type::Promise,
        _ => return None,
    })
}
