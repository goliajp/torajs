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
//! - On a top-level FnDecl only `any` / unannotated params convert
//!   freely. A scalar-typed or fn-shaped default converts through
//!   the TypedNarrow lane ONLY when it references a prior param —
//!   directly (`h(k: number, m: number = k + 1)`) or as an arrow's
//!   capture (`h2(k: number, cb: (x) => x = (x) => x + k)`): the
//!   call-site pad pasted the expression into the CALLER's scope —
//!   `unknown identifier k` + runtime ReferenceError where §9.2
//!   wants the callee's params environment. Any other typed default
//!   keeps today's pad / `FnDfltLits` channel (the callee is
//!   static; zero behavior movement).
//! - On an ArrowFn VALUE (closure / function expression) EVERY
//!   scalar-typed or fn-shaped default converts through the
//!   TypedNarrow lane: the closure direct call is a CallIndirect
//!   with no static callee, so the pad table cannot fire and a
//!   runtime-undefined `any` argument used to flow through the
//!   any→typed coercion instead of the default (`c(u)` answered NaN
//!   where bun owes 5 — the L3b ① recon shape). The param is renamed
//!   to a `__dflt_<name>` carrier, its slot widened to `any` (so the
//!   entry can SEE the undefined box), and the body head gets the
//!   guard plus `let <name>: <T> = __dflt_<name>` narrowing back to
//!   the declared type — body reads stay typed (the fn-shaped
//!   narrow is the plain any→closure `let` channel).
//! - Literal defaults (`b = 5`) on unannotated params ALSO convert:
//!   §10.2.1.4 fires a default on an explicit `undefined` argument,
//!   which only a callee-side guard can observe (the pad fills
//!   missing positions only). See [`converting_default`].
//! - Guards go at the very head of the body — BEFORE a destructured
//!   param's unpack prefix, which is what §9.2's ordering wants
//!   (the synthetic `__param_destr_N` materializes its default
//!   before the unpack lets read it).

use super::{Ast, BinOp, Expr, ExprId, Stmt};

mod expr_shape;
mod nested;

use expr_shape::{body_safe_default, refs_prior_param};

