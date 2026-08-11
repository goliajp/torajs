//! Bare-Ident JS globals dispatch (parseInt / parseFloat /
//! isNaN / isFinite / queueMicrotask) extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 221 — fourteenth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! All five share the same outer guard
//! `Expr::Ident(name) = callee` and dispatch into an inner
//! `match name.as_str()`. Each branch returns the spec-aligned
//! result type after typechecking the arg shape:
//!
//! - **parseInt** (S252 / S202 / S226 / S337 / S234) —
//!   ES §19.2.5. Accepts String / Undefined / Any in slot 0
//!   and Number / Undefined / Any in slot 1; trailing args
//!   silent-dropped per generic trailing-arg-ignore.
//!   Returns `Number`.
//!
//! - **parseFloat** (S252 / S202 / S226 / S336) —
//!   ES §19.2.4. Accepts String / Undefined / Any in slot 0;
//!   trailing silent-drop. Returns `Number`.
//!
//! - **isNaN / isFinite** (S252 / S202 + V3-18 wedge) —
//!   ES §19.2.3 / §19.2.4. Global form coerces via ToNumber
//!   before testing (distinct from strict `Number.isNaN`);
//!   any coercible type accepted, 0-arg form skips the
//!   typecheck. Returns `Boolean`.
//!
//! - **queueMicrotask** (P10.1-A1 / S323) —
//!   WHATWG HTML §queueMicrotask. cb must be exactly
//!   `() => void`; trailing args silent-drop per Web IDL
//!   §3.2.1 over-arity. Returns `Void`.
//!
//! Returns `Some(Ok(_))` on match, `Some(Err(_))` on arg
//! shape mismatch, `None` when callee isn't a bare-Ident
//! naming one of the dispatched globals (other globals
//! reach the regular cascade arms below in the main file).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Ident(name) = ast.get_expr(*callee) {
        match name.as_str() {
            "parseInt" => {
                // S252 — parseInt(str, radix, ...trailing)
                // per ES §19.2.5 trailing-arg ignore. SSA-emit
                // reads args[0..=1] (or less), so args[2..]
                // dropped at lower-time.
                for &arg in args.iter().skip(2) {
                    if let Err(e) = checker.type_of(ast, arg) {
                        return Some(Err(e));
                    }
                }
                // S202 / S226 / S337 — accept String,
                // Undefined, or Any in slot 0 (ToString in
                // step 1).
                if let Some(arg0) = args.first() {
                    let s_ty = match checker.type_of(ast, *arg0) {
                        Ok(t) => t,
                        Err(e) => return Some(Err(e)),
                    };
                    if !matches!(s_ty, Type::String | Type::Undefined | Type::Any) {
                        return Some(Err(format!("parseInt arg 0 must be string, got {s_ty:?}")));
                    }
                }
                if args.len() == 2 {
                    let r_ty = match checker.type_of(ast, args[1]) {
                        Ok(t) => t,
                        Err(e) => return Some(Err(e)),
                    };
                    // S234 / S337 — accept Number, Undefined,
                    // or Any in slot 1 (ToInt32 step 2).
                    if !matches!(r_ty, Type::Number | Type::Undefined | Type::Any) {
                        return Some(Err(format!("parseInt arg 1 must be number, got {r_ty:?}")));
                    }
                }
                Some(Ok(Type::Number))
            }
            "parseFloat" => {
                // S252 — parseFloat(str, ...trailing) per
                // ES §19.2.4 trailing-arg ignore.
                for &arg in args.iter().skip(1) {
                    if let Err(e) = checker.type_of(ast, arg) {
                        return Some(Err(e));
                    }
                }
                // S202 / S226 / S336 — accept String,
                // Undefined, or Any in slot 0.
                if let Some(arg0) = args.first() {
                    let s_ty = match checker.type_of(ast, *arg0) {
                        Ok(t) => t,
                        Err(e) => return Some(Err(e)),
                    };
                    if !matches!(s_ty, Type::String | Type::Undefined | Type::Any) {
                        return Some(Err(format!("parseFloat arg must be string, got {s_ty:?}")));
                    }
                }
                Some(Ok(Type::Number))
            }
            "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent" => {
                // §19.2.6 URI globals — one ToString'd argument
                // (String / Undefined / Any accepted like the
                // parse* pair), trailing args Web-IDL ignored.
                for &arg in args.iter().skip(1) {
                    if let Err(e) = checker.type_of(ast, arg) {
                        return Some(Err(e));
                    }
                }
                if let Some(arg0) = args.first() {
                    let s_ty = match checker.type_of(ast, *arg0) {
                        Ok(t) => t,
                        Err(e) => return Some(Err(e)),
                    };
                    if !matches!(s_ty, Type::String | Type::Undefined | Type::Any) {
                        return Some(Err(format!("{name} arg must be string, got {s_ty:?}")));
                    }
                }
                Some(Ok(Type::String))
            }
            "isNaN" | "isFinite" => {
                // S252 — isNaN/isFinite(value, ...trailing)
                // per ES §19.2.3 / §19.2.4 trailing-arg
                // ignore.
                for &arg in args.iter().skip(1) {
                    if let Err(e) = checker.type_of(ast, arg) {
                        return Some(Err(e));
                    }
                }
                // V3-18 wedge — global isNaN / isFinite apply
                // ToNumber on the argument before testing the
                // predicate (intentional contrast with the
                // strict Number.isNaN / Number.isFinite
                // namespaced methods that don't coerce).
                // 0-arg form skips the typecheck entirely;
                // ssa_lower returns the spec constant
                // (isNaN→true / isFinite→false).
                if let Some(arg0) = args.first() {
                    if let Err(e) = checker.type_of(ast, *arg0) {
                        return Some(Err(e));
                    }
                }
                Some(Ok(Type::Boolean))
            }
            "queueMicrotask" => {
                // P10.1-A1 — WHATWG HTML §queueMicrotask:
                // schedule cb to run as a microtask before
                // the next event-loop turn. cb is exactly
                // `() => void`. Higher arities / non-void ret
                // / simple-fn (no-env) defer to A1.1.
                //
                // S323 — Web IDL §3.2.1 over-arity rule:
                // operations silently ignore trailing args.
                if args.is_empty() {
                    return Some(Err("queueMicrotask expects 1 arg, got 0".to_string()));
                }
                let cb_ty = match checker.type_of(ast, args[0]) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                };
                match &cb_ty {
                    // TS void-callback variance — a callback in a
                    // void-returning position may return anything;
                    // the value is discarded (bun/tsc accept
                    // `queueMicrotask(() => order.push(1))`, and the
                    // chunk-607 ret fallback types un-sniffable cbs
                    // as `() => Any`). Param count stays exact.
                    Type::Function(params, _) if params.is_empty() => {}
                    _ => {
                        return Some(Err(format!(
                            "queueMicrotask cb must be `() => void`, got {cb_ty:?}"
                        )));
                    }
                }
                for &a in args.iter().skip(1) {
                    if let Err(e) = checker.type_of(ast, a) {
                        return Some(Err(e));
                    }
                }
                Some(Ok(Type::Void))
            }
            _ => None,
        }
    } else {
        None
    }
}
