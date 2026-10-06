//! T-11 / T-31 walker helpers for
//! [`super::arguments_object::desugar_arguments_object`] — split out
//! as a sibling (rotation 44) when the main pass crossed the 500-line
//! file limit: `stmt/expr_uses_dynamic_arguments` (materialize gate),
//! `body_has_arguments_length` (T-31 argc seed, via the shared
//! `stmt_scan`/`expr_scan` pair),
//! `body_has_non_length_arguments_touch` (RFC 20260708-closure-
//! argc-abi length-only classifier), plus the write-position scans
//! (LengthWrite / BareAssign, rotation 270). The `__torajs_arguments`
//! local builders live in the `arguments_object_synth` sibling.

use super::arguments_object_scan::{ScanFor, stmt_scan};
use super::{Ast, Expr, ExprId, Stmt};

pub(super) use super::arguments_object_walkers_return::body_has_unsafe_return_arguments;

/// True if the body writes or deletes `arguments.callee` anywhere —
/// the sloppy goal's materialization trigger (see
/// [`ScanFor::CalleeWrite`]).
pub(super) fn body_has_callee_write(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::CalleeWrite))
}

/// True if the body touches `arguments.callee` in any position —
/// the sloppy goal's forwarder-synthesis trigger (see
/// [`ScanFor::CalleeTouch`]).
pub(super) fn body_has_callee_touch(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::CalleeTouch))
}

/// True if the body WRITES `arguments.length` anywhere (assignment
/// or post-incr/decr). See [`ScanFor::LengthWrite`].
pub(super) fn body_has_arguments_length_write(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::LengthWrite))
}

/// True if the body ASSIGNS the bare `arguments` binding itself
/// (`arguments = v`, destructuring-default desugar included, or
/// `arguments++`). Such a body materializes on whichever face fits
/// it, receives a writable `any` local (`writable_if_assigned`), and
/// classifies as [`super::arguments_object::ArgcMode::LiveLength`]:
/// after the rebind every touch — `.length` included — is a touch of
/// the new value, so nothing about the body may fold statically.
pub(super) fn body_has_bare_arguments_assign(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::BareAssign))
}

/// The face-admission exclusion set — the shadowed fns (a binding
/// named `arguments` in the body resolves the ident, not the object).
///
/// Until rotation 625 a body with a bare `arguments = v` /
/// `arguments++` was excluded too, on the ground that the
/// materialized `__torajs_arguments` local was const and the swap
/// turned the assignment into a type error. The local a body that
/// assigns receives is writable now (`writable_if_assigned`), so
/// such a body takes whichever face fits it like any other — the
/// static fold with its injected extras, the argv face, the
/// unmapped materialize — and the assignment rebinds that local.
/// Left excluded, `function m(a) { const b = arguments[1];
/// arguments = 7; … }` fell to the declared-params builder and read
/// `b` as undefined.
pub(super) fn collect_face_excluded_fns(
    _ast: &Ast,
    shadowed: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    shadowed.clone()
}

/// True if the body touches `arguments` in any form other than
/// `arguments.length`. `pub(crate)`: the bind desugar's
/// kernel-lane routing (RFC 20260808 knife 2) consults the same
/// scan the collectors seed from.
pub(crate) fn body_has_non_length_arguments_touch(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter()
        .any(|s| stmt_scan(ast, s, ScanFor::NonLengthTouch))
}

/// True if the body spells `arguments` anywhere at all — see
/// [`ScanFor::AnyTouch`] for why neither Length nor NonLengthTouch
/// alone can answer this (the `.length` member and the inside of a
/// `delete` each fall dark in one of them).
pub(crate) fn body_has_any_arguments_touch(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::AnyTouch))
}

/// Whether the top-level declaration `name` reads its `arguments`
/// object at all — length, index, or whole-object escape. Such a
/// declaration's definition carries the hidden argc (and argv) slots
/// of its tier, which its FnSig value's signature does not show.
pub(crate) fn fn_decl_reads_arguments(ast: &Ast, name: &str) -> bool {
    ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name: n, body, .. }
            if n == name && body_has_any_arguments_touch(ast, body))
    })
}

