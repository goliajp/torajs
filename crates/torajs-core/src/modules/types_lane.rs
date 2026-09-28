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
        if let Stmt::ClassDecl { name, .. } = &decl {
            // recorded under the spelling an importer names it by
            // (`export { C as D }` answers to `D`); the declaration
            // itself keeps its own name, which is what its children
            // extend and what an unexported parent only has
            let face = bare.get(name).cloned().unwrap_or_else(|| name.clone());
            st.ambient_classes.push((path.to_path_buf(), face, decl));
            continue;
        }
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
/// A class a type-only import names that no value walk landed has
/// no declaration in the program, and every annotation naming it was
/// "unknown type". TS `import type` asks for exactly the shape: the
/// class declaration goes in — ancestors first, static initializers
/// dropped (they are the declaration's only evaluation), the module
/// body never runs — and the name joins `type_only_classes`, so a
/// value use of it is the TS1361 refusal instead of a fresh instance
/// of a class the program never evaluated. A class already declared
/// (the entry's own, or one a value walk landed) is left alone.
pub(super) fn inject_ambient_classes(ast: &mut Ast, st: &mut ResolveState) {
    let mut declared: HashSet<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::ClassDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut front: Vec<Stmt> = Vec::new();
    for (path, orig, _) in &st.type_aliases {
        if st
            .landed_by_path
            .get(path)
            .is_some_and(|m| m.contains_key(orig))
        {
            continue;
        }
        let Some(idx) = st
            .ambient_classes
            .iter()
            .position(|(p, face, _)| p == path && face == orig)
        else {
            continue;
        };
        push_ambient_chain(ast, &st.ambient_classes, idx, &mut declared, &mut front);
        // a bare-clause face lands under the declaration's own name;
        // the type alias that follows resolves the face through the
        // landed map like a value import would
        if let Stmt::ClassDecl { name, .. } = &st.ambient_classes[idx].2
            && name != orig
        {
            st.landed_by_path
                .entry(path.clone())
                .or_default()
                .insert(orig.clone(), name.clone());
        }
    }
    ast.stmts.splice(0..0, front);
}

fn push_ambient_chain(
    ast: &mut Ast,
    ambient: &[(PathBuf, String, Stmt)],
    idx: usize,
    declared: &mut HashSet<String>,
    out: &mut Vec<Stmt>,
) {
    let Stmt::ClassDecl { name, parent, .. } = &ambient[idx].2 else {
        return;
    };
    if !declared.insert(name.clone()) {
        return;
    }
    if let Some(pid) = parent
        && let crate::ast::Expr::Ident(pname) = ast.get_expr(*pid)
        && !declared.contains(pname)
        && let Some(pidx) = ambient
            .iter()
            .position(|(p, _, s)| {
                p == &ambient[idx].0 && matches!(s, Stmt::ClassDecl { name, .. } if name == pname)
            })
            .or_else(|| {
                ambient.iter().position(
                    |(_, _, s)| matches!(s, Stmt::ClassDecl { name, .. } if name == pname),
                )
            })
    {
        push_ambient_chain(ast, ambient, pidx, declared, out);
    }
    let mut decl = ambient[idx].2.clone();
    if let Stmt::ClassDecl { static_init, .. } = &mut decl {
        static_init.clear();
    }
    ast.type_only_classes.insert(name.clone());
    out.push(decl);
}

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
