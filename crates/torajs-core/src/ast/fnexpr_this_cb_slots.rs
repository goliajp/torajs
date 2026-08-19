//! Builtin callback-slot face collectors — carved out of
//! `fnexpr_this.rs`'s position walk (file-size hard limit: the
//! rotation-375 replace arm pushed the file to 517 and the walk fn
//! to 226). Verbatim moves; each arm's receiver check moved from the
//! match guard into the collector body, which is fall-through-safe
//! because no later cascade arm matches the same member name.

use super::fnexpr_this_faces::{FacePatch, collect_face, collect_ident_face};
use super::{Expr, ExprId, Stmt};

/// RFC 20260808-json-parse-any blade 4 — `JSON.parse(text,
/// <fn-expr>)`: the reviver slot. §25.5.1.1 calls it with the holder
/// as `this`; the internalize kernel dispatches through
/// `invoke_with_this`, which reads the closure's RECV flag and seeds
/// the holder into the promoted `__this` slot. Inline fn-expr only —
/// zero aliases.
///
/// `JSON.stringify(value, <fn-expr>)`'s replacer slot is the same
/// shape and shares this collector: §25.5.2.2 step 3 calls it with
/// the holder as `this` (the synthetic `{ "": value }` wrapper at the
/// root), through the same `invoke_with_this`. Every admitted
/// spelling routes to that kernel — the lowering's
/// `serves_as_replacer` takes a `Function` or an `Any` out of the
/// static unfold — so the promotion can never outrun the seeding.
pub(super) fn collect_json_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    args: &[ExprId],
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "JSON") {
        return;
    }
    if let Some(rev) = args.get(1) {
        collect_face(stmts, exprs, *rev, fn_expr_exprs, patches);
        collect_ident_face(exprs, *rev, ident_cands);
    }
}

/// Rotation 375 — string-pattern `.replace` / `.replaceAll`
/// functional replaceValue over a STRING-LITERAL receiver
/// (`"ab".replace("b", <fn-expr>)`, the function-code 10.4.3
/// family). The literal receiver + literal pattern pin the lowering
/// to the str replace_fn lane (`ssa_lower_str_replace_fn`'s
/// String/Null/Undefined pattern gate), whose kernel reads the
/// closure's receiver-first flag and seeds argv[0] undefined
/// (§22.1.3.18 step 10's Call(replaceValue, undefined, «…»)). A
/// non-literal receiver or a regex pattern keeps the loud reject —
/// the regex replacer lane dispatches by typed transmute and has no
/// receiver slot (L3b 375-01). The inline fn-expr and the
/// variable-routed Ident (knife 2's zero-alias profile, rotation
/// 446) both admit. The IIFE-return spelling was tried and REVERTED
/// in rotation 375 — the checker typed the IIFE call's result as
/// non-Function, the lowering fell to the ToString replaceValue
/// path, and the promote turned a loud reject into source-text
/// splicing — and rotation 446 unblocked it at the root: an `any`
/// replaceValue now answers §22.1.3.19 step 5's IsCallable at
/// runtime (`__torajs_str_replace_any_repl`), so the fn-return
/// faces and this slot compose instead of colliding.
///
/// Rotation 447 — an inline `new String(...)` receiver admits with
/// NO pattern gate: the wrapper mint checker-types `Any`
/// (`check_type_of_new.rs`), so the whole call rides the runtime
/// any-dispatch, where BOTH pattern lanes now serve a functional
/// replaceValue through receiver-flag-aware kernels (the literal
/// glue reads FLAG_CLOSURE_RECV_FIRST; the regex lane's boxed
/// kernel does the same with the pattern's own capture count).
/// A variable-routed wrapper receiver admits on the same argument
/// through `strwrapper_recvs` (the syntactically-certain census in
/// `fnexpr_this_recvs.rs` — every reaching value is a mint).
/// Fail-safe: this shape was an unconditional unclaimed loud
/// reject before, so admitting it can only turn a compile error
/// into the spec answer.
pub(super) fn collect_replace_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    args: &[ExprId],
    strwrapper_recvs: &std::collections::HashSet<String>,
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    let wrapper_recv = matches!(
        &exprs[obj.0 as usize],
        Expr::New { class_name, .. } if class_name == "String"
    ) || matches!(
        &exprs[obj.0 as usize],
        Expr::Ident(n) if strwrapper_recvs.contains(n)
    );
    if !wrapper_recv {
        if !matches!(&exprs[obj.0 as usize], Expr::String(_)) {
            return;
        }
        if !args
            .first()
            .is_some_and(|p| is_str_pattern_literal(exprs, *p))
        {
            return;
        }
    }
    if let Some(cb) = args.get(1) {
        collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
        collect_ident_face(exprs, *cb, ident_cands);
    }
}

