//! Phase C (424-04) — expression-default materialization for NESTED
//! `function` declarations. The Annex-B hoist (`desugar_nested_fns`)
//! runs AFTER the parent pass, so a nested FnDecl's params never
//! meet the top-level scan and its defaults stayed on the call-site
//! pad channel — which no DYNAMIC call can fire: a face-mismatched
//! fn-typed binding's boxed-lane call pads `undefined`, and
//! §10.2.1.4 wants a body guard to observe it. Same conversion as
//! the parent's Phase B, addressed through the mutable nested spine.
//! A FnDecl nested in a still-arena ArrowFn body is out of this
//! spine's reach — recorded residual, not silently claimed.

use super::*;

/// The spine walk: `stmts` taken out of `ast` so guard construction
/// can mint arena exprs against the disjoint `ast` while the walked
/// FnDecl's params/body are mutated in place.
pub(super) fn materialize_nested(ast: &mut Ast, global_fns: &[String]) {
    let mut stmts = std::mem::take(&mut ast.stmts);
    for s in &mut stmts {
        crate::ast::stmt_nested_lists::for_each_nested_vec_mut(s, &mut |v| {
            for inner in v.iter_mut() {
                let Stmt::FnDecl { params, body, .. } = inner else {
                    continue;
                };
                let conv = collect_fn_conv(ast, params, global_fns);
                if conv.is_empty() {
                    continue;
                }
                let (guards, pads) = build_guards_and_pads(ast, conv);
                patch_params(params, pads);
                splice_guards(body, guards);
            }
        });
    }
    ast.stmts = stmts;
}
