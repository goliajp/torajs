//! The FoldTo-admission mutation scan for
//! [`super::arguments_object::desugar_arguments_object`] (rotation
//! 272). A static-argv-face body may keep the zero-cost FoldTo
//! rewrite (literal-index param substitution + inline spread
//! expansion) ONLY when that is observationally equivalent to the
//! unmapped arguments object every module-code (strict) fn really
//! has. Equivalence holds exactly when nothing can tell the
//! snapshot from the live view:
//!
//! - no `arguments[...]` write (element write must not reach the
//!   param, and must show in later arguments reads),
//! - no write to any user param (a param write must NOT show in a
//!   later `arguments[i]` read),
//! - no escape of the arguments object itself (a bare `arguments`
//!   value use or a non-length member touch hands out an aliasable
//!   object; writes through the alias are statically invisible).
//!
//! Any hit routes the body to ArgcMode::Unmapped, where every index
//! rides the materialized `__torajs_arguments` array.
//!
//! The walk matches every statement and expression kind with no
//! catch-all arm: the rewrites substitute parameters inside every
//! node, so a node this scan skipped would be rewritten unexamined.
//! A kind whose contents the scan cannot reason about (a nested
//! declaration or class, a binding that may shadow a parameter)
//! answers "hit".

use super::{Ast, Expr, ExprId, Stmt};

/// Which question the shared walk below is answering.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MutScan {
    /// The FoldTo-admission scan (module doc): ANY mutation /
    /// escape / delete routes the body off the snapshot.
    View,
    /// The S3 Mapped-admission scan: param and in-range literal
    /// element writes are the POINT (they alias); only the shapes
    /// that need the materialized array disqualify. The count is the
    /// call site's argc: a position at or past it is not mapped.
    MappedBlocker(usize),
    /// The same question for an argv body ([`super::arguments_object::ArgcMode::MappedArgc`]):
    /// a `...arguments` spread also disqualifies, since it reads the
    /// materialized array, whose mapped slots the parameters own.
    MappedArgvBlocker,
    /// Does this subtree touch `arguments` at all — asked of the
    /// scope of a binding that shadows a parameter, where a
    /// substituted `arguments[k]` would read the inner binding.
    Touch,
}

/// True if the body can observe the difference between the FoldTo
/// snapshot and the real unmapped arguments object — see the module
/// doc for the three trigger classes.
pub(super) fn body_mutates_args_view(ast: &Ast, body: &[Stmt], params: &[String]) -> bool {
    stmts_walk(ast, body, params, MutScan::View)
}

/// RFC 20260810-sloppy-goal-arguments S3 — what disqualifies a
/// sloppy simple-param static-face body called with `argc` arguments
/// from the Mapped rewrite (the literal-index param substitution,
/// whose two-way aliasing IS §10.4.4 CreateMappedArgumentsObject's
/// semantics): an escape, a non-length member touch, an element
/// delete, or a dynamic / unmapped index — those need the
/// materialized array, and a mapped body must never mix the two
/// views (a dynamic write would be invisible to a substituted
/// literal read).
pub(super) fn body_has_mapped_blockers(
    ast: &Ast,
    body: &[Stmt],
    params: &[String],
    argc: usize,
) -> bool {
    stmts_walk(ast, body, params, MutScan::MappedBlocker(argc))
}

/// Whether an argv body keeps the sloppy goal's mapped arguments
/// object (see `ArgcMode::MappedArgc`): a sloppy body with simple
/// parameters that touches `arguments` only through in-range literal
/// indices and `.length`.
pub(super) fn argv_body_is_mapped(
    ast: &Ast,
    body: &[Stmt],
    decl_params: &[super::Param],
    params: &[String],
) -> bool {
    ast.body_is_sloppy(body)
        && decl_params
            .iter()
            .all(|p| p.default.is_none() && !p.is_rest && !p.name.starts_with("__param_destr_"))
        && !stmts_walk(ast, body, params, MutScan::MappedArgvBlocker)
}

