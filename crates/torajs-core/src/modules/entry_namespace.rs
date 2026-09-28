//! `import * as ns from "./self"` — the entry importing its own
//! namespace. Split from `repeat_request` at the file-size line.

use crate::ast::{Ast, Stmt};

use super::resolve_helpers::NsAccum;

/// `import * as ns from "./self"` — the entry's own namespace. Its
/// fields are the entry's own top-level bindings, claimed straight
/// from its export clauses: a walk of the entry would inject a second
/// copy of every declaration (mangled past the collision with the
/// original) and point the fields at the copies, so `ns.x` after
/// `x++` still read the copy's initial value. The entry's `export
/// default <expr>` has no binding of its own, so it becomes one here
/// (`__nsdefault_entry`, evaluated in place) and the `default` field
/// names it. An entry that re-exports (`export … from`, `export *
/// from`) answers false and keeps the walk: those fields come from
/// the modules the clauses name, which only the walk reaches.
pub(super) fn claim_entry_namespace(
    ast: &mut Ast,
    alias: &str,
    tables: &mut super::ns_tables::NsTables,
) -> bool {
    let reexports = ast.stmts.iter().any(|s| {
        matches!(
            s,
            Stmt::ExportDecl {
                source: Some(_),
                ..
            }
        )
    });
    if reexports {
        return false;
    }
    if !tables.accums.contains_key(alias) {
        tables.order.push(alias.to_string());
        tables
            .accums
            .insert(alias.to_string(), NsAccum::new(alias.to_string()));
    }
    let acc = tables.accums.get_mut(alias).expect("inserted above");
    for s in ast.stmts.iter_mut() {
        match s {
            Stmt::ExportDecl { inner: Some(d), .. } => {
                if let Some(n) = super::inject_export::decl_name(d) {
                    acc.claim(&n, &n);
                }
            }
            Stmt::ExportDecl {
                inner: None,
                default_expr: Some(eid),
                ..
            } => {
                acc.claim("default", ENTRY_DEFAULT);
                // `any`: the namespace literal at the program's head
                // reads it as a global cell whatever the initializer
                // (`export default null` has no slot shape to infer)
                *s = Stmt::LetDecl {
                    mutable: false,
                    name: ENTRY_DEFAULT.to_string(),
                    type_ann: Some("any".to_string()),
                    init: *eid,
                    is_var: false,
                };
            }
            Stmt::LetDecl { name, .. } if name == ENTRY_DEFAULT => {
                acc.claim("default", ENTRY_DEFAULT);
            }
            Stmt::ExportDecl {
                inner: None,
                named,
                source: None,
                type_only: false,
                ..
            } => {
                for (local, face) in named.iter() {
                    acc.claim(face.as_deref().unwrap_or(local), local);
                }
            }
            _ => {}
        }
    }
    true
}

/// The binding the entry's `export default <expr>` becomes when the
/// entry imports its own namespace (`claim_entry_namespace`).
pub(super) const ENTRY_DEFAULT: &str = "__nsdefault_entry";
