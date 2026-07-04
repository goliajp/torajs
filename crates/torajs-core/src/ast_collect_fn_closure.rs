//! `collect_fn_to_closure_store_sites` extracted from
//! [`crate::ast`] (chunk 142).
//!
//! Pre-extract this was a 205 LOC private fn in ast.rs (over the
//! 200-line god-fn hard limit per `torajs-file-size-debt`). Body
//! verbatim moved here; ast.rs keeps a `pub(crate)` re-export
//! shim so the 5 internal callsites stay source-compatible.
//!
//! The pre-typecheck pass walks statements recursively looking
//! for two store-site shapes that turn a bare top-level FnDecl
//! ident into a tagged Closure value:
//!
//! - `let x: <struct-with-Closure-field> = { ..., f: name, ... }` —
//!   wraps `name` (a top-FnDecl Ident) into a `Closure(name, [])`
//!   tagged value because the destination field is tagged-Any.
//! - Any other `Call` / nested ObjectLit / Return expr is walked
//!   via the paired `collect_expr_fn_to_closure` helper.
//!
//! Plain `let f: (n)=>num = name` stays FnSig → FnSig (direct
//! dispatch preserved); the wrap only fires when the destination
//! is tagged Any.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt, is_fn_like_ann};

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_fn_to_closure_store_sites(
    ast: &Ast,
    s: &Stmt,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>)>,
    struct_field_anns: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
    targets: &mut std::collections::HashSet<String>,
    rewrites: &mut Vec<(ExprId, String)>,
) {
    match s {
        Stmt::LetDecl { type_ann, init, .. } => {
            collect_objectlit_fn_to_closure(
                ast,
                *init,
                type_ann.as_deref(),
                fn_sigs,
                struct_field_anns,
                targets,
                rewrites,
            );
            collect_any_let_fn_to_closure(
                ast,
                *init,
                type_ann.as_deref(),
                fn_sigs,
                targets,
                rewrites,
            );
            collect_expr_fn_to_closure(ast, *init, fn_sigs, targets, rewrites);
        }
        Stmt::FnDecl { body, .. } => {
            for inner in body {
                collect_fn_to_closure_store_sites(
                    ast,
                    inner,
                    fn_sigs,
                    struct_field_anns,
                    targets,
                    rewrites,
                );
            }
        }
        Stmt::Expr(eid) => {
            collect_expr_fn_to_closure(ast, *eid, fn_sigs, targets, rewrites);
        }
        Stmt::Return(Some(eid)) => {
            collect_expr_fn_to_closure(ast, *eid, fn_sigs, targets, rewrites);
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_fn_to_closure(ast, *cond, fn_sigs, targets, rewrites);
            collect_fn_to_closure_store_sites(
                ast,
                then_branch,
                fn_sigs,
                struct_field_anns,
                targets,
                rewrites,
            );
            if let Some(eb) = else_branch {
                collect_fn_to_closure_store_sites(
                    ast,
                    eb,
                    fn_sigs,
                    struct_field_anns,
                    targets,
                    rewrites,
                );
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            collect_expr_fn_to_closure(ast, *cond, fn_sigs, targets, rewrites);
            collect_fn_to_closure_store_sites(
                ast,
                body,
                fn_sigs,
                struct_field_anns,
                targets,
                rewrites,
            );
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(init) = init {
                collect_fn_to_closure_store_sites(
                    ast,
                    init,
                    fn_sigs,
                    struct_field_anns,
                    targets,
                    rewrites,
                );
            }
            if let Some(c) = cond {
                collect_expr_fn_to_closure(ast, *c, fn_sigs, targets, rewrites);
            }
            if let Some(stp) = step {
                collect_expr_fn_to_closure(ast, *stp, fn_sigs, targets, rewrites);
            }
            collect_fn_to_closure_store_sites(
                ast,
                body,
                fn_sigs,
                struct_field_anns,
                targets,
                rewrites,
            );
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for inner in stmts {
                collect_fn_to_closure_store_sites(
                    ast,
                    inner,
                    fn_sigs,
                    struct_field_anns,
                    targets,
                    rewrites,
                );
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            collect_expr_fn_to_closure(ast, *scrutinee, fn_sigs, targets, rewrites);
            for c in cases {
                for inner in &c.body {
                    collect_fn_to_closure_store_sites(
                        ast,
                        inner,
                        fn_sigs,
                        struct_field_anns,
                        targets,
                        rewrites,
                    );
                }
            }
            if let Some(d) = default {
                for inner in d {
                    collect_fn_to_closure_store_sites(
                        ast,
                        inner,
                        fn_sigs,
                        struct_field_anns,
                        targets,
                        rewrites,
                    );
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for inner in body {
                collect_fn_to_closure_store_sites(
                    ast,
                    inner,
                    fn_sigs,
                    struct_field_anns,
                    targets,
                    rewrites,
                );
            }
            for inner in catch_body {
                collect_fn_to_closure_store_sites(
                    ast,
                    inner,
                    fn_sigs,
                    struct_field_anns,
                    targets,
                    rewrites,
                );
            }
            if let Some(fb) = finally_body {
                for inner in fb {
                    collect_fn_to_closure_store_sites(
                        ast,
                        inner,
                        fn_sigs,
                        struct_field_anns,
                        targets,
                        rewrites,
                    );
                }
            }
        }
        Stmt::Throw(eid) | Stmt::Yield(eid) => {
            collect_expr_fn_to_closure(ast, *eid, fn_sigs, targets, rewrites);
        }
        _ => {}
    }
}

/// Walk one Stmt (and any Stmts / Exprs it contains) looking for
/// store-sites where a bare top-level FnDecl reference appears in a
/// Closure-typed position. Mutates `targets` / `rewrites` in place.

/// Walk an Expr looking for nested store-sites (Call args, nested
/// ObjectLits not directly under a typed LetDecl, etc.).
pub(crate) fn collect_expr_fn_to_closure(
    ast: &Ast,
    eid: ExprId,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>)>,
    targets: &mut std::collections::HashSet<String>,
    rewrites: &mut Vec<(ExprId, String)>,
) {
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            // Call args are FnSig-typed (parse_type maps `__fn` →
            // FnSig); bare top-FnDecl Idents passed as callbacks
            // stay raw FnSig values, matching the FnSig param ABI
            // directly. We only need to recurse for nested ObjectLits
            // (e.g. `f({k: top_fn})` inside a typed param position
            // would still need the ObjectLit-field arm).
            collect_expr_fn_to_closure(ast, *callee, fn_sigs, targets, rewrites);
            for arg in args {
                collect_expr_fn_to_closure(ast, *arg, fn_sigs, targets, rewrites);
            }
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_expr_fn_to_closure(ast, *obj, fn_sigs, targets, rewrites);
        }
        Expr::Index { obj, index } => {
            collect_expr_fn_to_closure(ast, *obj, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *index, fn_sigs, targets, rewrites);
        }
        Expr::Assign { target, value } => {
            collect_expr_fn_to_closure(ast, *target, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *value, fn_sigs, targets, rewrites);
        }
        Expr::BinOp { left, right, .. } => {
            collect_expr_fn_to_closure(ast, *left, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *right, fn_sigs, targets, rewrites);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => {
            collect_expr_fn_to_closure(ast, *expr, fn_sigs, targets, rewrites);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_fn_to_closure(ast, *cond, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *then_branch, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *else_branch, fn_sigs, targets, rewrites);
        }
        Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            collect_expr_fn_to_closure(ast, *left, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *right, fn_sigs, targets, rewrites);
        }
        Expr::Array(eids) => {
            for e in eids {
                collect_expr_fn_to_closure(ast, *e, fn_sigs, targets, rewrites);
            }
        }
        Expr::ObjectLit { fields } => {
            // Untyped ObjectLit — only recurse into fields (no
            // closure-typed signal available without surrounding
            // LetDecl context).
            for (_, eid) in fields {
                collect_expr_fn_to_closure(ast, *eid, fn_sigs, targets, rewrites);
            }
        }
        Expr::PostIncr { target, .. } => {
            collect_expr_fn_to_closure(ast, *target, fn_sigs, targets, rewrites);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                collect_expr_fn_to_closure(ast, *a, fn_sigs, targets, rewrites);
            }
        }
        _ => {}
    }
}

