//! The type-only lane: a module reached through `import type` or
//! `export type … from`. Under `verbatimModuleSyntax` those clauses
//! are erased, so the module is not a requested module (§16.2.1.5) and
//! must not evaluate; what the checker still needs from it is its
//! exported type declarations. A value walk injects those anyway (see
//! `inject_export_inner`), so both lanes claim each type declaration
//! in the path's ledger under [`type_key`] and whichever comes first
//! lands it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{Ast, PropKey, Stmt};

use super::export_faces::collect_bare_exports;
use super::inject_export::rename_decl;
use super::load_lib::load_lib_section;
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
    let (bare, other_faces) = collect_bare_exports(&lib_section, &HashSet::new(), false);
    let mut out = st.injections_by_path.remove(path).unwrap_or_default();
    let ledger = st.injected_names.entry(path.to_path_buf()).or_default();
    for s in lib_section {
        let (decl, exported) = match s {
            Stmt::ImportDecl { ref source, .. } => {
                note_type_aliases(&mut st.type_aliases, &target_dir, &s);
                queue_types(&mut st.work, &target_dir, source)?;
                continue;
            }
            Stmt::ExportDecl {
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
        // every face of the one declaration: a type carries no
        // identity, so each lands as its own copy
        let others = other_faces.get(&face).cloned().unwrap_or_default();
        for face in std::iter::once(face).chain(others) {
            if ledger.insert(type_key(&face)) {
                let mut decl = decl.clone();
                rename_decl(&mut decl, face);
                out.push(decl);
            }
        }
    }
    st.graph.record(path.to_path_buf(), Vec::new());
    st.injections_by_path.insert(path.to_path_buf(), out);
    Ok(())
}

/// Record the type imports an import clause makes — `import type
/// { A as B }`, `import { type A as B }`, and the unaliased `import
/// type { C }` — as (source path, exported name, local spelling).
/// Both lanes inject a module's type declarations under their
/// exported names, so an alias needs its own declaration; and a
/// class reaches the program under whatever spelling its value walk
/// landed it (a hidden dependency lands mangled), so even the
/// unaliased import may need one ([`inject_type_aliases`]).
pub(super) fn note_type_aliases(out: &mut Vec<(PathBuf, String, String)>, dir: &Path, s: &Stmt) {
    let Stmt::ImportDecl {
        source,
        named,
        type_only,
        type_named,
        ..
    } = s
    else {
        return;
    };
    if is_builtin_module_source(source) {
        return;
    }
    let Ok(path) = resolve_path(dir, source) else {
        return;
    };
    let types = type_named.iter().chain(named.iter().filter(|_| *type_only));
    for (orig, alias) in types {
        let local = alias.clone().unwrap_or_else(|| orig.clone());
        out.push((path.clone(), orig.clone(), local));
    }
}

/// `type B<Ps> = A<Ps>` for each recorded type import whose local
/// spelling differs from the spelling its declaration landed under,
/// once every module's declarations have landed — appended last, so
/// the checker resolves the alias after its target. A type
/// declaration lands under its exported name; a class lands under
/// the spelling its value walk gave it (`landed`), which a hidden
/// dependency's mangle may have moved away from the exported name.
/// A target that never landed stays an unknown type.
pub(super) fn inject_type_aliases(
    ast: &mut Ast,
    aliases: &[(PathBuf, String, String)],
    landed: &HashMap<PathBuf, HashMap<String, String>>,
) {
    let mut seen: HashSet<&str> = HashSet::new();
    for (path, orig, alias) in aliases {
        let target_name = landed.get(path).and_then(|m| m.get(orig)).unwrap_or(orig);
        if target_name == alias || !seen.insert(alias) {
            continue;
        }
        let Some(params) = ast.stmts.iter().find_map(|s| match s {
            Stmt::TypeDecl {
                name, type_params, ..
            }
            | Stmt::ClassDecl {
                name, type_params, ..
            } if name == target_name => Some(type_params.clone()),
            _ => None,
        }) else {
            continue;
        };
        // the parser joins generic arguments with `|`
        let target = if params.is_empty() {
            target_name.clone()
        } else {
            format!("{target_name}<{}>", params.join("|"))
        };
        ast.stmts.push(Stmt::TypeDecl {
            name: alias.clone(),
            type_params: params,
            fields: vec![(PropKey::from("__alias__"), target)],
        });
    }
}
