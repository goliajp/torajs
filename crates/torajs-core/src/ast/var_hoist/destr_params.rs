//! The bindings a destructuring parameter introduces, which the parser
//! spells as `let`s at the head of the function body
//! (`Ast::param_destr_inits`). They are parameters all the same
//! (§10.2.11 step 21 creates every parameter name as a mutable
//! binding), so a `var` of the same name re-declares them (§15.2.1
//! sets parameter names against the body's lexical names only) and
//! the body may write them.

use crate::ast::{Ast, Stmt};

/// The names `body`'s destructuring parameters bind.
pub(super) fn names(ast: &Ast, body: &[Stmt]) -> Vec<String> {
    body.iter()
        .filter_map(|s| match s {
            Stmt::LetDecl { name, init, .. } if ast.param_destr_inits.contains(init) => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect()
}

/// Make each such binding the body writes (an assignment, or a `var`
/// initializer the hoist has turned into one) a mutable `let`.
pub(super) fn make_written_mutable(ast: &Ast, body: &mut [Stmt]) {
    let written = crate::ast::nested_fns_top::writes::scope_writes(ast, body);
    for s in body.iter_mut() {
        if let Stmt::LetDecl {
            name,
            init,
            mutable,
            ..
        } = s
            && ast.param_destr_inits.contains(init)
            && written.contains(name)
        {
            *mutable = true;
        }
    }
}
