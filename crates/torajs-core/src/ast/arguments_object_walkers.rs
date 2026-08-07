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

use super::{Ast, Expr, ExprId, Stmt};

/// What the shared scan walker below is looking for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanFor {
    /// `arguments.length` member read (T-31 argc seed).
    Length,
    /// Any `arguments` touch OTHER than `arguments.length` (RFC
    /// 20260708-closure-argc-abi length-only classifier): the
    /// `.length` member read is absorbed (its `obj` is not
    /// recursed), so a hit means the body reads `arguments[i]`,
    /// spreads `...arguments`, aliases it, or touches any other
    /// member — all of which need the arg VALUES (the argv face)
    /// and disqualify the runtime-argc-only tier.
    /// RFC 20260801 knife 2 retired the separate EscapingTouch
    /// scan: bare escapes ride the argv face now (the rewriter
    /// swaps them to the materialized `__torajs_arguments`).
    NonLengthTouch,
    /// `arguments.length` in a WRITE position — Assign target or
    /// PostIncr/PostDecr target (pre-incr desugars to Assign in the
    /// parser, so the two arms cover every mutation form). A hit
    /// moves a static-argv-face body from Unmapped to LiveLength:
    /// folding the write target mints a literal ("invalid
    /// assignment target") and every later read would answer the
    /// stale constant.
    LengthWrite,
    /// The bare `arguments` binding itself in a WRITE position
    /// (`arguments = v` / `arguments++`). See
    /// [`body_has_bare_arguments_assign`].
    BareAssign,
}

/// `Ident("arguments")` — the bare-binding shape the BareAssign
/// write-position scan keys on.
fn is_bare_arguments(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Ident(n) if n == "arguments")
}

/// `Member { obj: Ident("arguments"), name: "length" }` — the node
/// shape both write-position scans key on.
fn is_arguments_length(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Member { obj, name }
        if name == "length"
            && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments"))
}

/// True if the body WRITES `arguments.length` anywhere (assignment
/// or post-incr/decr). See [`ScanFor::LengthWrite`].
pub(super) fn body_has_arguments_length_write(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::LengthWrite))
}

/// True if the body ASSIGNS the bare `arguments` binding itself
/// (`arguments = v`, destructuring-default desugar included, or
/// `arguments++`). Such a fn must leave every swapping face: the
/// materialized `__torajs_arguments` local is const, so the swap
/// turned the assignment into a type error — pre-face these bodies
/// rode the undeclared-ident lane (sloppy auto-global write + read),
/// which is the behavior the for-await-of dstr tests observe.
pub(super) fn body_has_bare_arguments_assign(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::BareAssign))
}

/// The face-admission exclusion set: the shadowed fns plus every
/// bare-assign body (see [`body_has_bare_arguments_assign`] — those
/// still take the default FoldArity rewrite; only the face
/// admissions are gated, so this set feeds the collectors while
/// `shadowed` alone skips the rewrite loop).
pub(super) fn collect_face_excluded_fns(
    ast: &Ast,
    shadowed: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut excluded = shadowed.clone();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && !excluded.contains(name)
            && body_has_bare_arguments_assign(ast, body)
        {
            excluded.insert(name.clone());
        }
    }
    excluded
}

/// True if the body touches `arguments` in any form other than
/// `arguments.length`.
pub(super) fn body_has_non_length_arguments_touch(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter()
        .any(|s| stmt_scan(ast, s, ScanFor::NonLengthTouch))
}

/// RFC 20260708-closure-argv-face — true if any `return` in the
/// body could hand back an `arguments[i]` elem box through a
/// pass-through chain (ternary arm / nullish side / sequence tail /
/// as / assign value) WITHOUT a consuming node in between: the
/// return-root retain can't see through those, so the box would
/// leave borrowing the materialized array's stake (UAF once the
/// array scope-drops). Such bodies stay KeepLoud. A root
/// `arguments[i]` return is fine (the return lowering retains) and
/// any read under a consuming node (BinOp / call arg / literal /
/// member / index position) produces a fresh result.
pub(super) fn body_has_unsafe_return_arguments(ast: &Ast, body: &[Stmt]) -> bool {
    fn stmt_walk(ast: &Ast, s: &Stmt) -> bool {
        match s {
            Stmt::Return(Some(e)) => {
                // root arguments-index — retained by the return
                // lowering, safe.
                if matches!(ast.get_expr(*e), Expr::Index { obj, .. }
                    if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments"))
                {
                    return false;
                }
                passthrough_aliases(ast, *e)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                stmt_walk(ast, then_branch)
                    || else_branch.as_ref().is_some_and(|e| stmt_walk(ast, e))
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Labeled { body, .. } => stmt_walk(ast, body),
            Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_walk(ast, s)),
            Stmt::Try {
                body, catch_body, ..
            } => {
                body.iter().any(|s| stmt_walk(ast, s))
                    || catch_body.iter().any(|s| stmt_walk(ast, s))
            }
            Stmt::Switch { cases, default, .. } => {
                cases
                    .iter()
                    .any(|c| c.body.iter().any(|s| stmt_walk(ast, s)))
                    || default
                        .as_ref()
                        .is_some_and(|d| d.iter().any(|s| stmt_walk(ast, s)))
            }
            _ => false,
        }
    }
    fn passthrough_aliases(ast: &Ast, e: ExprId) -> bool {
        match ast.get_expr(e) {
            Expr::Index { obj, .. } if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") => {
                true
            }
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => passthrough_aliases(ast, *then_branch) || passthrough_aliases(ast, *else_branch),
            Expr::Nullish { lhs, rhs } => {
                passthrough_aliases(ast, *lhs) || passthrough_aliases(ast, *rhs)
            }
            Expr::Sequence { right, .. } => passthrough_aliases(ast, *right),
            Expr::As { expr, .. } => passthrough_aliases(ast, *expr),
            Expr::Assign { value, .. } => passthrough_aliases(ast, *value),
            _ => false,
        }
    }
    body.iter().any(|s| stmt_walk(ast, s))
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
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::PostIncr { target: expr, .. } => {
            expr_uses_dynamic_arguments(ast, *expr)
        }
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
        Expr::Nullish { lhs, rhs } => {
            expr_uses_dynamic_arguments(ast, *lhs) || expr_uses_dynamic_arguments(ast, *rhs)
        }
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
/// inject the `__torajs_real_argc` synthetic param.
pub(super) fn body_has_arguments_length(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_scan(ast, s, ScanFor::Length))
}

