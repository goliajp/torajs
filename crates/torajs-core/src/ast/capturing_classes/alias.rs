//! The user's own alias binding of a lane-claimed class — `const C =
//! class { … }` is, by the time the lane has α-renamed the class,
//! `const C = <minted>`.
//!
//! Receiver promotion pairs a binding to its uses by name under the
//! same only-declared-once rule the class binding was renamed for,
//! and `C` is any user spelling: two blocks each writing `const C =
//! class { … }` took each other down one level up from where the
//! class rename fixed it (the alias safety walk counts declarations
//! program-wide). So the alias is minted unique too. Its uses live
//! under the statement list that holds the class — the alias is
//! block-scoped exactly where the parse_stmt wrapper spliced the
//! decl — which is the same subtree the class rename already covers.

use super::super::{Ast, Expr, Stmt};

/// Rename every direct alias of `minted_class` (a LetDecl whose init
/// is exactly that Ident) to a program-unique `__cca<N>_<alias>`.
pub(super) fn mint_unique_aliases(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    minted_class: &str,
    counter: &mut u32,
) {
    let alias_renames: Vec<(usize, String, String)> = stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::LetDecl {
                name: alias, init, ..
            } if matches!(ast.get_expr(*init), Expr::Ident(n) if *n == minted_class) => {
                Some((i, alias.clone()))
            }
            _ => None,
        })
        .map(|(i, alias)| {
            let minted = format!("__cca{}_{}", *counter, alias);
            *counter += 1;
            (i, alias, minted)
        })
        .collect();
    for (i, a_old, a_new) in alias_renames {
        // The walker renames REFERENCES only — the declaration's own
        // `name` field is deliberately outside it (the class rename
        // must not swallow a same-named `let`) — so the alias decl
        // moves by hand.
        super::super::hoist_nested_classes_rename::rename_in_stmts(ast, stmts, &a_old, &a_new);
        if let Stmt::LetDecl { name, .. } = &mut stmts[i] {
            *name = a_new;
        }
    }
}
