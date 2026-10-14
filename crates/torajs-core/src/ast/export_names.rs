//! §16.2.3 early error — a module's ExportedNames must not repeat.
//!
//! Two `export` clauses may not hand on the same name, however
//! differently they spell it: `export { x as z }` beside
//! `export * as z from "m"`, or `export default x` beside
//! `export * as default from "m"`. Which name a clause exports is
//! purely syntactic, so this is a parse-phase SyntaxError — no module
//! has to be read to see it.
//!
//! `export * from "m"` contributes NOTHING here. The names it hands on
//! are whatever `m` exports, which is not knowable from this file, and
//! a collision reached through a star is not an early error at all —
//! §16.2.1.6 just leaves the name ambiguous and out of the namespace.
//!
//! Runs before `modules::resolve_imports` so the SyntaxError precedes
//! any resolver diagnostic: the test262 duplicate-export cases point
//! their `from` clause at the file itself, and "import error" would
//! otherwise answer first.

use crate::ast::{Ast, ExportStar, Stmt};
use std::collections::HashSet;

/// The names an `export` statement hands on outside its `{ … }` list,
/// which the caller walks itself.
fn clause_exported_names(s: &Stmt, out: &mut Vec<String>) {
    let Stmt::ExportDecl {
        inner,
        default_expr,
        star,
        ..
    } = s
    else {
        return;
    };
    if default_expr.is_some() {
        out.push("default".to_string());
    } else if let Some(ExportStar::AsNamespace(n)) = star {
        out.push(n.clone());
    } else if let Some(d) = inner.as_deref() {
        decl_export_names(d, out);
    }
}

/// A declaration's BoundNames — every declarator of `export var a, a`
/// counts, which the parser hands over as one `Multi`.
fn decl_export_names(s: &Stmt, out: &mut Vec<String>) {
    match s {
        Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } | Stmt::LetDecl { name, .. } => {
            out.push(name.clone())
        }
        Stmt::Multi(list) => list.iter().for_each(|d| decl_export_names(d, out)),
        // `export type T = …` is erased before anything can observe
        // it, and TS lets a type and a value share a name.
        _ => {}
    }
}

/// `Some(msg)` when the module's top level exports one name twice.
pub fn triage_duplicate_exports(ast: &Ast) -> Option<String> {
    let mut seen: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        let Stmt::ExportDecl { named, .. } = s else {
            continue;
        };
        let mut names: Vec<String> = named
            .iter()
            .map(|(orig, alias)| alias.clone().unwrap_or_else(|| orig.clone()))
            .collect();
        clause_exported_names(s, &mut names);
        for n in names {
            if !seen.insert(n.clone()) {
                return Some(format!("duplicate export name `{n}`"));
            }
        }
    }
    None
}
