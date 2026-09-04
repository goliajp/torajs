//! Splitting one block-nested `function` declaration into the TWO
//! bindings Annex B §B.3.3 gives it.
//!
//! Two passes rewrite a declaration in place into a `let` holding a
//! function expression, each for its own reason —
//! [`super::nested_fns_capture`] because the body reads an outer local,
//! [`super::fndecl_rebound`] because the name is assigned — and both
//! leave the same thing missing when the declaration sits in a block:
//! the var-scoped binding. This is that half, written once so the two
//! callers cannot drift apart on it.
//!
//! The block binding takes a name of its own and everything inside the
//! block that meant the declaration follows it; a `var` of the original
//! name is written where the declaration sits, which is where §B.3.3.1
//! step 3.a.ii puts the write. A reference outside the block means that
//! var binding and is left alone — it is not in `stmts`.
//!
//! The block binding is minted `mutable`: §B.3.3 makes an ordinary
//! let-like binding, and test262's `block-scoping` family assigns to it
//! from inside the declaration's own body and then checks that the var
//! binding still holds the function.

use super::annexb_fn_var::annexb_applies_at;
use super::free_vars::free_vars_of_body;
use super::nested_fns_idents::rewrite_idents_in_body;
use super::{Ast, Expr, Stmt};
use std::collections::HashMap;

/// `stmts[idx]` is a `LetDecl` a caller just rewrote a block-nested
/// declaration into. Give the name back to the var scope. The caller
/// owns the two questions this does not ask: whether the list is a
/// block at all, and whether an enclosing scope already binds the name
/// (§B.3.3.1 step 1.a.ii's early error, where the extension does not
/// apply).
pub(super) fn split_block_binding(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    idx: usize,
    name: &str,
    span: crate::lexer::Span,
    counter: &mut u32,
) {
    if !annexb_applies_at(ast, span) {
        return;
    }
    // A reference to the name BEFORE the declaration, in the same
    // block, cannot follow the rename: both callers leave the rewrite
    // where the declaration was, so the block binding does not exist
    // yet there — that is the hoisting boundary
    // `nested_fns_capture`'s own doc records. Such a block keeps the
    // shape it had, where the name means the var binding throughout;
    // of the two answers available it is the better one.
    // `{ let y = f(); function f() { y; } }` is the case: the spec
    // wants f callable (a block function is initialized on block
    // entry) and the ReferenceError to come from y's TDZ.
    if free_vars_of_body(ast, &[], &stmts[..idx])
        .iter()
        .any(|n| n == name)
    {
        return;
    }
    let mangled = format!("__blkfn_{name}_{counter}");
    *counter += 1;
    let map: HashMap<String, String> = HashMap::from([(name.to_string(), mangled.clone())]);
    rewrite_idents_in_body(ast, stmts, &map, true);
    let var_init = ast.add_expr(Expr::Ident(mangled.clone()));
    ast.set_expr_span(var_init, span);
    // The two callers leave different shapes behind — a single `let`,
    // or a `Multi` holding a hoisted `let` and the assignment that
    // initializes it (that pair is what makes a self-assigning
    // declaration legal). Both are handled the same way: every binding
    // of the name in the slot becomes the block binding, and the var
    // binding is appended. The ident rewrite above already moved the
    // assignment's target, since that is an expression.
    let taken = std::mem::replace(&mut stmts[idx], Stmt::Block(Vec::new()));
    let mut items = match taken {
        Stmt::Multi(v) => v,
        other => vec![other],
    };
    debug_assert!(
        items
            .iter()
            .any(|it| matches!(it, Stmt::LetDecl { name: n, .. } if n == name)),
        "the caller rewrote the declaration into a binding of its own name"
    );
    for it in items.iter_mut() {
        if let Stmt::LetDecl {
            name: n, mutable, ..
        } = it
        {
            if n == name {
                *n = mangled.clone();
                // §B.3.3 makes an ordinary let-like binding, and
                // test262's `block-scoping` family assigns to it from
                // inside the declaration's own body.
                *mutable = true;
            }
        }
    }
    items.push(Stmt::LetDecl {
        mutable: true,
        name: name.to_string(),
        type_ann: None,
        init: var_init,
        is_var: true,
    });
    // `Multi` shares the surrounding scope, so the block binding belongs
    // to the block and the `var` to the function or script around it.
    stmts[idx] = Stmt::Multi(items);
}
