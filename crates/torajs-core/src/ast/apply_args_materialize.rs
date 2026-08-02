//! Expression-default materialization — RFC 20260729-fn-value-any
//! V2b. Runs right after `desugar_classes` (every function form is a
//! top-level FnDecl by then) and BEFORE `bind_this_param` /
//! `lift_arrow_fns` / the capture passes, so a default moved into
//! the body — an arrow-bearing IIFE included — rides the same
//! pipeline as any other body expression (running later orphaned
//! the moved arrow from the closure lift: `closure capture not in
//! scope`, the gate-caught first cut).
//!
//! ES §9.2 evaluates a parameter default in the CALLEE's scope, in
//! parameter order, whenever the bound argument is undefined. tr's
//! call-site padding cannot express that for an EXPRESSION default:
//! padding `f(x, y = x)`'s call sites with the ExprId of `x` lands
//! the reference in the caller's scope, which either rejects the
//! whole program (`unknown identifier x` — the solo-name shape) or,
//! when the by-NAME method merge bailed on conflicting defaults,
//! silently binds the slot's implicit fill (the t262 dflt-params
//! ref-prior template family — `c.m(9)` answered null where the
//! spec owes 9).
//!
//! The rewrite moves exactly those defaults into the function body:
//!
//! ```text
//! function f(x, y: any = x) { body }
//! →
//! function f(x, y: any = undefined) {
//!   if (y === undefined) y = x;   // callee scope, param order
//!   body
//! }
//! ```
//!
//! The param KEEPS a default (now the `undefined` literal), so the
//! arity/padding machinery is untouched — call sites pad
//! `undefined`, which needs no scope, and the guard fires on it.
//! This is also what makes the runtime bare-call channel correct
//! for free: a detached method invoked through the boxed adapter
//! gets an undefined-filled argv, and the guard materializes the
//! default exactly like a padded site (the S2.38 this-free verdict
//! can stop refusing expression defaults once it learns this — a
//! registered follow-up, not part of this pass).
//!
//! Guarded faces:
//! - Only `any` / unannotated params convert. A TYPED default
//!   (`y: number = x` or `b: number = 5`) keeps today's behavior —
//!   its slot cannot carry the undefined box the pad would send.
//! - Literal defaults (`b = 5`) on unannotated params ALSO convert:
//!   §10.2.1.4 fires a default on an explicit `undefined` argument,
//!   which only a callee-side guard can observe (the pad fills
//!   missing positions only). See [`converting_default`].
//! - Guards go at the very head of the body — BEFORE a destructured
//!   param's unpack prefix, which is what §9.2's ordering wants
//!   (the synthetic `__param_destr_N` materializes its default
//!   before the unpack lets read it).

use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// Whether the default is safe to evaluate inside the callee body —
/// a recursive WHITELIST over expression shapes. A CAPTURING
/// function literal (the IIFE shape `_ = (() => { cc++; })()`)
/// stays on the call-site pad: a closure minted inside a fn body
/// cannot capture a top-level `let` today (pre-existing "closure
/// capture not in scope", gate-caught on the first cut of this
/// pass), while the padded copy sits at the top-level call site
/// where the capture works. A CLOSED function literal (free-var
/// set empty after the top-level-FnDecl / builtin filter — the
/// t262 abrupt template `(function() { throw new Test262Error();
/// }())`) has no capture to go wrong and converts, which is what
/// lets an async fn's throwing default reject instead of throwing
/// synchronously (see [`splice_guards`]). Unknown shapes answer
/// false — keeping today's behavior is always safe.
fn body_safe_default(ast: &Ast, e: ExprId, global_fns: &[String]) -> bool {
    match ast.get_expr(e) {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null | Expr::Ident(_) => true,
        Expr::BinOp { left, right, .. } => {
            body_safe_default(ast, *left, global_fns) && body_safe_default(ast, *right, global_fns)
        }
        Expr::Unary { expr, .. } | Expr::As { expr, .. } => {
            body_safe_default(ast, *expr, global_fns)
        }
        Expr::Member { obj, .. } => body_safe_default(ast, *obj, global_fns),
        Expr::Index { obj, index } => {
            body_safe_default(ast, *obj, global_fns) && body_safe_default(ast, *index, global_fns)
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            body_safe_default(ast, *cond, global_fns)
                && body_safe_default(ast, *then_branch, global_fns)
                && body_safe_default(ast, *else_branch, global_fns)
        }
        Expr::Sequence { left, right } => {
            body_safe_default(ast, *left, global_fns) && body_safe_default(ast, *right, global_fns)
        }
        Expr::Call { callee, args } => {
            body_safe_default(ast, *callee, global_fns)
                && args.iter().all(|a| body_safe_default(ast, *a, global_fns))
        }
        Expr::Array(elems) => elems.iter().all(|a| body_safe_default(ast, *a, global_fns)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .all(|(_, v)| body_safe_default(ast, *v, global_fns)),
        Expr::ArrowFn { params, body, .. } => {
            super::free_vars::free_vars_of_arrow(ast, params, body, global_fns).is_empty()
        }
        _ => false,
    }
}

/// Build one `if (name === undefined) name = <default>;` guard.
fn build_default_guard(ast: &mut Ast, name: &str, d: ExprId) -> Stmt {
    let undef = ast.add_expr(Expr::Ident("undefined".into()));
    let name_ref = ast.add_expr(Expr::Ident(name.to_string()));
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: name_ref,
        right: undef,
    });
    let target = ast.add_expr(Expr::Ident(name.to_string()));
    let assign = ast.add_expr(Expr::Assign { target, value: d });
    Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Expr(assign)),
        else_branch: None,
    }
}

