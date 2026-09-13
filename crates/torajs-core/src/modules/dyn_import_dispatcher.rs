//! Dynamic-import (§13.3.10) dispatcher synthesis — the runtime half
//! of [`super::dyn_import`]: the link's poison verdict per candidate
//! ([`poisoned_candidates`]) and the `__torajs_dyn_import` FnDecl that
//! matches a specifier against the candidate table
//! ([`synth_dispatcher`]). Split from `dyn_import.rs` when it crossed
//! the file-size line (rotation 645); candidate collection stays
//! there.

use crate::ast::{Ast, BinOp, Expr, Param, Stmt};
use std::collections::{HashMap, HashSet};

use super::decl_name;
use super::dyn_import::{DISPATCHER_NAME, DynTarget};
use super::resolve_helpers::NsAccum;

/// Synthesize the dispatcher FnDecl:
///
/// ```ts
/// function __torajs_dyn_import(__spec: any): any {
///   try {
///     const __s: any = String(__spec);
///     if (__s === "<lit_0>") return Promise.resolve(__dyn_ns_c0);
///     ...
///     throw new TypeError("Failed to resolve module specifier '" + __s + "'");
///   } catch (__e) {
///     return Promise.reject(__e);
///   }
/// }
/// ```
///
/// The whole §13.3.10 abrupt surface funnels through ONE catch: a
/// throwing `toString` (step 7) and the miss TypeError both land in
/// `Promise.reject`, which is exactly IfAbruptRejectPromise. The
/// `__dyn_ns_c<k>` idents rewrite to namespace object literals in
/// `inline_dyn_ns_objlits` — synthesis must run before that pass.
/// 426-01 — candidates whose namespace depends on a re-export binding
/// that never materialized. A circular indirect-export chain
/// (`export { x } from` A→B→A) deduplicates to nothing in the BFS:
/// the synthetic `__reex_<ns>_<face>` binding is requested but no
/// declaration ever injects. Per §16.2.1.5 the module fails to
/// instantiate with a SyntaxError — and a dyn-import candidate must
/// reject its promise, never fail the build. A missing (non-circular)
/// indirect export takes the same verdict: same ResolveExport step,
/// same SyntaxError. The dispatcher emits a SyntaxError throw for
/// these entries (riding its own reject-on-abrupt catch) and never
/// references the namespace, so the dangling binding keeps no use
/// site and the checker stays quiet.
/// A candidate is also poisoned when any of its namespace bindings is
/// AMBIGUOUS — two `export * from` clauses landed the same requested
/// spelling from different modules (§16.2.1.6.3 "ambiguous", the
/// t262 instn-iee-err-ambiguous-import family): same ResolveExport
/// verdict, same SyntaxError reject.
pub(super) fn poisoned_candidates(
    table: &[(String, DynTarget)],
    ns_accums: &HashMap<String, NsAccum>,
    injections: &[Stmt],
    ambiguous_locals: &HashSet<String>,
) -> HashSet<String> {
    let namespaces = table.iter().filter_map(|(_, t)| match t {
        DynTarget::Namespace(ns) => Some(ns),
        DynTarget::SyntaxError(_) => None,
    });
    let needs_scan = namespaces
        .clone()
        .any(|ns| ns_accums.get(ns).is_some_and(reex_backed));
    let mut declared: HashSet<String> = HashSet::new();
    if needs_scan {
        collect_decl_names(injections, &mut declared);
    }
    namespaces
        .filter(|ns| {
            ns_accums.get(ns.as_str()).is_some_and(|acc| {
                acc.fields().iter().any(|(_, local)| {
                    ambiguous_locals.contains(local)
                        || (local.starts_with("__reex_") && !declared.contains(local))
                })
            })
        })
        .cloned()
        .collect()
}

fn reex_backed(acc: &NsAccum) -> bool {
    acc.fields()
        .iter()
        .any(|(_, local)| local.starts_with("__reex_"))
}

pub(super) fn collect_decl_names(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Multi(inner) => collect_decl_names(inner, out),
            _ => {
                if let Some(n) = decl_name(s) {
                    out.insert(n);
                }
            }
        }
    }
}