pub(super) fn stmt_uses_dynamic_arguments(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            expr_uses_dynamic_arguments(ast, *eid)
        }
        Stmt::Return(opt) => opt.is_some_and(|e| expr_uses_dynamic_arguments(ast, e)),
        Stmt::LetDecl { init, .. } => expr_uses_dynamic_arguments(ast, *init),
        Stmt::YieldInto { value, .. } => expr_uses_dynamic_arguments(ast, *value),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_uses_dynamic_arguments(ast, *cond)
                || stmt_uses_dynamic_arguments(ast, then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_uses_dynamic_arguments(ast, e))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_uses_dynamic_arguments(ast, *cond) || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref()
                .is_some_and(|s| stmt_uses_dynamic_arguments(ast, s))
                || cond.is_some_and(|c| expr_uses_dynamic_arguments(ast, c))
                || step.is_some_and(|st| expr_uses_dynamic_arguments(ast, st))
                || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_uses_dynamic_arguments(ast, *parent)
                || expr_uses_dynamic_arguments(ast, *sep)
                || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_uses_dynamic_arguments(ast, *elem_expr) || stmt_uses_dynamic_arguments(ast, body),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_uses_dynamic_arguments(ast, s))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_uses_dynamic_arguments(ast, s))
                || catch_body
                    .iter()
                    .any(|s| stmt_uses_dynamic_arguments(ast, s))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_uses_dynamic_arguments(ast, s)))
        }
        _ => false,
    }
}

pub(super) fn expr_uses_dynamic_arguments(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Index { obj, index } => {
            // Match `arguments[<non-Number-literal>]`. Number-literal
            // case is already handled inline by the existing rewrite
            // (param-name substitution; no array materialization).
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") {
                if !matches!(ast.get_expr(*index), Expr::Number(_)) {
                    return true;
                }
                // Number index but out-of-range fall-through still
                // materializes — bun returns undefined; tr maps to
                // null in the boxed Any read. Conservative: treat as
                // dynamic so the array is available.
                if let Expr::Number(n) = ast.get_expr(*index)
                    && (n.fract() != 0.0 || (*n as usize) >= count_user_params(ast, eid))
                {
                    return true;
                }
            }
            expr_uses_dynamic_arguments(ast, *obj) || expr_uses_dynamic_arguments(ast, *index)
        }
        Expr::Member { obj, name } => {
            // `arguments.callee` — currently unhandled; will need its
            // own materialization later. Bare `arguments.<other>`
            // also forces materialize so stuff like
            // `arguments.length.toString()` keeps walking.
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") && name != "length"
            {
                return true;
            }
            expr_uses_dynamic_arguments(ast, *obj)
        }
        Expr::Ident(n) if n == "arguments" => {
            // Bare `arguments` reference (not Index / Member / spread —
            // those have their own arms). E.g. `let xs = arguments;`
            // or passing `arguments` to a fn that's not the spread
            // form. Forces materialize.
            true
        }
        Expr::Call { callee, args } => {
            expr_uses_dynamic_arguments(ast, *callee)
                || args.iter().any(|a| {
                    // `f(...arguments)` is handled by the inline-spread
                    // rewrite — no materialize needed.
                    if let Expr::Spread { expr } = ast.get_expr(*a)
                        && let Expr::Ident(n) = ast.get_expr(*expr)
                        && n == "arguments"
                    {
                        return false;
                    }
                    expr_uses_dynamic_arguments(ast, *a)
                })
        }
        Expr::BinOp { left, right, .. } => {
            expr_uses_dynamic_arguments(ast, *left) || expr_uses_dynamic_arguments(ast, *right)
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::As { expr, .. }
        | Expr::PostIncr { target: expr, .. } => expr_uses_dynamic_arguments(ast, *expr),
        Expr::Assign { target, value } => {
            expr_uses_dynamic_arguments(ast, *target) || expr_uses_dynamic_arguments(ast, *value)
        }
        Expr::Array(items) => items.iter().any(|e| {
            // `[...arguments]` — handled inline by spread rewrite.
            if let Expr::Spread { expr } = ast.get_expr(*e)
                && let Expr::Ident(n) = ast.get_expr(*expr)
                && n == "arguments"
            {
                return false;
            }
            expr_uses_dynamic_arguments(ast, *e)
        }),
        Expr::ObjectLit { fields } => fields
            .iter()
            .any(|(_, e)| expr_uses_dynamic_arguments(ast, *e)),
        // No inline-spread rewrite exists for constructor args, so a
        // `...arguments` here recurses to the bare-ident arm and
        // conservatively materializes — the safe direction.
        Expr::New { args, .. } => args.iter().any(|a| expr_uses_dynamic_arguments(ast, *a)),
        Expr::NewDynamic { callee, args } => {
            expr_uses_dynamic_arguments(ast, *callee)
                || args.iter().any(|a| expr_uses_dynamic_arguments(ast, *a))
        }
        Expr::Spread { expr } => expr_uses_dynamic_arguments(ast, *expr),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_uses_dynamic_arguments(ast, *cond)
                || expr_uses_dynamic_arguments(ast, *then_branch)
                || expr_uses_dynamic_arguments(ast, *else_branch)
        }
        Expr::Nullish { lhs, rhs }
        | Expr::Sequence {
            left: lhs,
            right: rhs,
        } => expr_uses_dynamic_arguments(ast, *lhs) || expr_uses_dynamic_arguments(ast, *rhs),
        Expr::OptChain { obj, .. } => expr_uses_dynamic_arguments(ast, *obj),
        Expr::OptIndex { obj, index } => {
            expr_uses_dynamic_arguments(ast, *obj) || expr_uses_dynamic_arguments(ast, *index)
        }
        Expr::OptCall { callee, args } => {
            expr_uses_dynamic_arguments(ast, *callee)
                || args.iter().any(|a| expr_uses_dynamic_arguments(ast, *a))
        }
        _ => false,
    }
}

