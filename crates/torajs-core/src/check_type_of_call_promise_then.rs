//! `Promise<T>.then` / `Promise<T>.catch` early-route arms
//! extracted from
//! [`crate::check_type_of_call::check`]'s top-level
//! `if let Expr::Member { … } …` cascade (chunk 207 — first
//! sub-batch of check_type_of_call.rs per-shape decomposition;
//! main file is the largest remaining file-size debt at
//! 4400 LOC).
//!
//! Covers the 4 early-route Promise.then/catch arms that
//! must run BEFORE the regular Member-method-table dispatch
//! because the method table's static signature carries fixed
//! arg count (1) and inner T constraint (homogeneous T==T):
//!
//! - T-19.l — `Promise<T>.then(onOk, onRejected)` 2-arg form
//!   (T ∈ Number / String / Boolean). Spec equivalent of
//!   `.then(onOk).catch(onRejected)`; ssa_lower emits a
//!   then→catch chain at the call site.
//! - T-19.o — heterogeneous `Promise<T>.then(cb)` /
//!   `.catch(cb)` where cb's return type U may differ from T
//!   (per ES2015). Includes the P10.7 `Promise<Any>` sub-arm
//!   (cb takes Any, returns any type → Promise<R>).
//! - P10.2-A1.1 — `Promise<Undefined>.then(cb)` /
//!   `.catch(cb)` for the 0-arg ctor (`Promise.resolve()` /
//!   `.reject()`); cb sig is `() => U`, result Promise<U> or
//!   Promise<Undefined> for Void/Undefined return.
//! - P10.2-A4 — `Promise<Array<U>>.then(cb)` / `.catch(cb)`
//!   for the `Promise.all<T>(…)` result; cb sig is
//!   `(arr: Array<U>) => V`.
//!
//! `.finally` is intentionally NOT handled here — its cb is
//! `() => void` per spec and the regular method-table arm
//! already covers that shape.
//!
//! Returns `Some(Ok(_))` on match success, `Some(Err(_))` on
//! cb shape mismatch within a matched pattern (e.g.
//! Promise<Undefined>.then with non-0-arg cb), and `None`
//! when none of the 4 patterns match (so the main `check`
//! falls through to the next early-route block or the
//! regular dispatch).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    try_then_two_arg(checker, ast, callee, args)
        .or_else(|| try_then_heterogeneous(checker, ast, callee, args))
        .or_else(|| try_then_undefined(checker, ast, callee, args))
        .or_else(|| try_then_array(checker, ast, callee, args))
}

/// T-19.l (v0.5.0) — `Promise<T>.then(onOk, onRejected)`
/// 2-arg form. Spec equivalent of `.then(onOk).catch
/// (onRejected)`. Both cbs share the simple cb shape
/// `(v: T) => T`. Routed here BEFORE the regular method
/// table because the method table's static signature
/// carries a fixed param count (1) and the generic arg-
/// count check below would reject 2-arg calls. ssa_lower
/// picks up the 2-arg shape and emits a then→catch
/// chain at the call site.
fn try_then_two_arg(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "then"
        && args.len() == 2
    {
        let src_ty = match checker.type_of(ast, *src_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if let Type::Promise(inner) = &src_ty
            && matches!(**inner, Type::Number | Type::String | Type::Boolean)
        {
            let inner_ty = (**inner).clone();
            let expected_cb = Type::Function(vec![inner_ty.clone()], Box::new(inner_ty.clone()));
            /* RFC 20260720-promise-any-cb knife 3 — either handler
             * may also be `(v: any) => R` (the knife-2 admit,
             * per-slot): the lowering's two-arg station marks each
             * PARAM_ANY independently. The result inner follows the
             * fulfilled leg: onOk `(any) => R` → Promise<R>, onOk
             * `(T) => T` → Promise<T> (onErr's return joins the
             * union in full TS; the single-value type here keeps
             * the fulfilled leg's answer). */
            let mut result_inner = inner_ty.clone();
            for (i, a) in args.iter().enumerate() {
                let aty = match checker.type_of(ast, *a) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                };
                if let Type::Function(params, ret) = &aty
                    && params.len() == 1
                    && matches!(params[0], Type::Any)
                {
                    if i == 0 {
                        result_inner = (**ret).clone();
                    }
                    continue;
                }
                if aty != expected_cb {
                    return Some(Err(format!(
                        "Promise.then arg {i}: expected {:?}, got {aty:?}",
                        expected_cb
                    )));
                }
            }
            return Some(Ok(Type::Promise(Box::new(result_inner))));
        }
    }
    None
}

