//! 423-01 knife D — full ClassDecl census rename.
//!
//! A class bakes its name at parse time into places a Fn/Let rename
//! never touches: `__priv_<C>__` member spellings (declaration side,
//! `Member`/`OptChain` reference side, and the `#x in o` key string),
//! the `__ccmk_<C>_<n>` computed-key hoists, the `__cm_gen_<C>__<m>`
//! generator forwarder decls, the `__cm_/__sm_<C>__` super-call
//! resolutions inside generator bodies, type-annotation strings, and
//! a set of name-keyed parser tables. Renaming the decl without
//! moving ALL of them turns today's loud collisions into silent
//! cross-module identity confusion (two `class C { #x }` sharing one
//! `__priv_C__x` brand — §13.10 `#x in o` answering true across
//! modules). So this file is the single place that does the whole
//! move; the census calls it wherever it renames a ClassDecl.
//!
//! Table-entry ownership: the parser merges same-named classes from
//! the entry and the lib into ONE name-keyed table row, so a rename
//! must decide which rows belong to the lib. Three mechanisms, in
//! preference order — (1) rows carrying an ExprId are split by the
//! lib's arena offset (exact); (2) rows whose key appeared or whose
//! value changed during the lib's parse (the snapshot diff taken by
//! `load_lib_section`) are the lib's (exact, and a changed value is
//! restored for the entry); (3) a remaining row whose key both sides
//! wrote identically is COPIED onto the mangle, gated on the lib
//! class actually having the member/ctor/parent in question — a
//! leftover row with no matching decl is a no-op for every consumer
//! (the knife-A `copy_fn_name_tables` posture).

use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashSet;

/// Does any decl in the lib declare a TYPE PARAMETER spelled `name`?
/// The annotation rewrite below is a flat word-boundary substitution
/// over every ann string in the lib — it cannot tell a class
/// reference from a same-named type param, so the census DECLINES
/// the class mangle when one exists (today's loud collision beats a
/// silently retyped signature).
pub(in crate::modules) fn type_param_shadows(lib_section: &[Stmt], name: &str) -> bool {
    fn in_stmt(s: &Stmt, name: &str) -> bool {
        match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => in_stmt(inner, name),
            Stmt::FnDecl {
                type_params, body, ..
            } => type_params.iter().any(|t| t == name) || body.iter().any(|s| in_stmt(s, name)),
            Stmt::TypeDecl { type_params, .. } => type_params.iter().any(|t| t == name),
            Stmt::ClassDecl {
                type_params,
                ctor,
                methods,
                static_methods,
                ..
            } => {
                type_params.iter().any(|t| t == name)
                    || methods
                        .iter()
                        .chain(static_methods.iter())
                        .any(|m| m.type_params.iter().any(|t| t == name))
                    || ctor
                        .as_ref()
                        .is_some_and(|c| c.body.iter().any(|s| in_stmt(s, name)))
            }
            Stmt::Block(v) | Stmt::Multi(v) => v.iter().any(|s| in_stmt(s, name)),
            _ => false,
        }
    }
    lib_section.iter().any(|s| in_stmt(s, name))
}

/// The whole knife-D move for one renamed class: called by the
/// census right after `rename_top_decl` pointed `lib_section[decl_idx]`
/// (an already-renamed ClassDecl) at `new`. `old` is the spelling
/// every baked artifact still carries.
#[allow(clippy::too_many_arguments)]
pub(super) fn rename_class_artifacts(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    decl_idx: usize,
    old: &str,
    new: &str,
    delta: &LibTableDelta,
    hidden: &HashSet<String>,
    hidden_inject: &mut HashSet<String>,
) {
    // `.name` reflection: the mangle is a compiler artifact, so the
    // class keeps reflecting its source spelling — the same
    // NamedEvaluation display table an anonymous class expression
    // uses (`class_globals` reads it for the `name` slot). The only
    // parse-time producers key on `__ClassExpr_<id>` synth names, so
    // a top-level decl never has a prior entry.
    let display = ast
        .class_expr_display_names
        .get(old)
        .cloned()
        .unwrap_or_else(|| old.to_string());
    ast.class_expr_display_names
        .entry(new.to_string())
        .or_insert(display);
    migrate_name_keyed_tables(ast, &lib_section[decl_idx], old, new, delta);
    migrate_exprid_tables(ast, lib_expr_offset, old, new);
    rewrite_member_spellings(&mut lib_section[decl_idx], old, new);
    rename_synth_decls(lib_section, old, new, hidden, hidden_inject);
    rewrite_arena_bakes(ast, lib_expr_offset, old, new);
    rewrite_anns(ast, lib_section, lib_expr_offset, old, new);
}

fn priv_prefix(class: &str) -> String {
    format!("__priv_{class}__")
}

fn swap_prefix(s: &str, old_pre: &str, new_pre: &str) -> Option<String> {
    s.strip_prefix(old_pre).map(|r| format!("{new_pre}{r}"))
}