/// `true` when the replace-family pattern argument is a shape the
/// str replace_fn lane accepts (mirror of
/// `ssa_lower_str_replace_fn`'s String / Null / Undefined checker
/// gate) — anything else may route to the regex replacer lane,
/// whose dispatch has no receiver slot.
fn is_str_pattern_literal(exprs: &[Expr], p: ExprId) -> bool {
    matches!(&exprs[p.0 as usize], Expr::String(_) | Expr::Null)
        || matches!(&exprs[p.0 as usize], Expr::Ident(n) if n == "undefined")
}

/// Rotation 261 — `f.call(thisArg, ...)` / `f.apply(thisArg,
/// [args])` on a bare-name binding: §20.2.3.3 / §20.2.3.1 supply an
/// EXPLICIT this, so the callee-obj position is receiver-correct by
/// construction — the fn-value lowering boxes thisArg into the
/// promoted `__this` argv slot instead of the no-this drop. The
/// Ident joins `call_faces` too: this use shape replays through
/// `closure_local`, whose variadic / real-argc lanes have no
/// `__this` slot, so those bindings keep the loud reject (same
/// argv-contention bar as the knife-2W mixed profile). An INLINE
/// fn-expr callee (`(function () { …this… }).apply(obj)`) promotes
/// directly — zero aliases by construction, the call is its only
/// consumer.
pub(super) fn collect_call_apply_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
    call_faces: &mut std::collections::HashSet<ExprId>,
) {
    // An `as` wrapper changes nothing about who supplies the
    // receiver: `(f as any).call(o)` is still §20.2.3.3 handing `o`
    // to `f`, and the any-lane method dispatch it lowers through
    // shifts argv on FLAG_CLOSURE_RECV_FIRST exactly like the static
    // lane does. Without the peel the binding saw an unrecognized
    // use shape, declined promotion, and its body's `this` capture
    // then had nothing to bind to ("closure `__closure_N` references
    // unknown identifier `__this`") — while the same call written
    // without the cast worked.
    let obj = super::fnexpr_this_names::peel_as(exprs, obj);
    collect_face(stmts, exprs, obj, fn_expr_exprs, patches);
    if matches!(&exprs[obj.0 as usize], Expr::Ident(_)) {
        collect_ident_face(exprs, obj, ident_cands);
        call_faces.insert(obj);
    }
}

/// Rotation 260 — `Array.from(iterable, <fn-expr>, thisArg?)`: the
/// static mapFn slot. Only the INLINE fn-expr promotes (zero aliases
/// by construction); the from lowering's map loop threads the boxed
/// thisArg ahead of (elem, i) exactly like the trio kernels.
/// `Array.fromAsync` rides the same face (rotation 435): its mapped
/// dyn kernel takes the §2.1.1 step 3.j.ii.6.a receiver as a third
/// operand and hands it to every mapfn call.
pub(super) fn collect_array_from_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    args: &[ExprId],
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Array") {
        return;
    }
    if let Some(cb) = args.get(1) {
        collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
        collect_ident_face(exprs, *cb, ident_cands);
    }
}