fn is_arguments_ident(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Ident(n) if n == "arguments")
}

fn is_write_target_hit(ast: &Ast, target: ExprId, params: &[String], what: MutScan) -> bool {
    match ast.get_expr(target) {
        // `arguments[i] = v` / `arguments[i]++` — element write.
        // Mapped ALLOWS the in-range literal spelling (the
        // substitution turns it into the aliasing param write);
        // a dynamic or out-of-range index still disqualifies.
        Expr::Index { obj, index } => {
            is_arguments_ident(ast, *obj)
                && (matches!(what, MutScan::View | MutScan::Touch)
                    || !is_mapped_literal_index(ast, *index, params, what))
        }
        // `p = v` / `p++` on a user param — the aliasing IS the
        // Mapped semantics.
        Expr::Ident(n) => what == MutScan::View && is_param(n, params),
        // A destructuring target binds names inside the pattern: a
        // parameter there is a parameter write, and an `arguments[k]`
        // there a write the mapped rewrites do not take apart.
        Expr::Array(_) | Expr::ObjectLit { .. } => subtree_has_ident(ast, target, &|n| {
            n == "arguments" || (what == MutScan::View && is_param(n, params))
        }),
        _ => false,
    }
}

/// An integer Number literal the substitution arm covers. The bound
/// mirrors each rewrite: the argv face guards every position with
/// the hidden argc, the static face substitutes only the positions
/// its call site passed (an unpassed one is unmapped storage).
fn is_mapped_literal_index(ast: &Ast, index: ExprId, params: &[String], what: MutScan) -> bool {
    let bound = match what {
        MutScan::MappedBlocker(argc) => argc.min(params.len()),
        MutScan::View | MutScan::MappedArgvBlocker | MutScan::Touch => params.len(),
    };
    matches!(ast.get_expr(index), Expr::Number(n)
        if n.fract() == 0.0 && *n >= 0.0 && (*n as usize) < bound)
}

fn is_param(name: &str, params: &[String]) -> bool {
    params.iter().any(|p| p == name)
}

/// A binding over a parameter's name scoped to `scope`: hit when the
/// scope touches `arguments`, since a substituted `arguments[k]`
/// there would name the inner binding.
fn shadow_hit(ast: &Ast, name: &str, scope: &[&Stmt], params: &[String], what: MutScan) -> bool {
    what != MutScan::Touch
        && is_param(name, params)
        && scope
            .iter()
            .any(|s| stmt_walk(ast, s, params, MutScan::Touch))
}

/// The lexical names a statement of a block declares (a
/// multi-declaration lists several).
fn lexical_names<'a>(s: &'a Stmt, out: &mut Vec<&'a str>) {
    match s {
        Stmt::LetDecl {
            name,
            is_var: false,
            ..
        }
        | Stmt::UsingDecl { name, .. } => out.push(name),
        Stmt::Multi(v) => v.iter().for_each(|s| lexical_names(s, out)),
        _ => {}
    }
}

fn stmts_walk(ast: &Ast, body: &[Stmt], params: &[String], what: MutScan) -> bool {
    let mut names = Vec::new();
    body.iter().for_each(|s| lexical_names(s, &mut names));
    let scope: Vec<&Stmt> = body.iter().collect();
    names
        .iter()
        .any(|n| shadow_hit(ast, n, &scope, params, what))
        || body.iter().any(|s| stmt_walk(ast, s, params, what))
}

