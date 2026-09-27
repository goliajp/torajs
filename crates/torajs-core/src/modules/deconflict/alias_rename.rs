//! A decl the importer names through exactly one alias (`import { f as
//! g }`, or a re-export's synthetic binding) lands under that alias.
//! Renaming only the decl left every other reference inside the module
//! on the old spelling — the module's own evaluation statements, and
//! sibling decls — reading a binding that was never injected. The
//! census's arena rewrite moves them all, the same move 427-02 makes
//! for a class (`class_rename::try_alias_rename`); this pass covers the
//! decls the census left: non-class kinds, and a decl the census
//! already renamed to its export face (`export { f as "☿" }`).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, Stmt};

use super::class_rename::rename_class_artifacts;
use super::{
    LibTableDelta, copy_fn_name_tables, is_class_decl, rebinds_elsewhere, rename_top_decl,
    type_param_shadows,
};

/// Rename each singly-aliased requested decl to its alias throughout
/// the lib's arena slice. Answers the (spelling, alias) pairs renamed,
/// which the caller re-points its want / visibles maps through. Declines
/// a decl whose spelling or alias is rebound anywhere in the lib.
#[allow(clippy::too_many_arguments)]
pub(super) fn rename_single_aliases(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    visibles: &HashMap<&str, Vec<&str>>,
    bare_exports: &mut HashMap<String, String>,
    delta: &LibTableDelta,
    hidden: &HashSet<String>,
    hidden_inject: &mut HashSet<String>,
) -> Vec<(String, String)> {
    let mut renamed: Vec<(String, String)> = Vec::new();
    for i in 0..lib_section.len() {
        let Some(name) = super::top_value_decl_name(&lib_section[i]) else {
            continue;
        };
        // a bare-exported decl is requested by its face, which the
        // census already made its spelling when the rename was sound
        if bare_exports.get(&name).is_some_and(|face| *face != name) {
            continue;
        }
        let Some([alias]) = visibles.get(name.as_str()).map(Vec::as_slice) else {
            continue;
        };
        if *alias == name
            || rebinds_elsewhere(ast, lib_section, lib_expr_offset, i, &name)
            || rebinds_elsewhere(ast, lib_section, lib_expr_offset, i, alias)
            || type_param_shadows(lib_section, &name)
        {
            continue;
        }
        let alias = alias.to_string();
        rename_top_decl(&mut lib_section[i], &alias);
        copy_fn_name_tables(ast, &name, &alias);
        if is_class_decl(&lib_section[i]) {
            rename_class_artifacts(
                ast,
                lib_section,
                lib_expr_offset,
                i,
                &name,
                &alias,
                delta,
                hidden,
                hidden_inject,
            );
        }
        if bare_exports.remove(&name).is_some() {
            bare_exports.insert(alias.clone(), alias.clone());
        }
        renamed.push((name, alias));
    }
    if renamed.is_empty() {
        return renamed;
    }
    let map: HashMap<&str, &str> = renamed
        .iter()
        .map(|(n, a)| (n.as_str(), a.as_str()))
        .collect();
    for e in ast.exprs[lib_expr_offset..].iter_mut() {
        match e {
            Expr::Ident(n) => {
                if let Some(a) = map.get(n.as_str()) {
                    *n = (*a).to_string();
                }
            }
            Expr::New { class_name, .. } => {
                if let Some(a) = map.get(class_name.as_str()) {
                    *class_name = (*a).to_string();
                }
            }
            _ => {}
        }
    }
    renamed
}
