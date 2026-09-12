//! A write to a top-level `const` from inside a function is a runtime
//! TypeError (rotation 636).
//!
//! §9.1.1.1.5 SetMutableBinding on an immutable binding throws a
//! TypeError — in strict AND sloppy code, `const` bindings being
//! strict-immutable — after the right-hand side has been evaluated
//! (§13.15.2 PutValue follows GetValue of the rref). The checker
//! refuses the same-scope spelling at compile time (`const c = 3;
//! c = 4` is a type error, which is the TypeScript answer and the
//! better one for a typed program); a write from a NESTED function
//! got past it and died in the lowering ("assign to unknown ident"),
//! since an inlined-literal `const` has no slot for a store to land
//! in. test262 `global-code/decl-lex` asserts the throw.
//!
//! The rewrite is the readonly-global lane's — `(rhs, throw)` for an
//! assignment, the throw alone for `c++` — applied through the
//! scope-aware collector the generator self-name pass uses: a nested
//! function, arrow, block, for-of head or catch clause that rebinds
//! the name owns the writes inside it.
//!
//! Runs after the sloppy implicit-global synthesis, before anything
//! lifts a function out of its scope.

use super::{Ast, Stmt};

pub fn rewrite_const_writes_in_functions(ast: &mut Ast) {
    let consts: Vec<String> = super::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::LetDecl {
                mutable: false,
                is_var: false,
                name,
                ..
            } => Some(name.clone()),
            _ => None,
        })
        .collect();
    if consts.is_empty() {
        return;
    }
    let stmts = std::mem::take(&mut ast.stmts);
    for name in &consts {
        super::hoist_gen_fn_exprs_self_write::rewrite_writes_inside_functions(ast, &stmts, name);
    }
    ast.stmts = stmts;
}
