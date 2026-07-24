//! Receiver ground truth for `apply_default_args`' Member-arm
//! padding gate — split out of `apply_args.rs` (file-size limit)
//! when the receiver-precise suppression landed.

use std::collections::HashMap;

use super::{Ast, Expr, Stmt};

/// Receiver ground truth for the Member-arm padding gate: walk every
/// statement container recursing into fn bodies, counting EVERY
/// binding occurrence per name (let/const decls, params, for-of loop
/// vars, catch params) and recording the field map of each
/// ObjectLit-init let (field name → `Some(closure fn name)` for
/// method-shaped fields, `None` for plain value fields). The caller
/// only trusts a field map when the name's total binding count is 1.
pub(super) fn collect_objlit_recv_fields(
    ast: &Ast,
    stmts: &[Stmt],
    classify: &impl Fn(&Expr) -> Option<String>,
    counts: &mut HashMap<String, usize>,
    objlit: &mut HashMap<String, HashMap<String, Option<String>>>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                *counts.entry(name.clone()).or_insert(0) += 1;
                if let Expr::ObjectLit { fields } = ast.get_expr(*init) {
                    let fm = fields
                        .iter()
                        .map(|(f, feid)| (f.clone(), classify(ast.get_expr(*feid))))
                        .collect();
                    objlit.insert(name.clone(), fm);
                }
            }
            Stmt::FnDecl { params, body, .. } => {
                for p in params {
                    *counts.entry(p.name.clone()).or_insert(0) += 1;
                }
                collect_objlit_recv_fields(ast, body, classify, counts, objlit);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                collect_objlit_recv_fields(ast, inner, classify, counts, objlit);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(then_branch),
                    classify,
                    counts,
                    objlit,
                );
                if let Some(eb) = else_branch {
                    collect_objlit_recv_fields(
                        ast,
                        core::slice::from_ref(eb),
                        classify,
                        counts,
                        objlit,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(body),
                    classify,
                    counts,
                    objlit,
                );
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_objlit_recv_fields(
                        ast,
                        core::slice::from_ref(i),
                        classify,
                        counts,
                        objlit,
                    );
                }
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(body),
                    classify,
                    counts,
                    objlit,
                );
            }
            Stmt::ForOf {
                var_name,
                i_ident,
                body,
                ..
            } => {
                *counts.entry(var_name.clone()).or_insert(0) += 1;
                *counts.entry(i_ident.clone()).or_insert(0) += 1;
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(body),
                    classify,
                    counts,
                    objlit,
                );
            }
            Stmt::ForOfSplitIter { var_name, body, .. } => {
                *counts.entry(var_name.clone()).or_insert(0) += 1;
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(body),
                    classify,
                    counts,
                    objlit,
                );
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                if let Some(cp) = catch_param {
                    *counts.entry(cp.clone()).or_insert(0) += 1;
                }
                collect_objlit_recv_fields(ast, body, classify, counts, objlit);
                collect_objlit_recv_fields(ast, catch_body, classify, counts, objlit);
                if let Some(fb) = finally_body {
                    collect_objlit_recv_fields(ast, fb, classify, counts, objlit);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_objlit_recv_fields(ast, &c.body, classify, counts, objlit);
                }
                if let Some(d) = default {
                    collect_objlit_recv_fields(ast, d, classify, counts, objlit);
                }
            }
            _ => {}
        }
    }
}
