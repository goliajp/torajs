//! Namespace-object materialization — the P13-S2 half of the BFS
//! resolver's helpers, carved out of `resolve_helpers` at the
//! 500-line boundary. That sibling answers "which requests still
//! need walking, and what does a lib contribute"; this one answers
//! "what does a namespace alias become".
//!
//! Two mints, one shape. A static `import * as ns` binds a synthetic
//! top-level `let`; a dynamic `import()` namespace is bound by its
//! candidate's `__dyn_eval_c<k>` after the module body has run
//! (`dyn_import_dispatcher::synth_eval_fn`) — the export values are
//! snapshots, so the literal has to be built after evaluation, and it
//! is memoized so every `import()` of the module answers one object.
//! Both are `let <alias>: … = { … }` rows in `namespace_bindings`,
//! which is what routes them into the dynobj lane and the
//! `module_ns_finalize` call that gives the object its §10.4.6
//! attributes. Rotation 652: the dynamic one used to be an inline
//! object literal at the return site, which lowered as an anonymous
//! struct — extensible, with configurable exports, and (once a
//! declared field could actually be deleted) `delete ns.default`
//! answered true.

use crate::ast::PropKey;
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::{HashMap, HashSet};

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
) {
    for alias in order.iter().rev() {
        // A dynamic candidate's namespace is bound inside its eval fn
        // (`synth_dispatcher` took its fields from these same accums).
        if alias.starts_with("__dyn_ns_") {
            continue;
        }
        let Some(accum) = accums.get(alias) else {
            continue;
        };
        let sorted = sorted_exports(accum.fields());
        let obj_id = namespace_objlit_expr(ast, &sorted);
        // §10.4.6.8 — mark the binding so a member miss on it answers
        // undefined instead of the anonymous-struct typo reject.
        ast.namespace_bindings.insert(alias.clone(), sorted);
        injections.push(Stmt::LetDecl {
            mutable: false,
            name: alias.clone(),
            type_ann: None,
            init: obj_id,
            is_var: false,
        });
    }
}

/// `{ name1: local1, name2: local2, … }` — the namespace object literal
/// over an already-sorted export list; each value is an `Ident(local)`
/// naming an injected lib decl.
pub(super) fn namespace_objlit_expr(
    ast: &mut Ast,
    sorted: &[(String, String)],
) -> crate::ast::ExprId {
    let mut fields: Vec<(PropKey, crate::ast::ExprId)> = Vec::with_capacity(sorted.len());
    for (name, local) in sorted {
        let id = ast.add_expr(Expr::Ident(local.clone()));
        fields.push((PropKey::from(name), id));
    }
    ast.add_expr(Expr::ObjectLit { fields })
}

/// §10.4.6.12 step 7 — the exports the object literal is built from,
/// sorted by CODE UNIT. The resolver discovers them in walk order,
/// but `[[OwnPropertyKeys]]` (§10.4.6.11) answers a SORTED copy and a
/// dynobj answers own-keys in insertion order, so the sort belongs at
/// the mint: one comparison per export at compile time, nothing at
/// run time.
///
/// Code unit, not `str`'s byte order. They agree across the BMP and
/// disagree above it — a supplementary character sorts before U+E000
/// as a surrogate pair and after it as UTF-8.
pub(super) fn sorted_exports(fields: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = fields.to_vec();
    out.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
    out
}

/// §16.2.1.10 — a namespace object's property read is the export's
/// binding itself, live; the literal above holds the values it saw
/// when it ran. Every `<alias>.<field>` READ of a static namespace
/// alias therefore becomes the landed binding's own name — unless
/// some inner scope rebinds the alias, or the member is written
/// (`ns.x = v` is the §10.4.6 TypeError, left to the object). The
/// object remains for reflection: `Object.keys(ns)`, `"x" in ns`,
/// spread.
pub(super) fn read_through_namespace(ast: &mut Ast) {
    if ast.namespace_bindings.is_empty() {
        return;
    }
    let mut written: HashSet<ExprId> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                written.insert(*target);
            }
            Expr::Delete { expr } => {
                written.insert(*expr);
            }
            _ => {}
        }
    }
    let aliases: Vec<String> = ast
        .namespace_bindings
        .keys()
        .filter(|a| !a.starts_with("__dyn_ns_") && !super::repeat_request::rebound(ast, a))
        .cloned()
        .collect();
    let mut rewrites: Vec<(ExprId, String)> = Vec::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let Expr::Member { obj, name } = e else {
            continue;
        };
        let eid = ExprId(i as u32);
        if written.contains(&eid) {
            continue;
        }
        let Expr::Ident(alias) = ast.get_expr(*obj) else {
            continue;
        };
        if !aliases.contains(alias) {
            continue;
        }
        if let Some((_, local)) = ast.namespace_bindings[alias]
            .iter()
            .find(|(f, _)| f == name)
        {
            rewrites.push((eid, local.clone()));
        }
    }
    for (eid, local) in rewrites {
        ast.exprs[eid.0 as usize] = Expr::Ident(local);
    }
}