fn stmt_walk(ast: &Ast, s: &Stmt, params: &[String], what: MutScan) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => expr_walk(ast, *eid, params, what),
        Stmt::Return(opt) => opt.is_some_and(|e| expr_walk(ast, e, params, what)),
        // A redeclared `var` of a parameter is the parameter; its
        // initializer writes it. A lexical one is scoped to the
        // enclosing statement list (see `stmts_walk`).
        Stmt::LetDecl {
            name, init, is_var, ..
        } => {
            name == "arguments"
                || (*is_var && what == MutScan::View && is_param(name, params))
                || expr_walk(ast, *init, params, what)
        }
        Stmt::UsingDecl { name, init, .. } => {
            name == "arguments" || expr_walk(ast, *init, params, what)
        }
        Stmt::YieldInto { var, value, .. } => {
            var == "arguments"
                || (what == MutScan::View && is_param(var, params))
                || expr_walk(ast, *value, params, what)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_walk(ast, *cond, params, what)
                || stmt_walk(ast, then_branch, params, what)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_walk(ast, e, params, what))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_walk(ast, *cond, params, what) || stmt_walk(ast, body, params, what)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            let mut names = Vec::new();
            if let Some(i) = init {
                lexical_names(i, &mut names);
            }
            names.iter().any(|n| shadow_hit(ast, n, &[s], params, what))
                || init
                    .as_ref()
                    .is_some_and(|s| stmt_walk(ast, s, params, what))
                || cond.is_some_and(|c| expr_walk(ast, c, params, what))
                || step.is_some_and(|st| expr_walk(ast, st, params, what))
                || stmt_walk(ast, body, params, what)
        }
        // The loop variable either writes a same-named parameter
        // (`for (a of xs)`) or shadows it (`for (let a of xs)`).
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => {
            loop_var_hit(ast, var_name, body, params, what)
                || expr_walk(ast, *parent, params, what)
                || expr_walk(ast, *sep, params, what)
                || stmt_walk(ast, body, params, what)
        }
        Stmt::ForOf {
            var_name,
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            loop_var_hit(ast, var_name, body, params, what)
                || expr_walk(ast, *elem_expr, params, what)
                || forin_obj.is_some_and(|o| expr_walk(ast, o, params, what))
                || stmt_walk(ast, body, params, what)
        }
        Stmt::Labeled { body, .. } => stmt_walk(ast, body, params, what),
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            expr_walk(ast, *scrutinee, params, what)
                || cases.iter().any(|c| {
                    expr_walk(ast, c.value, params, what) || stmts_walk(ast, &c.body, params, what)
                })
                || default
                    .as_ref()
                    .is_some_and(|d| stmts_walk(ast, d, params, what))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts_walk(ast, stmts, params, what),
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            catch_param.as_ref().is_some_and(|c| {
                c == "arguments"
                    || shadow_hit(ast, c, &catch_body.iter().collect::<Vec<_>>(), params, what)
            }) || stmts_walk(ast, body, params, what)
                || stmts_walk(ast, catch_body, params, what)
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| stmts_walk(ast, fb, params, what))
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => {
            false
        }
        // A nested declaration is lifted out before this pass; one
        // still here may read the parameters or `arguments` itself.
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } | Stmt::ExportDecl { .. } => true,
    }
}

/// A for-of variable named like a parameter either writes it (a
/// mutation the strict snapshot cannot show) or shadows it in the
/// loop body.
fn loop_var_hit(ast: &Ast, var: &str, body: &Stmt, params: &[String], what: MutScan) -> bool {
    var == "arguments"
        || (what == MutScan::View && is_param(var, params))
        || shadow_hit(ast, var, &[body], params, what)
}

