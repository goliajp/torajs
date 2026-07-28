//! Expression-default materialization — RFC 20260729-fn-value-any
//! V2b, run by [`super::apply_args::apply_default_args`] before it
//! builds its call-site padding tables.
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
//! - Only `any` / unannotated params convert. A TYPED expression
//!   default (`y: number = x`) keeps today's loud reject — its slot
//!   cannot carry the undefined box the pad would send.
//! - Literal defaults (`b = 5`) keep the call-site pad unchanged.
//! - Guards go at the very head of the body — BEFORE a destructured
//!   param's unpack prefix, which is what §9.2's ordering wants
//!   (the synthetic `__param_destr_N` materializes its default
//!   before the unpack lets read it).

use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// A default the call-site pad can carry verbatim: a scope-free
/// literal. Everything else references the callee's environment and
/// must materialize in the body.
fn is_pad_safe_literal(ast: &Ast, d: ExprId) -> bool {
    matches!(
        ast.get_expr(d),
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null
    ) || matches!(ast.get_expr(d), Expr::Ident(n) if n == "undefined")
}

/// See module doc. Walks every top-level FnDecl (all function forms
/// are FnDecls by this point in the pass pipeline — class methods
/// are `__cm_*`, lifted closures `__closure_*`, generator factories
/// their own names).
pub(crate) fn materialize_expr_defaults(ast: &mut Ast) {
    // Phase A — collect (stmt index, param index, param name,
    // default ExprId) for every converting param, immutably.
    let mut sites: Vec<(usize, usize, String, ExprId)> = Vec::new();
    for (si, s) in ast.stmts.iter().enumerate() {
        let Stmt::FnDecl { params, .. } = s else {
            continue;
        };
        for (pi, p) in params.iter().enumerate() {
            let Some(d) = p.default else { continue };
            if is_pad_safe_literal(ast, d) {
                continue;
            }
            let ann_ok = p.type_ann.as_deref().is_none_or(|a| a.trim() == "any");
            if !ann_ok {
                continue;
            }
            sites.push((si, pi, p.name.clone(), d));
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
            let undef = ast.add_expr(Expr::Ident("undefined".into()));
            let name_ref = ast.add_expr(Expr::Ident(name.clone()));
            let cond = ast.add_expr(Expr::BinOp {
                op: BinOp::Eq,
                left: name_ref,
                right: undef,
            });
            let target = ast.add_expr(Expr::Ident(name.clone()));
            let assign = ast.add_expr(Expr::Assign { target, value: d });
            guards.push(Stmt::If {
                cond,
                then_branch: Box::new(Stmt::Expr(assign)),
                else_branch: None,
            });
            // The param's default becomes the undefined literal —
            // arity bookkeeping and the pad table stay live, the
            // padded value is now scope-free.
            let pad_undef = ast.add_expr(Expr::Ident("undefined".into()));
            let Stmt::FnDecl { params, .. } = &mut ast.stmts[si] else {
                unreachable!()
            };
            params[pi].default = Some(pad_undef);
        }
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[si] else {
            unreachable!()
        };
        body.splice(0..0, guards);
    }
}