/// `if (<cond>) throw new SyntaxError(<reason>)` — the dispatcher's
/// catch turns it into the §16.2.1.5 promise reject.
fn push_syntax_error_arm(
    ast: &mut Ast,
    body: &mut Vec<Stmt>,
    cond: crate::ast::ExprId,
    reason: String,
) {
    let msg = ast.add_expr(Expr::String(reason.into()));
    let err = ast.add_expr(Expr::New {
        class_name: "SyntaxError".to_string(),
        args: vec![msg],
        type_args: Vec::new(),
    });
    body.push(Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Throw(err)),
        else_branch: None,
    });
}

pub(super) fn synth_dispatcher(
    ast: &mut Ast,
    table: &[(String, DynTarget)],
    poisoned: &HashSet<String>,
) -> Stmt {
    let mut body: Vec<Stmt> = Vec::new();
    // const __s: any = String(__spec);
    let string_ident = ast.add_expr(Expr::Ident("String".to_string()));
    // §13.3.10 runs the intrinsic ToString — like the Promise idents
    // below, no `with` object may substitute these globals.
    ast.synth_global_refs.insert(string_ident);
    let spec_ident = ast.add_expr(Expr::Ident("__spec".to_string()));
    let s_init = ast.add_expr(Expr::Call {
        callee: string_ident,
        args: vec![spec_ident],
    });
    body.push(Stmt::LetDecl {
        mutable: false,
        name: "__s".to_string(),
        type_ann: Some("any".to_string()),
        init: s_init,
        is_var: false,
    });
    // Per candidate: if (__s === "<lit>") return Promise.resolve(<ns>);
    for (lit, target) in table {
        let s_ref = ast.add_expr(Expr::Ident("__s".to_string()));
        let lit_expr = ast.add_expr(Expr::String(lit.clone().into()));
        let cond = ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: s_ref,
            right: lit_expr,
        });
        let ns_name = match target {
            DynTarget::SyntaxError(reason) => {
                push_syntax_error_arm(ast, &mut body, cond, reason.clone());
                continue;
            }
            DynTarget::Namespace(ns) if poisoned.contains(ns.as_str()) => {
                push_syntax_error_arm(
                    ast,
                    &mut body,
                    cond,
                    format!(
                        "indirect exports of module '{lit}' do not resolve (circular, missing, or ambiguous)"
                    ),
                );
                continue;
            }
            DynTarget::Namespace(ns) => ns,
        };
        let ns_ident = ast.add_expr(Expr::Ident(ns_name.clone()));
        let promise_ident = ast.add_expr(Expr::Ident("Promise".to_string()));
        ast.synth_global_refs.insert(promise_ident);
        let resolve = ast.add_expr(Expr::Member {
            obj: promise_ident,
            name: "resolve".to_string(),
        });
        let resolved = ast.add_expr(Expr::Call {
            callee: resolve,
            args: vec![ns_ident],
        });
        body.push(Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Return(Some(resolved))),
            else_branch: None,
        });
    }
    // throw new TypeError("Failed to resolve module specifier '" + __s + "'");
    let msg_head = ast.add_expr(Expr::String(
        "Failed to resolve module specifier '".to_string().into(),
    ));
    let s_ref = ast.add_expr(Expr::Ident("__s".to_string()));
    let head_concat = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: msg_head,
        right: s_ref,
    });
    let msg_tail = ast.add_expr(Expr::String("'".to_string().into()));
    let msg = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: head_concat,
        right: msg_tail,
    });
    let type_error = ast.add_expr(Expr::New {
        class_name: "TypeError".to_string(),
        args: vec![msg],
        type_args: Vec::new(),
    });
    body.push(Stmt::Throw(type_error));
    // catch (__e) { return Promise.reject(__e); }
    let promise_ident = ast.add_expr(Expr::Ident("Promise".to_string()));
    ast.synth_global_refs.insert(promise_ident);
    let reject = ast.add_expr(Expr::Member {
        obj: promise_ident,
        name: "reject".to_string(),
    });
    let e_ident = ast.add_expr(Expr::Ident("__e".to_string()));
    let rejected = ast.add_expr(Expr::Call {
        callee: reject,
        args: vec![e_ident],
    });
    let catch_body = vec![Stmt::Return(Some(rejected))];
    Stmt::FnDecl {
        name: DISPATCHER_NAME.to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            name: "__spec".to_string(),
            type_ann: Some("any".to_string()),
            default: None,
            is_rest: false,
        }],
        return_type: Some("any".to_string()),
        body: vec![Stmt::Try {
            body,
            had_catch: true,
            catch_param: Some("__e".to_string()),
            catch_type: None,
            catch_body,
            finally_body: None,
        }],
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}