fn expr_walk(ast: &Ast, eid: ExprId, params: &[String], what: MutScan) -> bool {
    match ast.get_expr(eid) {
        Expr::Assign { target, value } => {
            is_write_target_hit(ast, *target, params, what)
                || expr_walk(ast, *target, params, what)
                || expr_walk(ast, *value, params, what)
        }
        Expr::PostIncr { target, .. } => {
            is_write_target_hit(ast, *target, params, what) || expr_walk(ast, *target, params, what)
        }
        // `delete arguments[i]` — a MUTATION (§10.4.4.6 unmapped
        // elements are plain deletable data properties): the
        // snapshot substitution would turn the target into a bare
        // param name (10.5-7-b-4-s's "must be a property reference"
        // reject). Routes the body to the materialized array.
        Expr::Delete { expr } => {
            matches!(ast.get_expr(*expr), Expr::Index { obj, .. }
                if is_arguments_ident(ast, *obj))
                || matches!(ast.get_expr(*expr), Expr::Member { obj, name }
                    if name == "length" && is_arguments_ident(ast, *obj))
                || expr_walk(ast, *expr, params, what)
        }
        // `arguments[i]` read — absorbed (obj not recursed): the
        // substitution answer is the snapshot value, correct while
        // no write / escape exists elsewhere in the body. Mapped
        // additionally requires the literal in-range spelling — a
        // dynamic read would ride the materialized array, blind to
        // the substituted writes.
        Expr::Index { obj, index } if is_arguments_ident(ast, *obj) => {
            what == MutScan::Touch
                || (what != MutScan::View && !is_mapped_literal_index(ast, *index, params, what))
                || expr_walk(ast, *index, params, what)
        }
        // `arguments.length` — absorbed (folds to the static argc).
        Expr::Member { obj, name } if name == "length" && is_arguments_ident(ast, *obj) => {
            what == MutScan::Touch
        }
        // Any OTHER member touch (`arguments.callee`, …) hands out /
        // observes the object itself — no snapshot equivalence.
        Expr::Member { obj, .. } if is_arguments_ident(ast, *obj) => true,
        // `arguments[k]()` calls with the arguments object as `this`.
        Expr::Call { callee, .. } | Expr::OptCall { callee, .. }
            if matches!(ast.get_expr(*callee), Expr::Index { obj, .. } | Expr::Member { obj, .. }
                if is_arguments_ident(ast, *obj)) =>
        {
            true
        }
        // `...arguments` spread — absorbed: the inline expansion is
        // the snapshot, correct under the same no-write condition. An
        // argv body spreads the materialized array instead.
        Expr::Spread { expr } if is_arguments_ident(ast, *expr) => {
            matches!(what, MutScan::MappedArgvBlocker | MutScan::Touch)
        }
        // A bare `arguments` value use anywhere else is an escape —
        // the alias can be written through invisibly.
        Expr::Ident(n) => n == "arguments",
        // A lifted closure that captures `arguments` holds the object
        // itself; one that captures a parameter can write it, which
        // only the mapped faces' substitution keeps in step.
        Expr::Closure { captures, .. } => captures
            .iter()
            .any(|c| c == "arguments" || (what == MutScan::View && is_param(c, params))),
        // Function expressions are lifted before this pass.
        Expr::ArrowFn { .. } => true,
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::This
        | Expr::NewTarget
        | Expr::Elision => false,
        Expr::BinOp { .. }
        | Expr::Unary { .. }
        | Expr::Member { .. }
        | Expr::Call { .. }
        | Expr::Index { .. }
        | Expr::Array(_)
        | Expr::ObjectLit { .. }
        | Expr::New { .. }
        | Expr::NewDynamic { .. }
        | Expr::Super { .. }
        | Expr::Ternary { .. }
        | Expr::Sequence { .. }
        | Expr::As { .. }
        | Expr::TypeOf { .. }
        | Expr::InstanceOf { .. }
        | Expr::Spread { .. }
        | Expr::Nullish { .. }
        | Expr::OptChain { .. }
        | Expr::OptIndex { .. }
        | Expr::OptCall { .. } => super::desugar_with::walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_walk(ast, c, params, what)),
    }
}

/// Whether an identifier `pred` accepts appears anywhere under `eid`.
fn subtree_has_ident(ast: &Ast, eid: ExprId, pred: &dyn Fn(&str) -> bool) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => pred(n),
        _ => super::desugar_with::walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| subtree_has_ident(ast, c, pred)),
    }
}
