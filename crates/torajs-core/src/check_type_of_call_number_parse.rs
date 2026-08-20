//! `Number.parseInt` / `Number.parseFloat` early-route arms
//! extracted from
//! [`crate::check_type_of_call::check`]'s top-level
//! `if let Expr::Member { … } &&  Expr::Ident(ns) && ns == "Number"`
//! cascade (chunk 209 — third sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! Covers the 2 Number namespace static methods that need
//! early-route handling because the regular static-method
//! table fixes arity / arg type in ways the spec ignores:
//!
//! - V3-18 wedge — `Number.parseInt(s)` 1-arg + 2-arg form
//!   per §21.1.2.13. Pre-wedge the declared sig was
//!   `Function([String, Number], Number)` so the 1-arg form
//!   failed the arity check. Mirror of the global `parseInt`
//!   handler. SSA-lower already handles the 1-arg shape
//!   (passes ConstI64(0) as the auto-detect radix sentinel).
//!   Per S202 / S226 also accepts an Undefined arg; per
//!   S327 widens the radix to also accept Any (spec coerces
//!   via ToInt32 anyway). Trailing args[2..] silent-drop
//!   per ES §21.1.2.13.
//! - S202 — `Number.parseFloat(s)` 0-arg + 1-arg per
//!   §21.1.2.12. Alias to global parseFloat; missing string
//!   defaults to undefined → ToString → "undefined" → NaN.
//!   Pre-fix the declared `vec![Type::String]` was rejecting
//!   the 0-arg form. Per S226 / S330 widens to also accept
//!   Undefined / Any args (ssa_lower decodes Any via
//!   anyv_to_str_pair). Trailing args[1..] silent-drop.
//!
//! Returns `Some(Ok(_))` on match, `Some(Err(_))` on arg shape
//! mismatch, `None` when callee isn't `Number.parseInt` or
//! `Number.parseFloat`.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // V3-18 wedge — `Number.parseInt(s)` and
    // `Number.parseInt(s, radix)`. Per JS spec §21.1.2.13
    // the radix is optional; bare `Number.parseInt(s)`
    // auto-detects (`0x` prefix → 16, otherwise 10).
    // Pre-fix the type was declared as
    // `Function([String, Number], Number)` so the 1-arg
    // form failed at the unified arity check. Mirror of
    // the global `parseInt` handler at line ~4615.
    // SSA lower already handles the 1-arg shape (passes
    // ConstI64(0) as the auto-detect radix sentinel).
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "parseInt"
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Number"
    {
        // S253 — Number.parseInt(str, radix, ...trailing)
        // per ES §21.1.2.13 trailing-arg ignore. SSA-emit
        // reads args[0..=1] (or less), so args[2..]
        // dropped at lower-time.
        for &arg in args.iter().skip(2) {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        // S202 — spec §21.1.2.13 Number.parseInt aliases
        // global parseInt. Step 1 reads `string` which
        // defaults to undefined when omitted; ToString →
        // "undefined" → parse fails → NaN.
        //
        // S226 — also accept an explicit `undefined` arg
        // via the same ToString("undefined") → NaN path;
        // ssa_lower mirror folds the call to ConstF64(NaN).
        if let Some(arg0) = args.first() {
            let s_ty = match checker.type_of(ast, *arg0) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            // Rotation 461 — step 1 is ToString, which takes any
            // value; the global spelling already accepted Any here
            // and this one is the same function object (§21.1.2.13).
            let _ = s_ty;
        }
        if args.len() == 2 {
            let r_ty = match checker.type_of(ast, args[1]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            // S234 — accept Undefined radix per ES §21.1.2.13
            // (aliases §19.2.5.1 step 2-3): ToInt32(undefined)=0,
            // step 8 R==0 → R=10 default. ssa_lower mirror
            // substitutes ConstI64(0) for the helper's
            // auto-detect branch.
            //
            // S327 — accept Any radix. Spec §19.2.5.1 step 2
            // calls ToInt32 on the radix, which coerces Any
            // (NaN→0, ∞→0, etc.).
            //
            // Rotation 461 — and everything else, for the same
            // reason: ToInt32's ToNumber takes any value, so
            // the shape guard was answering a question the spec
            // does not ask (`parseInt("11", "16")` is 17 in
            // every engine). ssa_lower boxes whatever it gets
            // and runs the same route. `let _ = r_ty;` keeps
            // the arg's own type error surfacing above.
            let _ = r_ty;
        }
        return Some(Ok(Type::Number));
    }
    // S202 — Number.parseFloat 0-arg per ES §21.1.2.12.
    // Alias to global parseFloat; missing string defaults
    // to undefined → ToString → "undefined" → NaN. The
    // declared `vec![Type::String]` signature was rejecting
    // the 0-arg form through the generic arity gate.
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "parseFloat"
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Number"
    {
        // S253 — Number.parseFloat(str, ...trailing) per
        // ES §21.1.2.12 trailing-arg ignore. SSA-emit
        // reads args[0] (or empty), so args[1..] dropped
        // at lower-time.
        for &arg in args.iter().skip(1) {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        // S226 — explicit undefined arg → NaN (same path).
        //
        // S330 — widen accept Any. Spec §19.2.5.2 step 1
        // calls ToString on the operand, which already
        // coerces arbitrary-typed input. ssa_lower mirror
        // decodes Any via anyv_to_str_pair (tag + value)
        // before passing to num_parse_float. Sister to
        // S327 (parseInt Any radix).
        if let Some(arg0) = args.first() {
            let s_ty = match checker.type_of(ast, *arg0) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if !matches!(s_ty, Type::String | Type::Undefined | Type::Any) {
                return Some(Err(format!(
                    "Number.parseFloat arg 0 must be string, got {s_ty:?}"
                )));
            }
        }
        return Some(Ok(Type::Number));
    }
    None
}