/// Declaration-side `__priv_<C>__` member spellings on the renamed
/// class itself.
fn rewrite_member_spellings(decl: &mut Stmt, old: &str, new: &str) {
    let Stmt::ClassDecl {
        fields,
        ctor: _,
        methods,
        static_methods,
        static_init,
        ..
    } = (match decl {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => inner.as_mut(),
        other => other,
    })
    else {
        return;
    };
    let (pre_old, pre_new) = (priv_prefix(old), priv_prefix(new));
    for m in methods.iter_mut().chain(static_methods.iter_mut()) {
        if let Some(n) = swap_prefix(&m.name, &pre_old, &pre_new) {
            m.name = n;
        }
    }
    for (n, _) in fields.iter_mut() {
        if let Some(nn) = swap_prefix(n, &pre_old, &pre_new) {
            *n = nn;
        }
    }
    for si in static_init.iter_mut() {
        if let crate::ast::StaticInit::Field(f) = si
            && let Some(nn) = swap_prefix(&f.name, &pre_old, &pre_new)
        {
            f.name = nn;
        }
    }
}

/// Top-level parser synthetics that bake the class name into their
/// OWN decl name: `__ccmk_<C>_<n>` computed-key hoists and
/// `__cm_gen_<C>__<m>` generator forwarder fns. The census skips
/// them as census candidates (they follow their class, here).
fn rename_synth_decls(
    lib_section: &mut [Stmt],
    old: &str,
    new: &str,
    hidden: &HashSet<String>,
    hidden_inject: &mut HashSet<String>,
) {
    let pairs = [
        (format!("__ccmk_{old}_"), format!("__ccmk_{new}_")),
        (format!("__cm_gen_{old}__"), format!("__cm_gen_{new}__")),
    ];
    for s in lib_section.iter_mut() {
        let inner = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner.as_mut(),
            other => other,
        };
        if let Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } = inner {
            for (pre, pre_n) in &pairs {
                if let Some(nn) = swap_prefix(name, pre, pre_n) {
                    // The fn-table rows keyed on the derived spelling
                    // were copied by `migrate_name_keyed_tables`. The
                    // hidden-injection entry follows the new spelling
                    // whichever side of the class the census reached
                    // first (the skip arm inserts the OLD name when
                    // it runs before this rename, and can no longer
                    // match afterwards).
                    if hidden_inject.remove(name) || hidden.contains(name.as_str()) {
                        hidden_inject.insert(nn.clone());
                    }
                    *name = nn;
                    break;
                }
            }
        }
    }
}

/// Arena-side baked spellings in the lib slice: `New.class_name`
/// stays with the shared census rewrite (the mangles map); this walk
/// owns the PREFIX bakes — `__ccmk_` / `__cm_gen_` / `__cm_` /
/// `__sm_` idents (computed-key reads, forwarder callees, generator
/// super-call resolutions naming the parent), `__priv_<C>__` member
/// reference spellings, and the `#x in o` key string (recognized
/// structurally through its `__torajs_priv_in_op` call so an
/// ordinary user string is never touched).
fn rewrite_arena_bakes(ast: &mut Ast, lib_expr_offset: usize, old: &str, new: &str) {
    let in_op_keys: Vec<usize> = ast.exprs[lib_expr_offset..]
        .iter()
        .filter_map(|e| {
            let Expr::Call { callee, args } = e else {
                return None;
            };
            let Expr::Ident(cn) = &ast.exprs[callee.0 as usize] else {
                return None;
            };
            if cn != "__torajs_priv_in_op" {
                return None;
            }
            args.first().map(|a| a.0 as usize)
        })
        .collect();
    let ident_pairs = [
        (format!("__ccmk_{old}_"), format!("__ccmk_{new}_")),
        (format!("__cm_gen_{old}__"), format!("__cm_gen_{new}__")),
        (format!("__cm_{old}__"), format!("__cm_{new}__")),
        (format!("__sm_{old}__"), format!("__sm_{new}__")),
    ];
    let (pre_old, pre_new) = (priv_prefix(old), priv_prefix(new));
    for e in ast.exprs[lib_expr_offset..].iter_mut() {
        match e {
            Expr::Ident(n) => {
                for (pre, pre_n) in &ident_pairs {
                    if let Some(nn) = swap_prefix(n, pre, pre_n) {
                        *n = nn;
                        break;
                    }
                }
            }
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
                if let Some(nn) = swap_prefix(name, &pre_old, &pre_new) {
                    *name = nn;
                }
            }
            _ => {}
        }
    }
    for i in in_op_keys {
        if let Expr::String(s) = &mut ast.exprs[i]
            && let Some(nn) = swap_prefix(s, &pre_old, &pre_new)
        {
            *s = nn;
        }
    }
}

mod anns;
use anns::rewrite_anns;
mod tables;
pub(in crate::modules) use tables::{LibTableDelta, diff_class_tables, snapshot_class_tables};
use tables::{migrate_exprid_tables, migrate_name_keyed_tables};
