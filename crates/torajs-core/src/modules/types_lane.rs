//! The type-only lane: a module reached through `import type` or
//! `export type … from`. Under `verbatimModuleSyntax` those clauses
//! are erased, so the module is not a requested module (§16.2.1.5) and
//! must not evaluate; what the checker still needs from it is its
//! exported type declarations. A value walk injects those anyway (see
//! `inject_export_inner`), so both lanes claim each type declaration
//! in the path's ledger under [`type_key`] and whichever comes first
//! lands it.

use std::path::Path;

use crate::ast::{Ast, Stmt};

use super::inject_export::rename_decl;
use super::load_lib::load_lib_section;
use super::resolve_helpers::collect_bare_exports;
use super::resolve_state::ResolveState;
use super::work_item::WorkItem;
use super::{is_builtin_module_source, resolve_path};

/// The ledger entry of an injected type declaration — apart from the
/// value names the ledger also holds, since TypeScript lets a type and
/// a value share a spelling.
pub(super) fn type_key(name: &str) -> String {
    format!("__type {name}")
}

/// Queue the type-only request an erased clause makes of `source`.
pub(super) fn queue_types(
    work: &mut std::collections::VecDeque<WorkItem>,
    dir: &Path,
    source: &str,
) -> Result<(), String> {
    if !is_builtin_module_source(source) {
        work.push_back(WorkItem::types(resolve_path(dir, source)?));
    }
    Ok(())
}

/// Walk `path` for its exported type declarations only, following its
/// own imports and re-exports along the same lane.
pub(super) fn walk_types(
    ast: &mut Ast,
    st: &mut ResolveState,
    path: &Path,
    base_dir: &Path,
) -> Result<(), String> {
    if !st
        .injected_names
        .entry(path.to_path_buf())
        .or_default()
        .insert("__types".into())
    {
        return Ok(());
    }
    let (lib_section, _, target_dir, _) = load_lib_section(
        ast,
        path,
        base_dir,
        &mut st.closure_paths,
        &mut st.closure_files,
    )?;
    let bare = collect_bare_exports(&lib_section);
    let mut out = st.injections_by_path.remove(path).unwrap_or_default();
    let ledger = st.injected_names.entry(path.to_path_buf()).or_default();
    for s in lib_section {
        let (decl, exported) = match s {
            Stmt::ImportDecl { source, .. }
            | Stmt::ExportDecl {
                source: Some(source),
                ..
            } => {
                queue_types(&mut st.work, &target_dir, &source)?;
                continue;
            }
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => (*inner, true),
            other => (other, false),
        };
        let Stmt::TypeDecl { name, .. } = &decl else {
            continue;
        };
        let face = match bare.get(name) {
            Some(face) => face.clone(),
            None if exported => name.clone(),
            None => continue,
        };
        if ledger.insert(type_key(&face)) {
            let mut decl = decl;
            rename_decl(&mut decl, face);
            out.push(decl);
        }
    }
    st.graph.record(path.to_path_buf(), Vec::new());
    st.injections_by_path.insert(path.to_path_buf(), out);
    Ok(())
}
