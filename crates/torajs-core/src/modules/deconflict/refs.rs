//! The reference side of a mangle: which lib statements rebind a
//! spelling, and the arena rewrite that follows a renamed declaration.

use crate::ast::{Ast, Expr, Stmt};
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