/// Shared per-param conversion gate: a body-safe default on an
/// `any` / unannotated param. Literal defaults convert too —
/// §10.2.1.4 fires a default on an EXPLICIT `undefined` argument,
/// which the call-site pad can never see (it only fills MISSING
/// positions), so the IsUndefined test must live in the callee as a
/// guard. Two exclusions:
/// - `__yield_arg`: the generator desugar's resume slot keeps its
///   shape-uniform `Number(0)` default across lanes — converting
///   only the any-lane copy desyncs `merge_method_defaults`'
///   `same_literal` check and evicts "next" from the pad table
///   (every `it.next()` then binds the implicit fill: silent
///   wrong, the rotation-275 ref-prior failure shape).
/// - the `undefined` literal: it is this pass's own pad value — a
///   guard assigning `undefined` on undefined is a no-op.
fn converting_default(ast: &Ast, p: &super::Param, global_fns: &[String]) -> Option<ExprId> {
    let d = p.default?;
    if p.name == "__yield_arg" {
        return None;
    }
    if matches!(ast.get_expr(d), Expr::Ident(n) if n == "undefined") {
        return None;
    }
    if !body_safe_default(ast, d, global_fns) {
        return None;
    }
    let ann_ok = p.type_ann.as_deref().is_none_or(|a| a.trim() == "any");
    if !ann_ok {
        return None;
    }
    Some(d)
}

/// Splice the guard block into a function body. §15.8.4
/// EvaluateAsyncFunctionBody steps 2-3: an abrupt completion from
/// parameter instantiation REJECTS the async fn's promise instead
/// of propagating synchronously. The async desugar wraps a body as
/// exactly one `try { ... } catch (__async_err: any) { return
/// Promise.reject(__async_err); }` (`build_async_body`, same shape
/// for async class methods and async fn values), so guards for such
/// a body go INSIDE that try; every other body takes the head
/// splice. (Generators — sync AND async — are unaffected: their
/// factory body is never async-wrapped, and instantiation abrupt
/// completions there propagate synchronously per §27.3/§27.6.)
fn splice_guards(body: &mut Vec<Stmt>, guards: Vec<Stmt>) {
    let into_try = matches!(
        &body[..],
        [Stmt::Try {
            catch_param: Some(p),
            ..
        }] if p == "__async_err"
    );
    if into_try {
        let Stmt::Try { body: tb, .. } = &mut body[0] else {
            unreachable!()
        };
        tb.splice(0..0, guards);
    } else {
        body.splice(0..0, guards);
    }
}

