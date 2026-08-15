//! Program-wide `ClassDecl` name census (406-02) — a computed STATIC
//! field's side-table rows match by class NAME and leave no trace on
//! the class itself, so the capturing lane may install them only when
//! the name provably has one owner. Split from
//! `hoist_nested_classes.rs` at the file-size cap; the walk mirrors
//! `walk_child`'s container coverage, class bodies included (a
//! same-named class may hide inside a method body).

use super::*;

/// 405-01 face 2 — the REAL classes a capturing subclass may
/// `extends`: top-level, non-generic, non-abstract, name-unique
/// program-wide (the lane resolves the parent by NAME; a shadowed
/// spelling could link the wrong class — the silent direction),
/// and whose whole `extends` chain is made of such classes. The
/// chain condition (rotation 408, replacing the root-only admit)
/// is what keeps the ctor-twin mint sound: a derived real class's
/// ctor carries a `super(…)` form, which the mint rewrites into a
/// direct `__ctorany_<parent>` call — legal only when the parent
/// is itself admitted (its twin provably mints too). Fills
/// `ast.top_root_real_classes`; moved here from the hoist driver at
/// the file-size cap (rotation 409).
pub(super) fn admit_top_root_real_classes(
    ast: &mut Ast,
    name_counts: &std::collections::HashMap<String, u32>,
) {
    let mut cand: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for s in &ast.stmts {
        let inner = if let Stmt::ExportDecl {
            inner: Some(inner), ..
        } = s
        {
            inner.as_ref()
        } else {
            s
        };
        if let Stmt::ClassDecl {
            name,
            parent,
            type_params,
            is_abstract,
            ..
        } = inner
            && type_params.is_empty()
            && !is_abstract
            && name_counts.get(name).copied() == Some(1)
        {
            // A non-Ident heritage expression can make the class
            // neither a ROOT nor a statically-named chain link — keep
            // it out of the table entirely (mapping it to None would
            // mis-admit it as a root).
            let pn = ast.parent_ident_name(*parent).map(str::to_string);
            if parent.is_none() || pn.is_some() {
                cand.insert(name.clone(), pn);
            }
        }
    }
    for name in cand.keys() {
        // Walk the heritage chain; every link must be a candidate.
        // Legal programs cannot cycle (extends is TDZ-gated), but a
        // bounded walk keeps a malformed tree from hanging the pass.
        let mut cur = name.as_str();
        let mut admitted = false;
        for _ in 0..=cand.len() {
            match cand.get(cur) {
                Some(None) => {
                    admitted = true;
                    break;
                }
                Some(Some(p)) if cand.contains_key(p) => cur = p.as_str(),
                _ => break,
            }
        }
        if admitted {
            ast.top_root_real_classes.insert(name.clone());
        }
    }
}

pub(super) fn count_class_decl_names(s: &Stmt, out: &mut std::collections::HashMap<String, u32>) {
    if let Stmt::ClassDecl {
        name,
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    {
        *out.entry(name.clone()).or_insert(0) += 1;
        if let Some(c) = ctor {
            for cs in &c.body {
                count_class_decl_names(cs, out);
            }
        }
        for m in methods.iter().chain(static_methods.iter()) {
            for ms in &m.body {
                count_class_decl_names(ms, out);
            }
        }
        for si in static_init {
            if let StaticInit::Block(v) = si {
                for vs in v {
                    count_class_decl_names(vs, out);
                }
            }
        }
        return;
    }
    match s {
        Stmt::FnDecl { body, .. } | Stmt::Block(body) | Stmt::Multi(body) => {
            for cs in body {
                count_class_decl_names(cs, out);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            count_class_decl_names(then_branch, out);
            if let Some(eb) = else_branch {
                count_class_decl_names(eb, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => count_class_decl_names(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                count_class_decl_names(i, out);
            }
            count_class_decl_names(body, out);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for cs in &c.body {
                    count_class_decl_names(cs, out);
                }
            }
            if let Some(d) = default {
                for cs in d {
                    count_class_decl_names(cs, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for cs in body.iter().chain(catch_body.iter()) {
                count_class_decl_names(cs, out);
            }
            if let Some(fb) = finally_body {
                for cs in fb {
                    count_class_decl_names(cs, out);
                }
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => count_class_decl_names(inner, out),
        _ => {}
    }
}
