//! The reference side of a mangle: which lib statements rebind a
//! spelling, and the two rewrites that follow a renamed declaration.
//!
//! A lib that never rebinds the name gets the blind arena rewrite —
//! every `Ident` in the lib's slice is that top-level binding. A lib
//! that rebinds it somewhere (a fn param, a body `let`, an arrow)
//! gets the site rewrite instead: the free-variable walk reports the
//! sites where the reference resolves to the top level, and only
//! those move. Declining used to be the answer there, and a declined
//! hidden decl that also collided never landed at all — `export const
//! r = f(); function g(r) {…}` under an entry `const r` silently lost
//! `r`'s initializer, and a reader inside the lib bound to the
//! entry's `r`.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// Does any OTHER statement of the lib — or any arrow value in the
/// lib's arena slice — rebind `name`? True means the blind arena
/// rewrite is unsound for this spelling.
pub(in crate::modules) fn rebinds_elsewhere(
    ast: &Ast,
    lib_section: &[Stmt],
    lib_expr_offset: usize,
    decl_idx: usize,
    name: &str,
) -> bool {
    let stmt_rebinds = lib_section.iter().enumerate().any(|(j, s)| {
        if j == decl_idx {
            return false;
        }
        let inner = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner,
            other => other,
        };
        crate::ast::rebinds_in_stmt(inner, name)
    });
    if stmt_rebinds {
        return true;
    }
    ast.exprs[lib_expr_offset..].iter().any(|e| {
        matches!(e, Expr::ArrowFn { params, body, .. }
            if params.iter().any(|p| p.name == name)
                || body.iter().any(|s| crate::ast::rebinds_in_stmt(s, name)))
    })
}

/// Does another TOP-LEVEL statement of the lib declare `name` itself
/// (a second `function r`)? Two top-level declarations of one
/// spelling are one binding the later one wins; renaming only the
/// first would split it, so that shape keeps the decline.
pub(super) fn redeclared_at_top(lib_section: &[Stmt], decl_idx: usize, name: &str) -> bool {
    lib_section
        .iter()
        .enumerate()
        .any(|(j, s)| j != decl_idx && super::top_value_decl_name(s).is_some_and(|n| n == name))
}

/// Reference rewrite — every lib expression lives in the appended
/// arena slice; a bare Ident there referencing a mangled top-level
/// binding follows it (self-references included). `New` names its
/// class in a String field the Ident arm can't see (knife D — and
/// the parser also mints `Expr::New` for `new f()` over a plain fn,
/// so the arm applies to every mangle, not just classes).
pub(super) fn rewrite_lib_refs(
    ast: &mut Ast,
    lib_expr_offset: usize,
    mangles: &HashMap<String, String>,
) {
    for e in ast.exprs[lib_expr_offset..].iter_mut() {
        match e {
            Expr::Ident(n) => {
                if let Some(m) = mangles.get(n) {
                    *n = m.clone();
                }
            }
            Expr::New { class_name, .. } => {
                if let Some(m) = mangles.get(class_name) {
                    *class_name = m.clone();
                }
            }
            _ => {}
        }
    }
}

/// The site rewrite for a spelling the lib rebinds somewhere: only a
/// reference no enclosing frame binds follows the mangle. The lib's
/// statements are walked as they stand after the decl rename, so the
/// renamed declaration's own body reads its old spelling free and
/// moves with the rest. A class's computed keys live only in the
/// side tables; the lib's share of them (arena ids past the offset)
/// walks too.
pub(super) fn rewrite_lib_ref_sites(
    ast: &mut Ast,
    lib_section: &[Stmt],
    lib_expr_offset: usize,
    mangles: &HashMap<String, String>,
) {
    let in_lib = |e: &ExprId| e.0 as usize >= lib_expr_offset;
    let side_stmts: Vec<Stmt> = ast
        .class_computed_keys
        .values()
        .filter(|v| in_lib(v))
        .map(|v| Stmt::Expr(*v))
        .chain(
            ast.class_computed_static_fields
                .iter()
                .filter(|r| in_lib(&r.2))
                .map(|r| Stmt::Expr(r.2)),
        )
        .collect();
    let mut rewrites: Vec<(ExprId, &str)> = Vec::new();
    for (name, mangled) in mangles {
        for s in lib_section.iter().chain(side_stmts.iter()) {
            for eid in crate::ast::top_ref_sites(ast, s, name, &[]) {
                rewrites.push((eid, mangled.as_str()));
            }
        }
    }
    for (eid, m) in rewrites {
        match &mut ast.exprs[eid.0 as usize] {
            Expr::Ident(n) => *n = m.to_string(),
            Expr::New { class_name, .. } => *class_name = m.to_string(),
            _ => {}
        }
    }
}
