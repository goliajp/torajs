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

use crate::ast::{
    Ast, ExprId, Param, Stmt, collect_expr_fn_to_closure, collect_objectlit_fn_to_closure,
};

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