fn stmt_scan(ast: &Ast, s: &Stmt, what: ScanFor) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => expr_scan(ast, *eid, what),
        Stmt::Return(opt) => opt.is_some_and(|e| expr_scan(ast, e, what)),
        Stmt::LetDecl { init, .. } => expr_scan(ast, *init, what),
        Stmt::YieldInto { value, .. } => expr_scan(ast, *value, what),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_scan(ast, *cond, what)
                || stmt_scan(ast, then_branch, what)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_scan(ast, e, what))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_scan(ast, *cond, what) || stmt_scan(ast, body, what)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref().is_some_and(|s| stmt_scan(ast, s, what))
                || cond.is_some_and(|c| expr_scan(ast, c, what))
                || step.is_some_and(|st| expr_scan(ast, st, what))
                || stmt_scan(ast, body, what)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_scan(ast, *parent, what)
                || expr_scan(ast, *sep, what)
                || stmt_scan(ast, body, what)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_scan(ast, *elem_expr, what) || stmt_scan(ast, body, what),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_scan(ast, s, what)),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_scan(ast, s, what))
                || catch_body.iter().any(|s| stmt_scan(ast, s, what))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_scan(ast, s, what)))
        }
        // Nested FnDecl is an independent scope; its `arguments`
        // refers to the inner fn, not the outer one we're scanning.
        // desugar_nested_fns lifts these to top-level before us, so
        // this arm is mostly defensive.
        _ => false,
    }
}

fn expr_scan(ast: &Ast, eid: ExprId, what: ScanFor) -> bool {
    match ast.get_expr(eid) {
        Expr::Member { obj, name } if name == "length" => {
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") {
                // `arguments.length` — the Length target hit; the
                // NonLengthTouch scan absorbs it (obj not recursed).
                return what == ScanFor::Length;
            }
            expr_scan(ast, *obj, what)
        }
        Expr::Member { obj, .. } => expr_scan(ast, *obj, what),
        Expr::Index { obj, index } => expr_scan(ast, *obj, what) || expr_scan(ast, *index, what),
        Expr::BinOp { left, right, .. } => {
            expr_scan(ast, *left, what) || expr_scan(ast, *right, what)
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } => expr_scan(ast, *expr, what),
        Expr::PostIncr { target, .. } => {
            if what == ScanFor::LengthWrite && is_arguments_length(ast, *target) {
                return true;
            }
            if what == ScanFor::BareAssign && is_bare_arguments(ast, *target) {
                return true;
            }
            expr_scan(ast, *target, what)
        }
        Expr::Assign { target, value } => {
            if what == ScanFor::LengthWrite && is_arguments_length(ast, *target) {
                return true;
            }
            if what == ScanFor::BareAssign && is_bare_arguments(ast, *target) {
                return true;
            }
            expr_scan(ast, *target, what) || expr_scan(ast, *value, what)
        }
        Expr::Call { callee, args } => {
            expr_scan(ast, *callee, what) || args.iter().any(|a| expr_scan(ast, *a, what))
        }
        Expr::Array(items) => items.iter().any(|e| expr_scan(ast, *e, what)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_scan(ast, *e, what)),
        Expr::Spread { expr } => expr_scan(ast, *expr, what),
        Expr::Ident(n) if n == "arguments" => what == ScanFor::NonLengthTouch,
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_scan(ast, *cond, what)
                || expr_scan(ast, *then_branch, what)
                || expr_scan(ast, *else_branch, what)
        }
        Expr::Nullish { lhs, rhs } => expr_scan(ast, *lhs, what) || expr_scan(ast, *rhs, what),
        Expr::OptChain { obj, .. } => expr_scan(ast, *obj, what),
        Expr::OptIndex { obj, index } => expr_scan(ast, *obj, what) || expr_scan(ast, *index, what),
        Expr::OptCall { callee, args } => {
            expr_scan(ast, *callee, what) || args.iter().any(|a| expr_scan(ast, *a, what))
        }
        _ => false,
    }
}

// The `__torajs_arguments` local builders live in the sibling
// `arguments_object_synth` (split rotation 270 when the BareAssign
// scan pushed this file past the 500-line limit — builders and
// walkers were always two identities sharing one file).
