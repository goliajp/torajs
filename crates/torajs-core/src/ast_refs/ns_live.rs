use crate::ast::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet};

/// The top-level bindings a namespace object literal refers to
/// (`ast.ns_binding_fields`) whose fields lower to cells over their
/// global slots rather than plain copies: the MUTABLE ones — `let` /
/// `var` declarations, which a module may reassign after the object
/// is built — and the ones a top-level literal names before their
/// declaration (the entry's own namespace, built at the program's
/// head over bindings the entry declares further down: a copy would
/// read the dead zone). An immutable export declared before its
/// literal never changes, so its field stays a plain copy.
pub fn ns_live_exports(ast: &Ast) -> HashSet<String> {
    let stmts = crate::ast::toplevel_stmts_flat(ast);
    let mut decls: HashMap<&str, (usize, bool)> = HashMap::new();
    for (i, s) in stmts.iter().enumerate() {
        if let Stmt::LetDecl {
            name,
            mutable,
            is_var,
            ..
        } = s
        {
            decls.insert(name.as_str(), (i, *mutable || *is_var));
        }
    }
    let mut live: HashSet<String> = ast
        .ns_binding_fields
        .iter()
        .filter_map(|eid| match ast.get_expr(*eid) {
            Expr::Ident(n) if decls.get(n.as_str()).is_some_and(|d| d.1) => Some(n.clone()),
            _ => None,
        })
        .collect();
    for (i, s) in stmts.iter().enumerate() {
        let Stmt::LetDecl { name, init, .. } = s else {
            continue;
        };
        if !ast.namespace_bindings.contains_key(name) {
            continue;
        }
        let Expr::ObjectLit { fields } = ast.get_expr(*init) else {
            continue;
        };
        for (_, v) in fields {
            if let Expr::Ident(n) = ast.get_expr(*v)
                && ast.ns_binding_fields.contains(v)
                && decls.get(n.as_str()).is_some_and(|d| d.0 > i)
            {
                live.insert(n.clone());
            }
        }
    }
    live
}