/// See module doc. Walks every top-level FnDecl (all function forms
/// are FnDecls by this point in the pass pipeline — class methods
/// are `__cm_*`, lifted closures `__closure_*`, generator factories
/// their own names) AND every `Expr::ArrowFn` still in the arena:
/// arrow-fn / function-expression VALUES only become FnDecls at
/// `lift_arrow_fns`, which runs AFTER this pass — the t262
/// arrow/function-expression dflt-params families were falling
/// through with their defaults pasted into the caller's scope
/// (`unknown identifier`, rotation 276).
pub fn materialize_expr_defaults(ast: &mut Ast) {
    // Top-level FnDecl names — pre-bound in the closed-fn-literal
    // check so a reference to one (the t262 harness `Test262Error`
    // shape) doesn't read as a capture. Same construction as
    // `lift_arrow_fns`.
    let global_fns: Vec<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    // Phase A — collect (stmt index, param index, param name,
    // default ExprId) for every converting param, immutably.
    let mut sites: Vec<(usize, usize, String, ExprId)> = Vec::new();
    for (si, s) in ast.stmts.iter().enumerate() {
        let Stmt::FnDecl { params, .. } = s else {
            continue;
        };
        for (pi, p) in params.iter().enumerate() {
            if let Some(d) = converting_default(ast, p, &global_fns) {
                sites.push((si, pi, p.name.clone(), d));
            }
        }
    }
    let mut arrow_sites: Vec<(usize, usize, String, ExprId)> = Vec::new();
    for (ei, e) in ast.exprs.iter().enumerate() {
        let Expr::ArrowFn { params, .. } = e else {
            continue;
        };
        for (pi, p) in params.iter().enumerate() {
            if let Some(d) = converting_default(ast, p, &global_fns) {
                arrow_sites.push((ei, pi, p.name.clone(), d));
            }
        }
    }
    // Phase B — mutate. Guards are built per fn in PARAM ORDER and
    // spliced as a block at the body head, so a later default can
    // read an earlier materialized param (`(x, y = x, z = y)`).
    let mut by_stmt: std::collections::BTreeMap<usize, Vec<(usize, String, ExprId)>> =
        std::collections::BTreeMap::new();
    for (si, pi, name, d) in sites {
        by_stmt.entry(si).or_default().push((pi, name, d));
    }
    for (si, mut conv) in by_stmt {
        conv.sort_by_key(|(pi, _, _)| *pi);
        let mut guards: Vec<Stmt> = Vec::new();
        for (pi, name, d) in conv {
            guards.push(build_default_guard(ast, &name, d));
            // The param's default becomes the undefined literal —
            // arity bookkeeping and the pad table stay live, the
            // padded value is now scope-free. An unannotated param
            // becomes `any` EXPLICITLY: the conversion's whole
            // premise is that the slot carries the undefined box the
            // pad sends and then whatever the guard assigns — leaving
            // the ann empty let downstream inference type the slot
            // off the padded undefined (Ptr) and reject the body's
            // reads (`x[0]` on Ptr — the cluster-`values` fixture
            // shape).
            let pad_undef = ast.add_expr(Expr::Ident("undefined".into()));
            let Stmt::FnDecl { params, .. } = &mut ast.stmts[si] else {
                unreachable!()
            };
            params[pi].default = Some(pad_undef);
            if params[pi].type_ann.is_none() {
                params[pi].type_ann = Some("any".to_string());
            }
        }
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[si] else {
            unreachable!()
        };
        splice_guards(body, guards);
    }
    // Phase B' — same conversion for arrow-fn / fn-expression values
    // still living in the expr arena (nested arrows included: the
    // arena walk sees every ArrowFn regardless of depth).
    let mut by_expr: std::collections::BTreeMap<usize, Vec<(usize, String, ExprId)>> =
        std::collections::BTreeMap::new();
    for (ei, pi, name, d) in arrow_sites {
        by_expr.entry(ei).or_default().push((pi, name, d));
    }
    for (ei, mut conv) in by_expr {
        conv.sort_by_key(|(pi, _, _)| *pi);
        let mut guards: Vec<Stmt> = Vec::new();
        let mut pads: Vec<(usize, ExprId)> = Vec::new();
        for (pi, name, d) in conv {
            guards.push(build_default_guard(ast, &name, d));
            let pad_undef = ast.add_expr(Expr::Ident("undefined".into()));
            pads.push((pi, pad_undef));
        }
        let Expr::ArrowFn { params, body, .. } = &mut ast.exprs[ei] else {
            unreachable!()
        };
        for (pi, pad) in pads {
            params[pi].default = Some(pad);
            if params[pi].type_ann.is_none() {
                params[pi].type_ann = Some("any".to_string());
            }
        }
        splice_guards(body, guards);
    }
}