/// Build one `if (name === undefined) name = <default>;` guard.
pub(super) fn build_default_guard(ast: &mut Ast, name: &str, d: ExprId) -> Stmt {
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
fn converting_default(
    ast: &Ast,
    p: &super::Param,
    global_fns: &[String],
    prior: &[String],
) -> Option<ExprId> {
    let d = guardable_default(ast, p, global_fns, prior)?;
    let ann_ok = p.type_ann.as_deref().is_none_or(|a| a.trim() == "any");
    if !ann_ok {
        return None;
    }
    Some(d)
}

/// The channel-independent half of the conversion gate: a body-safe
/// default, minus the two exclusions [`converting_default`] documents.
pub(super) fn guardable_default(
    ast: &Ast,
    p: &super::Param,
    global_fns: &[String],
    prior: &[String],
) -> Option<ExprId> {
    let d = p.default?;
    if p.name == "__yield_arg" {
        return None;
    }
    if matches!(ast.get_expr(d), Expr::Ident(n) if n == "undefined") {
        return None;
    }
    if !body_safe_default(ast, d, global_fns, prior) {
        return None;
    }
    Some(d)
}

/// TypedNarrow gate (module doc "Guarded faces"): a body-safe
/// default on a scalar-typed or fn-shaped param. Answers the
/// declared annotation so the mutate phase can rebuild the typed
/// local (the any→closure `let` narrow is the same channel a plain
/// `let cb: (x: number) => number = someAny` takes). Every ArrowFn
/// takes this lane; a FnDecl takes it only when the default
/// references a prior param (see [`refs_prior_param`]).
fn converting_typed_default(
    ast: &Ast,
    p: &super::Param,
    global_fns: &[String],
    prior: &[String],
) -> Option<(ExprId, String)> {
    let d = guardable_default(ast, p, global_fns, prior)?;
    let ann = p.type_ann.as_deref()?.trim();
    // Fn-shaped test mirrors `lift_arrow_fns`'s `is_fn_type_ann`.
    let fn_shaped = ann.starts_with("__cls(")
        || ann.starts_with("__fn(")
        || ann.contains("=>")
        || ann.starts_with('(');
    if !matches!(ann, "number" | "string" | "boolean") && !fn_shaped {
        return None;
    }
    Some((d, ann.to_string()))
}

/// Per-param conversion plan, shared by the FnDecl and ArrowFn walks.
enum Conv {
    /// The `any` / unannotated lane: guard only, param keeps its name.
    Any(ExprId),
    /// Scalar-typed lane: rename the param to a `__dflt_<name>` `any`
    /// carrier, guard the carrier, then `let <name>: <ann> = carrier`.
    TypedNarrow(ExprId, String),
}

/// Build one fn's guard block and param patch list from its sorted
/// conversion plan. Guards run in PARAM ORDER with each TypedNarrow
/// param's `let` immediately after its guard, so a later default
/// reading an earlier param (`(x: number = 1, y: number = x)`) sees
/// the NARROWED typed local, per §9.2 ordering. Patch entries are
/// `(param index, pad ExprId, TypedNarrow carrier name)`.
fn build_guards_and_pads(
    ast: &mut Ast,
    conv: Vec<(usize, String, Conv)>,
) -> (Vec<Stmt>, Vec<(usize, ExprId, Option<String>)>) {
    let mut guards: Vec<Stmt> = Vec::new();
    let mut pads: Vec<(usize, ExprId, Option<String>)> = Vec::new();
    for (pi, name, kind) in conv {
        let pad_undef = ast.add_expr(Expr::Ident("undefined".into()));
        match kind {
            Conv::Any(d) => {
                guards.push(build_default_guard(ast, &name, d));
                pads.push((pi, pad_undef, None));
            }
            Conv::TypedNarrow(d, ann) => {
                let carrier = format!("__dflt_{name}");
                guards.push(build_default_guard(ast, &carrier, d));
                let init = ast.add_expr(Expr::Ident(carrier.clone()));
                guards.push(Stmt::LetDecl {
                    mutable: true,
                    name,
                    type_ann: Some(ann),
                    init,
                    is_var: false,
                });
                pads.push((pi, pad_undef, Some(carrier)));
            }
        }
    }
    (guards, pads)
}

/// Apply one patch list to a params slice (shared FnDecl / ArrowFn
/// mutate tail). The param's default becomes the undefined literal —
/// arity bookkeeping and the pad table stay live, the padded value is
/// now scope-free. An unannotated param becomes `any` EXPLICITLY: the
/// conversion's whole premise is that the slot carries the undefined
/// box the pad sends and then whatever the guard assigns — leaving
/// the ann empty let downstream inference type the slot off the
/// padded undefined (Ptr) and reject the body's reads (`x[0]` on Ptr
/// — the cluster-`values` fixture shape). A TypedNarrow patch renames
/// to the carrier and widens the slot to `any` (the body's typed
/// reads come from the spliced `let`).
fn patch_params(params: &mut [super::Param], pads: Vec<(usize, ExprId, Option<String>)>) {
    for (pi, pad, carrier) in pads {
        params[pi].default = Some(pad);
        if let Some(carrier) = carrier {
            params[pi].name = carrier;
            params[pi].type_ann = Some("any".to_string());
        } else if params[pi].type_ann.is_none() {
            params[pi].type_ann = Some("any".to_string());
        }
    }
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
pub(super) fn splice_guards(body: &mut Vec<Stmt>, guards: Vec<Stmt>) {
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
    // conversion plan) for every converting param, immutably. The
    // FnDecl TypedNarrow lane is gated on a prior-param reference —
    // an ordinary typed literal default keeps today's pad channel
    // (zero behavior movement); the ArrowFn lane takes every
    // scalar-typed default (no static callee, no pad).
    let mut sites: Vec<(usize, usize, String, Conv)> = Vec::new();
    for (si, s) in ast.stmts.iter().enumerate() {
        let Stmt::FnDecl { params, .. } = s else {
            continue;
        };
        for (pi, name, kind) in collect_fn_conv(ast, params, &global_fns) {
            sites.push((si, pi, name, kind));
        }
    }
    let mut arrow_sites: Vec<(usize, usize, String, Conv)> = Vec::new();
    for (ei, e) in ast.exprs.iter().enumerate() {
        let Expr::ArrowFn { params, .. } = e else {
            continue;
        };
        for (pi, p) in params.iter().enumerate() {
            let prior: Vec<String> = params[..pi].iter().map(|q| q.name.clone()).collect();
            if let Some(d) = converting_default(ast, p, &global_fns, &prior) {
                arrow_sites.push((ei, pi, p.name.clone(), Conv::Any(d)));
            } else if let Some((d, ann)) = converting_typed_default(ast, p, &global_fns, &prior) {
                arrow_sites.push((ei, pi, p.name.clone(), Conv::TypedNarrow(d, ann)));
            }
        }
    }
    // Phase B — mutate the FnDecls (guard/pad construction shared
    // with the ArrowFn phase; see `build_guards_and_pads` for the
    // param-order contract).
    let mut by_stmt: std::collections::BTreeMap<usize, Vec<(usize, String, Conv)>> =
        std::collections::BTreeMap::new();
    for (si, pi, name, kind) in sites {
        by_stmt.entry(si).or_default().push((pi, name, kind));
    }
    for (si, mut conv) in by_stmt {
        conv.sort_by_key(|(pi, _, _)| *pi);
        let (guards, pads) = build_guards_and_pads(ast, conv);
        let Stmt::FnDecl { params, body, .. } = &mut ast.stmts[si] else {
            unreachable!()
        };
        patch_params(params, pads);
        splice_guards(body, guards);
    }
    // Phase B' — same conversion for arrow-fn / fn-expression values
    // still living in the expr arena (nested arrows included: the
    // arena walk sees every ArrowFn regardless of depth).
    let mut by_expr: std::collections::BTreeMap<usize, Vec<(usize, String, Conv)>> =
        std::collections::BTreeMap::new();
    for (ei, pi, name, kind) in arrow_sites {
        by_expr.entry(ei).or_default().push((pi, name, kind));
    }
    for (ei, mut conv) in by_expr {
        conv.sort_by_key(|(pi, _, _)| *pi);
        let (guards, pads) = build_guards_and_pads(ast, conv);
        let Expr::ArrowFn { params, body, .. } = &mut ast.exprs[ei] else {
            unreachable!()
        };
        patch_params(params, pads);
        splice_guards(body, guards);
    }
    // Phase C (424-04) — nested `function` declarations, in the
    // child module (see its doc for why the spine walk exists).
    nested::materialize_nested(ast, &global_fns);
}

/// One fn's conversion plan (module doc "Guarded faces" — the FnDecl
/// gates), shared by the top-level scan and the nested Phase C.
/// Entries come out in param order.
fn collect_fn_conv(
    ast: &Ast,
    params: &[super::Param],
    global_fns: &[String],
) -> Vec<(usize, String, Conv)> {
    let mut conv: Vec<(usize, String, Conv)> = Vec::new();
    for (pi, p) in params.iter().enumerate() {
        let prior: Vec<String> = params[..pi].iter().map(|q| q.name.clone()).collect();
        if let Some(d) = converting_default(ast, p, global_fns, &prior) {
            conv.push((pi, p.name.clone(), Conv::Any(d)));
        } else if let Some((d, ann)) = converting_typed_default(ast, p, global_fns, &prior)
            && refs_prior_param(ast, params, pi, d, global_fns)
        {
            conv.push((pi, p.name.clone(), Conv::TypedNarrow(d, ann)));
        }
    }
    conv
}