/// `const f: any = top_fn` — a bare top-FnDecl Ident crossing into an
/// `any`-annotated binding (any-method-call RFC C4+ FnSig-into-any).
/// Without the wrap the lowerer's fn-addr arm claims the binding as a
/// raw FnSig slot (the annotation is never consulted there), and the
/// bare any-call route then hands the raw fn address to
/// `__torajs_any_call`, which rejects non-closure-cell callees with a
/// TypeError. Rewriting to the `__forward_<name>` zero-capture closure
/// routes the value through the closure construction site, which
/// carries the boxed dual entry — `f(1)` dispatches uniformly.
pub(crate) fn collect_any_let_fn_to_closure(
    ast: &Ast,
    init: ExprId,
    type_ann: Option<&str>,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>)>,
    targets: &mut std::collections::HashSet<String>,
    rewrites: &mut Vec<(ExprId, String)>,
) {
    if type_ann.is_none_or(|a| a.trim() != "any") {
        return;
    }
    if let Expr::Ident(name) = ast.get_expr(init)
        && fn_sigs.contains_key(name)
    {
        targets.insert(name.clone());
        rewrites.push((init, name.clone()));
    }
}

/// When a LetDecl has shape `const o: T = { k: v, ... }` and `T`
/// resolves to a known TypeDecl whose field `k` is fn-typed, and `v`
/// is a bare Ident referring to a top-level FnDecl, mark `v` as
/// rewrite-target.
pub(crate) fn collect_objectlit_fn_to_closure(
    ast: &Ast,
    init: ExprId,
    type_ann: Option<&str>,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>)>,
    struct_field_anns: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
    targets: &mut std::collections::HashSet<String>,
    rewrites: &mut Vec<(ExprId, String)>,
) {
    let Some(ann) = type_ann else { return };
    let Some(field_anns) = struct_field_anns.get(ann.trim()) else {
        return;
    };
    if let Expr::ObjectLit { fields } = ast.get_expr(init) {
        for (fname, feid) in fields {
            if let Some(fann) = field_anns.get(fname)
                && is_fn_like_ann(fann)
                && let Expr::Ident(name) = ast.get_expr(*feid)
                && fn_sigs.contains_key(name)
            {
                targets.insert(name.clone());
                rewrites.push((*feid, name.clone()));
            }
        }
    }
}
