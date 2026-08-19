//! P10.2-A1.1 — `Promise<Undefined>.then(cb)` / `.catch(cb)` for
//! the 0-arg ctor (`Promise.resolve()` / `.reject()`); cb sig is
//! `() => U`, result Promise<U> (or Promise<Undefined> for a
//! Void / Undefined return). Split verbatim from
//! `check_type_of_call_promise_then.rs` at its 500-line watch
//! (rotation 452 — the 0-arg arm's wiring tipped it; this arm was
//! the most self-contained resident).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};
use crate::check_type_of_call_promise_then::promise_of;

/// P10.2-A1.1 (resumed-session 2026-05-21) —
/// `Promise<Undefined>.then(cb)` / `.catch(cb)`. The
/// 0-arg ctor `Promise.resolve()` / `.reject()` (A1)
/// produces inner T=Undefined, which the generic
/// arm above rejects (it limits inner T to the i64-
/// roundtrippable Number/String/Boolean primitives).
///
/// cb sig is `() => U` or `(v) => U`. The spec form is
/// the latter (the callback is handed the settled
/// value, which here is `undefined`); the former is
/// what most real code writes for a promise carrying
/// nothing. Rejecting the one-argument shape turned
/// `Promise.resolve().then((v) => …)` — which bun
/// accepts and runs — into a type error.
/// The helper calls cb via SystemV `int64_t
/// (*)(int64_t)` either way; a 0-arg cb just ignores
/// its argument slot.
///
/// cb return U: primitive (Number / String / Boolean)
/// → Promise<U>; Void / Undefined → Promise<Undefined>.
///
/// Both closure-typed and simple-fn-typed cb shapes
/// are accepted at this layer; ssa_lower's existing
/// cb_ty Closure/FnSig dispatch (line ~17220) routes
/// to promise_then_closure / _simple correctly without
/// any Promise<T> inner-T inspection (SSA Type::Promise
/// is a unit variant).
pub(crate) fn try_then_undefined(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && (m_name == "then" || m_name == "catch")
        && args.len() == 1
    {
        let src_ty = match checker.type_of(ast, *src_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        /* rotation 233 — Void rides the Undefined lane: a
         * Promise(Void) cell settles with `undefined` (the kernel's
         * REPR_VOID ret stamp zeroes the result leg), so its
         * `.then`/`.catch` contract is exactly this arm's. */
        if let Type::Promise(inner) = &src_ty
            && matches!(**inner, Type::Undefined | Type::Void)
        {
            let cb_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if let Type::Function(params, ret) = &cb_ty
                && params.len() <= 1
            {
                let result_inner = match &**ret {
                    Type::Number | Type::String | Type::Boolean => (**ret).clone(),
                    Type::Void | Type::Undefined => Type::Undefined,
                    // Chunk 607's ret fallback types un-sniffable cbs
                    // as `() => Any` — the then-result is Promise<Any>
                    // (the existing P10.7 Any lane).
                    Type::Any => Type::Any,
                    other => {
                        return Some(Err(format!(
                            "Promise.{m_name} on Promise<Undefined>: cb return must be Number / String / Boolean / Void / Undefined, got {other:?}"
                        )));
                    }
                };
                return Some(Ok(promise_of(&result_inner)));
            }
            return Some(Err(format!(
                "Promise.{m_name} on Promise<Undefined>: cb must be 0-arg `() => U`, got {cb_ty:?}"
            )));
        }
    }
    None
}
