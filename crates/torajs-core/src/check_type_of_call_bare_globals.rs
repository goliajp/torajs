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
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    if let Expr::Ident(name) = ast.get_expr(*callee) {
        // A module that writes this property of the global object
        // takes the general cascade: the callee reads Any there
        // (`check_type_of_ident`), which is what routes the call to
        // the runtime dispatcher.
        if checker.proto_shadow.writes_global(name) {
            return None;
        }
        // §9.1.1.1 — a user binding of the name (local, parameter,
        // capture, top-level function or binding) answers before the
        // global object, so the call is an ordinary call of it.
        if checker.lookup(name).is_some()
            || checker.globals.contains_key(name)
            || checker.hoisted_closure_lets.contains_key(name)
        {
            return None;
        }
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
                    // Rotation 461 — step 1 is ToString, which takes
                    // any value at all.
                    let _ = s_ty;
                }
                if args.len() == 2 {
                    // Step 2 is ToInt32(radix), whose ToNumber takes
                    // any value at all — the shape guard that used to
                    // stand here answered a question the spec does not
                    // ask (`parseInt("11", "16")` is 17 everywhere).
                    // Rotation 461; ssa_lower boxes whatever it gets.
                    if let Err(e) = checker.type_of(ast, args[1]) {
                        return Some(Err(e));
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
                // §19.2.4 step 1 is ToString(string), which takes
                // ANY value — `parseFloat(1.5)` is 1.5 and
                // `parseFloat(null)` is NaN on every engine. The slot
                // still types its argument (the visit records
                // `expr_types`, which the lowerer reads), it just
                // does not refuse what the spec coerces.
                //
                // `parseInt` above lost the same guard in rotation
                // 461 with the same reasoning; this was the second
                // spelling of one sentence, and only one of them had
                // been corrected. `decode_any_to_str` boxes whatever
                // it gets, so the lowering side was already there.
                if let Some(arg0) = args.first()
                    && let Err(e) = checker.type_of(ast, *arg0)
                {
                    return Some(Err(e));
                }
                Some(Ok(Type::Number))
            }
            "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent" | "escape"
            | "unescape" => {
                // §19.2.6 URI globals — one ToString'd argument
                // (String / Undefined / Any accepted like the
                // parse* pair), trailing args Web-IDL ignored.
                for &arg in args.iter().skip(1) {
                    if let Err(e) = checker.type_of(ast, arg) {
                        return Some(Err(e));
                    }
                }
                // Same ToString as `parseFloat` above — §19.2.6.x
                // step 1 for the URI four, §B.2.1 step 1 for
                // `escape` / `unescape`. `escape(null)` is "null" and
                // `escape(1n)` is "1"; refusing them refused what the
                // spec spells out.
                if let Some(arg0) = args.first()
                    && let Err(e) = checker.type_of(ast, *arg0)
                {
                    return Some(Err(e));
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
                if let Some(arg0) = args.first()
                    && let Err(e) = checker.type_of(ast, *arg0)
                {
                    return Some(Err(e));
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
