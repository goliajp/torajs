//! Program-wide by-name bind-annotation collection — the sniff
//! FALLBACK behind [`super::closure_capture_anns`]'s construction-
//! site snapshots. Split out of that file (file-size hard limit)
//! when the rotation-261 `__this` exclusion grew it past 500 prod
//! lines; body verbatim.

use super::closure_capture_anns::Binds;
use super::{AstExprsView, Param, Stmt, binds_to_params, infer_expr_ann_with};
use std::collections::HashMap;

/// RFC 20260705 chunk 556 — program-wide bind-annotation collection
/// for the closure return-ann sniff (moved here from
/// `ast_desugar_implicit_generics` when the construction-site
/// snapshot pass joined it in this bind-collection domain). Recurses
/// through control-flow shapes + FnDecl bodies; by-name, no scope
/// precision, shadowing keeps the last-seen ann. It remains the sniff
/// FALLBACK — `collect_closure_capture_anns` overlays the
/// scope-correct anns for captured names.
pub(crate) fn collect_outer_binds(
    stmts: &[Stmt],
    ast_exprs_view: AstExprsView,
    fn_sigs: &HashMap<String, String>,
    binds: &mut Binds,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                // `__this` is a per-fn synthetic receiver (every
                // desugared class ctor binds one), so a by-name
                // program-wide entry is always some OTHER scope's
                // class — the injected Error subclasses made
                // `function () { return this }` sniff a
                // `ReferenceError` return. Same exclusion as the
                // param arm below; the site snapshot still carries
                // a scope-correct `__this` where one exists.
                if name == "__this" {
                    continue;
                }
                if let Some(ann) = type_ann {
                    binds.insert(name.clone(), ann.clone());
                } else {
                    let bs: Vec<Param> = binds_to_params(binds);
                    if let Some(ann) =
                        infer_expr_ann_with(ast_exprs_view, *init, &bs, binds, fn_sigs)
                    {
                        binds.insert(name.clone(), ann);
                    }
                }
            }
            Stmt::FnDecl { params, body, .. } => {
                for p in params {
                    if let Some(ann) = &p.type_ann
                        && p.name != "__env"
                        && p.name != "__this"
                    {
                        binds.insert(p.name.clone(), ann.clone());
                    }
                }
                collect_outer_binds(body, ast_exprs_view, fn_sigs, binds);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                collect_outer_binds(inner, ast_exprs_view, fn_sigs, binds);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_outer_binds(
                    std::slice::from_ref(then_branch),
                    ast_exprs_view,
                    fn_sigs,
                    binds,
                );
                if let Some(e) = else_branch {
                    collect_outer_binds(std::slice::from_ref(e), ast_exprs_view, fn_sigs, binds);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_outer_binds(std::slice::from_ref(body), ast_exprs_view, fn_sigs, binds);
            }
            Stmt::Labeled { body, .. } => {
                collect_outer_binds(std::slice::from_ref(body), ast_exprs_view, fn_sigs, binds);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_outer_binds(std::slice::from_ref(i), ast_exprs_view, fn_sigs, binds);
                }
                collect_outer_binds(std::slice::from_ref(body), ast_exprs_view, fn_sigs, binds);
            }
            Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
                collect_outer_binds(std::slice::from_ref(body), ast_exprs_view, fn_sigs, binds);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_outer_binds(&c.body, ast_exprs_view, fn_sigs, binds);
                }
                if let Some(d) = default {
                    collect_outer_binds(d, ast_exprs_view, fn_sigs, binds);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_outer_binds(body, ast_exprs_view, fn_sigs, binds);
                collect_outer_binds(catch_body, ast_exprs_view, fn_sigs, binds);
                if let Some(f) = finally_body {
                    collect_outer_binds(f, ast_exprs_view, fn_sigs, binds);
                }
            }
            _ => {}
        }
    }
}
