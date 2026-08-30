//! The `arguments` scan walk itself — what a body looks like, as
//! opposed to what is being asked of it.
//!
//! Split from [`super::arguments_object_walkers`] (rotation 537) when
//! that file crossed the 500-line limit. The seam is the one the
//! parent's own shape already had: it holds the *questions*
//! (`body_has_*`, `collect_face_excluded_fns`) — one thin wrapper per
//! [`ScanFor`] variant — while the single walk that answers all of
//! them lives here, with the three node-shape predicates it keys on.
//!
//! Nothing here is reachable except through a `ScanFor`, which is why
//! the enum travels with the walk rather than with the callers that
//! name its variants.

use super::{Ast, Expr, ExprId, Stmt};

/// What the shared scan walker below is looking for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScanFor {
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
    /// [`super::arguments_object_walkers::body_has_bare_arguments_assign`].
    BareAssign,
    /// RFC 20260810-sloppy-goal-arguments S2 — `arguments.callee` in
    /// a WRITE or DELETE position (`arguments.callee = v` / `delete
    /// arguments.callee`). A hit forces materialization under the
    /// sloppy goal so the keyed rewrite always has the bag entry to
    /// land on (S10.6_A3_T3/T4). Strict-goal callers never scan for
    /// this (the thrower arms cover both spellings there).
    CalleeWrite,
    /// S2 — `arguments.callee` in ANY position (read, write, or
    /// delete). A sloppy-goal hit is what makes the pass synthesize
    /// the fn's `__forward_` closure shim: both the Ident-position
    /// rewrite and the mint's defineProperty express the callee
    /// value as `Closure { __forward_<fn>, [] }` (a bare fn Ident in
    /// value position is not a closure-shaped value this late in the
    /// pipeline — typeof answered "object").
    CalleeTouch,
    /// ANY `arguments` spelling in the body, every position —
    /// including the two spots the classifier scans deliberately
    /// leave dark: the `arguments.length` member node itself (Length
    /// only answers it, NonLengthTouch absorbs it) and the inside of
    /// a `delete` (invisible to both, by the arm's own doc). This is
    /// the Unmapped-arm gate's question (rotation 435): a body with
    /// NO spelling must never ride the materialized array (every fn
    /// that reassigned a param paid a never-read prologue — the
    /// gcd1m bench regression), while a body with ANY spelling must
    /// stay eligible — `delete arguments.length` was classified off
    /// the ride by the narrower Length∪NonLengthTouch gate and its
    /// rewrite then read a `__torajs_arguments` that was never
    /// materialized (S10.6_A5_T3 pass regression). Face admissions
    /// keep their own narrower scans untouched.
    AnyTouch,
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

/// `Member { obj: Ident("arguments"), name: "callee" }` — the node
/// shape the CalleeWrite write/delete-position scan keys on.
fn is_arguments_callee(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Member { obj, name }
        if name == "callee"
            && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments"))
}

pub(super) fn stmt_scan(ast: &Ast, s: &Stmt, what: ScanFor) -> bool {
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
                return matches!(what, ScanFor::Length | ScanFor::AnyTouch);
            }
            expr_scan(ast, *obj, what)
        }
        Expr::Member { obj, name }
            if what == ScanFor::CalleeTouch && name == "callee" && is_bare_arguments(ast, *obj) =>
        {
            true
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
            if what == ScanFor::CalleeWrite && is_arguments_callee(ast, *target) {
                return true;
            }
            expr_scan(ast, *target, what) || expr_scan(ast, *value, what)
        }
        // Delete stays invisible to every other scan (the catch-all
        // answered false before this arm existed, and widening it
        // would silently change face admissions keyed on those
        // scans); only the two callee probes look inside.
        Expr::Delete { expr } => {
            matches!(
                what,
                ScanFor::CalleeWrite | ScanFor::CalleeTouch | ScanFor::AnyTouch
            ) && (is_arguments_callee(ast, *expr) || expr_scan(ast, *expr, what))
        }
        Expr::Call { callee, args } => {
            expr_scan(ast, *callee, what) || args.iter().any(|a| expr_scan(ast, *a, what))
        }
        // Constructor arguments are ordinary value positions — the
        // missing arms left `new Boolean(arguments.length === 0)`
        // (the t262 bind-construct thunk idiom) invisible to every
        // scan, so the fn never seeded a face and the raw ident
        // leaked to the checker.
        Expr::New { args, .. } => args.iter().any(|a| expr_scan(ast, *a, what)),
        Expr::NewDynamic { callee, args } => {
            expr_scan(ast, *callee, what) || args.iter().any(|a| expr_scan(ast, *a, what))
        }
        Expr::Array(items) => items.iter().any(|e| expr_scan(ast, *e, what)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_scan(ast, *e, what)),
        Expr::Spread { expr } => expr_scan(ast, *expr, what),
        Expr::Ident(n) if n == "arguments" => {
            matches!(what, ScanFor::NonLengthTouch | ScanFor::AnyTouch)
        }
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