/// T-19.o (v0.5.0) — generic `Promise<T>.then(cb)` /
/// `.catch(cb)` where cb's return type U may differ
/// from T (per ES2015). Routed here BEFORE the
/// method-table because the table's static signature
/// fixes T == U. We probe cb's actual signature: if
/// its param matches T and its return is a primitive
/// the runtime helper can pack through i64 (Number /
/// String / Boolean), the result is Promise<U>.
///
/// `.finally` is intentionally not handled here —
/// its cb is `() => void` per spec and the table arm
/// already covers that shape.
fn try_then_heterogeneous(
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
        // P10.7 — `Promise<Any>.then(cb)` accepts any
        // callable taking Any and returning ANY type
        // (including Void — e.g. `.then(v =>
        // console.log(v))`). Result is `Promise<R>` where
        // R is the cb's actual return type, mirroring the
        // spec: `Promise<any>.then((v) => U): Promise<U>`.
        if let Type::Promise(inner) = &src_ty
            && matches!(**inner, Type::Any)
        {
            let cb_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if let Type::Function(params, ret) = &cb_ty
                && params.len() == 1
                && matches!(params[0], Type::Any)
            {
                return Some(Ok(Type::Promise(ret.clone())));
            }
        }
        if let Type::Promise(inner) = &src_ty
            && matches!(**inner, Type::Number | Type::String | Type::Boolean)
        {
            let inner_ty = (**inner).clone();
            let cb_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            /* RFC 20260720-promise-any-cb knife 2 — `(v: any) => R`
             * over a typed Promise<T>: TS any-parameter
             * contravariance admits the handler against any T; the
             * lowering marks it PARAM_ANY so the kernel boxes the
             * settled value per the source's repr stamp (knife 1).
             * Result mirrors the P10.7 Any-inner lane:
             * `Promise<R>` from the cb's actual return type
             * (including Void — aligns with the kernel's REPR_VOID
             * ret handling). */
            if let Type::Function(params, ret) = &cb_ty
                && params.len() == 1
                && matches!(params[0], Type::Any)
            {
                return Some(Ok(Type::Promise(ret.clone())));
            }
            if let Type::Function(params, ret) = &cb_ty
                && params.len() == 1
                && params[0] == inner_ty
                && matches!(
                    **ret,
                    Type::Number | Type::String | Type::Boolean | Type::Void
                )
                && **ret != inner_ty
            {
                /* Heterogeneous T → U accepted; result is
                 * Promise<U>. Same-T case falls through to
                 * the method-table arm below (which still
                 * handles the common `(T) => T` shape).
                 * Void is TS callback-return variance —
                 * `(v: T) => void` is the plain side-effect
                 * handler (bun runs it); the kernel's
                 * REPR_VOID ret stamp zeroes the result leg
                 * (knife 1), same as the any-param lane. */
                return Some(Ok(Type::Promise(ret.clone())));
            }
        }
    }
    None
}

/// P10.2-A1.1 (resumed-session 2026-05-21) —
/// `Promise<Undefined>.then(cb)` / `.catch(cb)`. The
/// 0-arg ctor `Promise.resolve()` / `.reject()` (A1)
/// produces inner T=Undefined, which the generic
/// arm above rejects (it limits inner T to the i64-
/// roundtrippable Number/String/Boolean primitives).
///
/// cb sig is `() => U` — ergonomic surface for the
/// 0-arg ctor (TS spec actually wants `(v: undefined)
/// => U`, but bare-`() => U` is what real code looks
/// like and what bun accepts as a structural sig).
/// The helper still calls cb via SystemV `int64_t
/// (*)(int64_t)`; cb just ignores its argument slot.
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
fn try_then_undefined(
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
        if let Type::Promise(inner) = &src_ty
            && matches!(**inner, Type::Undefined)
        {
            let cb_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if let Type::Function(params, ret) = &cb_ty
                && params.is_empty()
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
                return Some(Ok(Type::Promise(Box::new(result_inner))));
            }
            return Some(Err(format!(
                "Promise.{m_name} on Promise<Undefined>: cb must be 0-arg `() => U`, got {cb_ty:?}"
            )));
        }
    }
    None
}

/// P10.2-A4 (resumed-session 2026-05-22) —
/// `Promise<Array<U>>.then(cb)` / `.catch(cb)`. The
/// Promise.all<T>(promises) result has inner T=Array<U>,
/// which the generic .then/.catch arm above rejects
/// (it limits inner T to the i64-roundtrippable
/// Number/String/Boolean primitives).
///
/// cb sig is `(arr: Array<U>) => V` per spec (1-arg
/// structural sig accepted; mirrors A1.1's 0-arg arm
/// pattern for Promise<Undefined>). SystemV `int64_t
/// (*)(int64_t)` already passes Array ptr in rdi; cb
/// reads it directly. No runtime change.
///
/// cb return V: primitive (Number / String / Boolean)
/// → Promise<V>; Void / Undefined → Promise<Undefined>.
/// Array<W> return is deferred (would need helper-side
/// value_is_heap=true propagation for next Promise's
/// heap value — separate sub-A).
///
/// cb does NOT retain the array past invocation —
/// source Promise still owns the Array ref for its
/// lifetime; the cb only reads through the passed
/// ptr. So no rc concern in either direction.
fn try_then_array(
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
        if let Type::Promise(inner) = &src_ty
            && matches!(**inner, Type::Array(_))
        {
            let inner_arr_ty = (**inner).clone();
            let cb_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            /* RFC 20260720-promise-any-cb residual — `(v: any) => R`
             * over the Array-inner lane too (the combinator-result
             * shape: `Promise.allSettled(...).then((r: any) => ...)`).
             * Same knife-2 admit: the lowering marks PARAM_ANY off
             * the SSA sig and the kernel boxes per the source's
             * REPR_HEAP stamp; result is Promise<R>. */
            if let Type::Function(params, ret) = &cb_ty
                && params.len() == 1
                && matches!(params[0], Type::Any)
            {
                return Some(Ok(Type::Promise(ret.clone())));
            }
            if let Type::Function(params, ret) = &cb_ty
                && params.len() == 1
                && params[0] == inner_arr_ty
            {
                let result_inner = match &**ret {
                    Type::Number | Type::String | Type::Boolean => (**ret).clone(),
                    Type::Void | Type::Undefined => Type::Undefined,
                    other => {
                        return Some(Err(format!(
                            "Promise.{m_name} on Promise<{inner_arr_ty:?}>: cb return must be Number / String / Boolean / Void / Undefined (got {other:?}); Array<W> return deferred to a later sub-A"
                        )));
                    }
                };
                return Some(Ok(Type::Promise(Box::new(result_inner))));
            }
            return Some(Err(format!(
                "Promise.{m_name} on Promise<{inner_arr_ty:?}>: cb must be `(arr: {inner_arr_ty:?}) => V`, got {cb_ty:?}"
            )));
        }
    }
    None
}
