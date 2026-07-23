//! Support helpers for [`super::infer_closure_params`] — extracted
//! as the rotation-196 file-size sweep. The parent had drifted to
//! 508 prod LOC over the recent .map/.flatMap wedge additions;
//! `mapset_foreach_expected` (Map/Set forEach param shape) and
//! `build_ann_table` (per-name annotation gather over top-level
//! FnDecls + let-inits) are self-contained and share zero mutable
//! state with the main walker, so cluster naturally into a sibling.
//! Verbatim moves; signatures / semantics unchanged.

use std::collections::HashMap;

use crate::ast::infer_closure_lets::{collect_let_anns, collect_let_init_anns};
use crate::ast::{Ast, Stmt};

/// Callback param/return annotations for `forEach` on a `Map<K|V>` /
/// `Set<T>` receiver ann (the flat generic spelling). None for any
/// other method or receiver shape — Map/Set carry no other
/// callback-bearing methods.
pub(super) fn mapset_foreach_expected(ann: &str, method: &str) -> Option<(Vec<String>, String)> {
    if method != "forEach" {
        return None;
    }
    if let Some(inner) = ann.strip_prefix("Map<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner, true);
        let [k, v] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![v.to_string(), k.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    if let Some(inner) = ann.strip_prefix("Set<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner, true);
        let [t] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![t.to_string(), t.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    None
}

/// Per-name → type-annotation table feeding receiver resolution.
/// Walk all top-level FnDecl bodies gathering param + let-decl
/// annotations (the same name may appear in multiple fns; call-site
/// inference resolves the right binding via the enclosing fn), plus:
///
/// - V3-18 m1.h.23 — top-level let decls (the synthetic `main`
///   wraps these at ssa_lower time, but at this AST pass they sit at
///   ast.stmts level, so the FnDecl-only walk misses them; without
///   this `let arr = [1,2,3]; arr.find(x => ...)` can't infer x).
/// - Inferred-from-init shape: `let arr = [<lit>, ...]` infers
///   arr's annotation as `<lit_ty>[]` so .map / .filter on
///   unannotated lets still get param inference.
pub(super) fn build_ann_table(ast: &Ast) -> HashMap<String, String> {
    let mut all_anns: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                if let Some(ann) = &p.type_ann {
                    all_anns.insert(p.name.clone(), ann.clone());
                }
            }
            collect_let_anns(body, &mut all_anns);
        }
    }
    collect_let_anns(&ast.stmts, &mut all_anns);
    let mut inferred_inits: HashMap<String, String> = HashMap::new();
    collect_let_init_anns(ast, &ast.stmts, &mut inferred_inits);
    for (k, v) in inferred_inits {
        all_anns.entry(k).or_insert(v);
    }
    all_anns
}
