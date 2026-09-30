//! The regex literal a bare name stands for at every one of its reads:
//! a top-level immutable binding initialised with the literal, when
//! nothing else in the program declares that name. A call that names
//! such a binding knows the pattern as well as one that spells it.

use super::fnexpr_this_names::name_shadowed_elsewhere;
use super::stmt_nested_lists::for_each_nested_list;
use super::{Ast, Expr, Stmt, toplevel_stmts_flat};

pub(crate) fn toplevel_regex_literal<'a>(ast: &'a Ast, name: &str) -> Option<(&'a str, &'a str)> {
    let mut found = None;
    let mut decls = 0;
    for s in toplevel_stmts_flat(ast) {
        match s {
            Stmt::LetDecl {
                name: dn,
                init,
                mutable,
                is_var,
                ..
            } if dn == name => {
                decls += 1;
                if !*mutable
                    && !*is_var
                    && let Expr::Regex { pattern, flags } = ast.get_expr(*init)
                {
                    found = Some((pattern.as_str(), flags.as_str()));
                }
            }
            Stmt::FnDecl { name: dn, .. } | Stmt::ClassDecl { name: dn, .. } if dn == name => {
                decls += 1;
            }
            _ => {}
        }
    }
    if decls != 1 || name_shadowed_elsewhere(&ast.stmts, name) || declared_nested(&ast.stmts, name)
    {
        return None;
    }
    found
}

/// A lexical declaration of `name` inside any nested statement list —
/// a function body, a block, a loop — which the name's reads there
/// belong to.
fn declared_nested(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|s| {
        let mut hit = false;
        for_each_nested_list(s, &mut |inner| {
            hit = hit
                || inner
                    .iter()
                    .any(|i| matches!(i, Stmt::LetDecl { name: dn, .. } if dn == name))
                || declared_nested(inner, name);
        });
        hit
    })
}
