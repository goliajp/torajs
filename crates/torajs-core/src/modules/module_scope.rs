//! §16.2.1.6 — module scope over the flat top level.
//!
//! The resolver injects every module's statements into one list, so
//! a name one module declares is spelled at the top level for all of
//! them: a lib reading `A` where only the entry declared `A` resolved
//! to the entry's binding, where a module's own environment record
//! (ModuleDeclarationInstantiation step 16, GetBindingValue §9.1.1.5)
//! has no binding and the read throws ReferenceError — test262
//! `module-code/instn-iee-bndng-*` read the entry's function through
//! an indirect export's module and expected exactly that throw.
//!
//! Each module's visible set is its own top-level declarations (the
//! spellings the census left in its section, hidden dependencies
//! included), the import bindings its clauses made, and the global
//! object; every other module's declaration is foreign. A free
//! `Ident` in the module's statements naming a foreign declaration is
//! recorded in `Ast::module_foreign_reads`, and the checker turns it
//! into the unresolvable-Reference lane (`undeclared_reads`: `Any`,
//! a catchable ReferenceError, `typeof` answering "undefined").
//!
//! Never marked: a synthesized spelling (`is_synthesized_dunder` —
//! the resolver's own `__reex_*` / `__nsdefault_*` / mangles) and a
//! namespace alias (its object literal lands in the exporting
//! module's section, not the importer's). The entry's visible set
//! also holds the spellings bound without a walk
//! (`entry_self_bindings`) and both sides of every repeat alias, whose
//! reads `follow_aliases` rewrites after the splice.
//!
//! Runs after the worklist drains and before the splice, while each
//! module's statements are still keyed by path.

use super::inject_export::decl_name;
use super::resolve_state::ResolveState;
use super::static_resolution::EntrySeed;
use crate::ast::{Ast, ExprId, Stmt, foreign_ref_sites, is_synthesized_dunder};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(super) fn mark_foreign_reads(ast: &mut Ast, st: &ResolveState, entry_seed: &EntrySeed) {
    if st.injections_by_path.is_empty() {
        return;
    }
    let (all, visible_by_path, entry_visible) = visible_sets(ast, st, entry_seed);
    let mut marks: Vec<ExprId> = Vec::new();
    for (path, stmts) in &st.injections_by_path {
        let foreign: HashSet<String> = all
            .difference(&visible_by_path[path.as_path()])
            .cloned()
            .collect();
        if foreign.is_empty() {
            continue;
        }
        for s in stmts {
            marks.extend(foreign_ref_sites(ast, s, &foreign));
        }
    }
    let foreign: HashSet<String> = all.difference(&entry_visible).cloned().collect();
    if !foreign.is_empty() {
        for s in &ast.stmts {
            marks.extend(foreign_ref_sites(ast, s, &foreign));
        }
    }
    ast.module_foreign_reads.extend(marks);
}

type VisibleSets<'a> = (
    HashSet<String>,
    HashMap<&'a Path, HashSet<String>>,
    HashSet<String>,
);

/// Every markable declaration name in the program, each library
/// module's visible set (its own declarations plus the import
/// bindings its clauses made), and the entry's.
fn visible_sets<'a>(ast: &Ast, st: &'a ResolveState, entry_seed: &EntrySeed) -> VisibleSets<'a> {
    let mut visible_by_path: HashMap<&Path, HashSet<String>> = st
        .injections_by_path
        .iter()
        .map(|(p, stmts)| {
            (
                p.as_path(),
                stmts.iter().filter_map(top_decl_name).collect(),
            )
        })
        .collect();
    let entry_decls: HashSet<String> = ast.stmts.iter().filter_map(top_decl_name).collect();
    let mut all: HashSet<String> = entry_decls.clone();
    for d in visible_by_path.values() {
        all.extend(d.iter().cloned());
    }
    all.retain(|n| !st.ns.accums.contains_key(n) && !is_synthesized_dunder(ast, n));
    for n in &st.ledger.nested {
        if let Some(v) = visible_by_path.get_mut(n.requester.as_path()) {
            v.insert(n.binding.clone());
        }
    }
    let mut entry_visible = entry_decls;
    entry_visible.extend(entry_seed.import_bindings.iter().cloned());
    entry_visible.extend(st.entry_self_bindings.iter().cloned());
    for (later, first) in &st.repeat_aliases {
        entry_visible.insert(later.clone());
        entry_visible.insert(first.clone());
    }
    (all, visible_by_path, entry_visible)
}

/// A statement's top-level declaration name, through the `export`
/// wrapper the entry's statements still carry at this point.
fn top_decl_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::ExportDecl { inner: Some(d), .. } => decl_name(d),
        _ => decl_name(s),
    }
}
