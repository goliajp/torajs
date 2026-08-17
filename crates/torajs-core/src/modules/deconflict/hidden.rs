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
        if let Some(n) = super::top_value_decl_name(s) {
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
                    push_class_synths(lib_section, &n, &mut work);
                    push_computed_key_frees(ast, &n, &mut work);
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
                    push_class_synths(lib_section, &n, &mut work);
                    push_computed_key_frees(ast, &n, &mut work);
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
        push_class_synths(lib_section, &f, &mut work);
        push_computed_key_frees(ast, &f, &mut work);
        work.extend(crate::ast::free_idents_of_stmt(ast, &lib_section[j]));
    }
    hidden
}

/// Knife D — a computed METHOD's key expression lives only in the
/// `class_computed_keys` / `class_computed_static_fields` side tables
/// (a computed FIELD also gets a `__ccmk_` hoist decl, but a method
/// does not), so the statement walk cannot reach whatever the key
/// reads. Feed those expressions' free idents into the closure when
/// their class joins it.
fn push_computed_key_frees(ast: &Ast, class: &str, work: &mut Vec<String>) {
    let eids: Vec<crate::ast::ExprId> = ast
        .class_computed_keys
        .iter()
        .filter(|(k, _)| k.0 == class)
        .map(|(_, v)| *v)
        .chain(
            ast.class_computed_static_fields
                .iter()
                .filter(|r| r.0 == class)
                .map(|r| r.2),
        )
        .collect();
    for eid in eids {
        work.extend(crate::ast::free_idents_of_stmt(ast, &Stmt::Expr(eid)));
    }
}

/// Knife D — a class's parser synthetics (`__ccmk_<C>_<n>` computed-
/// key hoists, `__cm_gen_<C>__<m>` generator forwarders) are separate
/// TOP-LEVEL decls the free-vars walk cannot connect to their class:
/// the class body reaches them through side-table ExprIds
/// (`class_computed_keys`) or desugar-time reconstruction, not
/// through a statement-tree Ident. Whenever a class lands in the
/// injection set, its synthetics enter the work list so the closure
/// carries them (and, transitively, whatever their initializers
/// read). Cheap no-op for a non-class name — nothing matches the
/// prefixes.
fn push_class_synths(lib_section: &[Stmt], class: &str, work: &mut Vec<String>) {
    let ccmk = format!("__ccmk_{class}_");
    let genp = format!("__cm_gen_{class}__");
    for s in lib_section {
        let inner = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner.as_ref(),
            other => other,
        };
        if let Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } = inner
            && (name.starts_with(&ccmk) || name.starts_with(&genp))
        {
            work.push(name.clone());
        }
    }
}

// Knife D — `top_class_decl_name` / `admit_bare_class_deps` retired:
// `top_value_decl_name` admits ClassDecl now, so a hidden class dep
// rides the census's ordinary mangle/decline lanes like any Fn/Let
// (`rename_class_artifacts` moves the baked spellings with it).
