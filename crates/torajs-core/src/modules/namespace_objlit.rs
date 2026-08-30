//! Namespace-object materialization — the P13-S2 half of the BFS
//! resolver's helpers, carved out of `resolve_helpers` at the
//! 500-line boundary. That sibling answers "which requests still
//! need walking, and what does a lib contribute"; this one answers
//! "what does a namespace alias become".
//!
//! Two mints, one shape. A static `import * as ns` binds a synthetic
//! top-level `let`; a dynamic `import()` namespace has no top-level
//! binding to reach from a lifted fn body, so its object literal goes
//! in at each use site instead.

use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashMap;

use super::resolve_helpers::NsAccum;

/// P13-S2 — materialize each namespace alias as a synthetic
/// `let <alias> = { name1: name1, name2: name2, ... }`. Each field's
/// value is an `Ident(name)` referencing an already-injected lib decl,
/// so member access (`<alias>.<name>`) resolves through the struct's
/// field type.
///
/// Emission waits until the BFS has drained — that is what lets a
/// star re-export contribute to a namespace whose own lib was walked
/// several work items earlier. Order is REVERSE discovery: a namespace
/// can only name another that was discovered while walking it
/// (`export * as inner from "m"`), so the later-discovered one has to
/// bind first.
pub(super) fn materialize_pending_namespaces(
    ast: &mut Ast,
    injections: &mut Vec<Stmt>,
    order: &[String],
    accums: &HashMap<String, NsAccum>,
    dyn_ns_inline: &mut Vec<(String, Vec<(String, String)>)>,
) {
    for alias in order.iter().rev() {
        let Some(accum) = accums.get(alias) else {
            continue;
        };
        if alias.starts_with("__dyn_ns_") {
            dyn_ns_inline.push((alias.clone(), accum.fields().to_vec()));
            continue;
        }
        let mut fields: Vec<(String, crate::ast::ExprId)> =
            Vec::with_capacity(accum.fields().len());
        for (name, local) in accum.fields() {
            let id = ast.add_expr(Expr::Ident(local.clone()));
            fields.push((name.clone(), id));
        }
        let obj_id = ast.add_expr(Expr::ObjectLit { fields });
        // §10.4.6.8 — mark the binding so a member miss on it answers
        // undefined instead of the anonymous-struct typo reject.
        ast.namespace_bindings
            .insert(alias.clone(), accum.fields().to_vec());
        injections.push(Stmt::LetDecl {
            mutable: false,
            name: alias.clone(),
            type_ann: None,
            init: obj_id,
            is_var: false,
        });
    }
}

/// Dynamic-import namespaces skip the `let` above: the use site may
/// sit inside a (lifted) fn body where a main-local top-level binding
/// is invisible, so each `Ident(__dyn_ns_<n>)` becomes the namespace
/// object literal in place. Field Idents reference the injected lib
/// decls, which named-fn bodies resolve (FnDecls via the pass-1
/// signature hoist, literal consts via the pass-2 pre-pass). The
/// arena scan is name-keyed — sound because `__dyn_ns_<n>` names are
/// parser-minted and arena-offset-seeded, so no other Ident can
/// spell one.
pub(super) fn inline_dyn_ns_objlits(ast: &mut Ast, rewrites: &[(String, Vec<(String, String)>)]) {
    for (ns_name, field_names) in rewrites {
        let hits: Vec<usize> = ast
            .exprs
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Expr::Ident(n) if n == ns_name => Some(i),
                _ => None,
            })
            .collect();
        for idx in hits {
            let mut fields: Vec<(String, crate::ast::ExprId)> =
                Vec::with_capacity(field_names.len());
            for (name, local) in field_names {
                let id = ast.add_expr(Expr::Ident(local.clone()));
                fields.push((name.clone(), id));
            }
            ast.exprs[idx] = Expr::ObjectLit { fields };
        }
    }
}