pub(super) fn count_user_params(_ast: &Ast, _eid: ExprId) -> usize {
    // Caller's params count is captured during the FnDecl walk and
    // not threaded through expr_uses_dynamic_arguments today; default
    // to a large value so the literal-bounds-check arm never trips.
    // The bounds-aware materialize is a follow-up.
    usize::MAX
}

/// T-31 — returns true if the fn body references `arguments.length`
/// (i.e. an `Expr::Member { obj: Ident("arguments"), name: "length" }`)
/// anywhere. Used by `desugar_arguments_object` to decide whether to
/// inject the `argv-face` synthetic param, and by the bind
/// desugar's kernel-lane routing (RFC 20260808 knife 2).
pub(crate) fn body_has_arguments_length(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::Length))
}

/// Receiver eids of every `<ident>.length` / `<ident>.name` member
/// access (a read, or the target of a write / delete). Both answer the fn object's own data properties (§10.2.10
/// SetFunctionLength, §10.2.9 SetFunctionName) and never enter its
/// body, so a binding read this way still has every call site in
/// view: the reference walks that admit an arguments face treat these
/// receivers as non-calling uses rather than as escapes.
pub(super) fn fn_meta_read_receivers(ast: &Ast) -> std::collections::HashSet<u32> {
    ast.exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Member { obj, name }
                if (name == "length" || name == "name")
                    && matches!(ast.get_expr(*obj), Expr::Ident(_)) =>
            {
                Some(obj.0)
            }
            _ => None,
        })
        .collect()
}

/// Every arena slot where a by-name use of `name` reads a fn PARAM of
/// that name rather than the top-level binding: the body of each fn
/// declaring the param, plus — to a fixpoint — the body of every
/// lifted closure minted inside such a region that captures `name`
/// (its env slot holds the param). A closure minted OUTSIDE every
/// such region captures the top-level binding, so its body's uses stay
/// references to it. Body-`let` shadows are not resolved here; the
/// callers keep refusing those by name.
pub(super) fn param_shadowed_eids(ast: &Ast, name: &str) -> Vec<bool> {
    let mut owned = vec![false; ast.exprs.len()];
    let mut marked: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name: f,
            params,
            body,
            ..
        } = s
            && params.iter().any(|p| p.name == name)
        {
            marked.insert(f.as_str());
            super::desugar_eval::body_owned_exprs(ast, body, &mut owned);
        }
    }
    if marked.is_empty() {
        return owned;
    }
    loop {
        let inner: Vec<&str> = ast
            .exprs
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Expr::Closure { fn_name, captures }
                    if owned[i]
                        && !marked.contains(fn_name.as_str())
                        && captures.iter().any(|c| c == name) =>
                {
                    Some(fn_name.as_str())
                }
                _ => None,
            })
            .collect();
        if inner.is_empty() {
            return owned;
        }
        for f in inner {
            marked.insert(f);
            for s in &ast.stmts {
                if let Stmt::FnDecl { name: n, body, .. } = s
                    && n == f
                {
                    super::desugar_eval::body_owned_exprs(ast, body, &mut owned);
                }
            }
        }
    }
}
