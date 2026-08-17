//! 423-01 knife C — the hidden-dependency census. The parent answers
//! "does this decl need a new name"; this file answers the question
//! that precedes it: which UN-requested lib decls must still inject
//! because an injected decl's free variables reach them.

use crate::ast::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

/// Which non-named lanes the request carries — drives the hidden-
/// dependency census's seed set. A side-effect walk injects every
/// statement in source order, so it has no hidden deps at all.
pub(in crate::modules) struct LaneShape {
    pub(in crate::modules) has_default_alias: bool,
    pub(in crate::modules) is_ns: bool,
    pub(in crate::modules) side_effect_only: bool,
}

/// Which UN-injected lib decls must still land because an injected
/// decl's free variables reach them (424-02: lib `function mk(){…};
/// export const c = mk();` under `import { c }` left `mk` unbound).
/// Seeds are the decls THIS request will inject: want-hit exports and
/// bare exports (every one of them under a namespace request), plus
/// the default expr when a default binding was asked. Their
/// free-identifier sets intersect the lib's own top-level value decl
/// names, transitively to a fixpoint. The answer holds ORIGINAL
/// spellings; the census mangles them (never importer-visible) and
/// the walk injects them.
pub(super) fn hidden_injection_closure(
    ast: &Ast,
    lib_section: &[Stmt],
    want: &HashSet<&str>,
    bare_exports: &HashMap<String, String>,
    lane: &LaneShape,
) -> HashSet<String> {
    let mut top_decls: HashMap<String, usize> = HashMap::new();
    for (i, s) in lib_section.iter().enumerate() {
        if let Some(n) = super::top_value_decl_name(s).or_else(|| top_class_decl_name(s)) {
            top_decls.insert(n, i);
        }
    }
    let mut will_inject: HashSet<String> = HashSet::new();
    // Free names awaiting classification, from every seed decl/expr.
    let mut work: Vec<String> = Vec::new();
    for (i, s) in lib_section.iter().enumerate() {
        match s {
            Stmt::ExportDecl { inner: Some(d), .. } => {
                if let Some(n) = super::super::decl_name(d)
                    && (lane.is_ns || want.contains(n.as_str()))
                {
                    will_inject.insert(n);
                    work.extend(crate::ast::free_idents_of_stmt(ast, &lib_section[i]));
                }
            }
            Stmt::ExportDecl {
                default_expr: Some(eid),
                ..
            } if lane.has_default_alias => {
                work.extend(crate::ast::free_idents_of_stmt(ast, &Stmt::Expr(*eid)));
            }
            other => {
                if let Some(n) = super::super::decl_name(other)
                    && let Some(face) = bare_exports.get(&n)
                    && (lane.is_ns || want.contains(face.as_str()))
                {
                    will_inject.insert(n);
                    work.extend(crate::ast::free_idents_of_stmt(ast, &lib_section[i]));
                }
            }
        }
    }
    let mut hidden: HashSet<String> = HashSet::new();
    while let Some(f) = work.pop() {
        let Some(&j) = top_decls.get(&f) else {
            continue;
        };
        if will_inject.contains(&f) || !hidden.insert(f.clone()) {
            continue;
        }
        work.extend(crate::ast::free_idents_of_stmt(ast, &lib_section[j]));
    }
    hidden
}

/// The ClassDecl name a top-level lib statement declares (through the
/// `export` wrapper). Kept separate from `top_value_decl_name`: the
/// census can MANGLE a Fn/Let decl, while a class dep can only inject
/// BARE — `__priv_<C>__` member names and every class-keyed side
/// table bake the class name at parse time (knife D territory).
pub(super) fn top_class_decl_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => top_class_decl_name(inner),
        Stmt::ClassDecl { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Post-census classification of the hidden CLASS deps: a class
/// cannot mangle, so it injects under its own spelling when nothing
/// collides, and keeps the loud unknown-identifier reject when
/// something does.
pub(super) fn admit_bare_class_deps(
    lib_section: &[Stmt],
    hidden: &HashSet<String>,
    seed: &super::SeedNames,
    current_path: &std::path::Path,
    hidden_inject: &mut HashSet<String>,
) {
    for s in lib_section {
        let Some(name) = top_class_decl_name(s) else {
            continue;
        };
        if !hidden.contains(&name) {
            continue;
        }
        let collides = seed.hard.contains(&name)
            || seed.reserved.get(&name).is_some_and(|p| p != current_path);
        if !collides {
            hidden_inject.insert(name);
        }
    }
}
