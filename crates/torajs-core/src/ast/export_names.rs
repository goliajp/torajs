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

/// The one name an `export` statement hands on, when it hands on
/// exactly one. Clauses with a list are walked by the caller.
fn single_exported_name(s: &Stmt) -> Option<String> {
    let Stmt::ExportDecl {
        inner,
        default_expr,
        star,
        ..
    } = s
    else {
        return None;
    };
    if default_expr.is_some() {
        return Some("default".to_string());
    }
    if let Some(ExportStar::AsNamespace(n)) = star {
        return Some(n.clone());
    }
    inner.as_deref().and_then(decl_export_name)
}

fn decl_export_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } | Stmt::LetDecl { name, .. } => {
            Some(name.clone())
        }
        // `export type T = …` is erased before anything can observe
        // it, and TS lets a type and a value share a name.
        _ => None,
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
        names.extend(single_exported_name(s));
        for n in names {
            if !seen.insert(n.clone()) {
                return Some(format!("duplicate export name `{n}`"));
            }
        }
    }
    None
}
