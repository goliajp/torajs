//! A labelled function declaration is a function declaration.
//!
//! §14.13.4 evaluates `LabelledItem : FunctionDeclaration` as the
//! declaration itself (it answers empty), and every declaration-
//! instantiation question treats it as one: TopLevelVarScopedDeclarations
//! of a script or function body (§8.2.10) and LexicallyScopedDeclarations
//! of a block (§8.2.5) both reach through the label to the declaration.
//! The label can never be a target either — a `break` / `continue`
//! inside the body does not cross the function boundary (§14.9.1).
//!
//! So the wrapper carries nothing once the parse has judged it, and it
//! cost the declaration its hoisting: the lift passes read a `Labeled`
//! body as a branch position, so `l: function f() { return 4 }` at the
//! top of a script was evaluated in place and beat a later plain
//! `function f() { return 3 }` that should have won (§16.1.7 keeps the
//! last declaration).
//!
//! The unwrap runs after the whole parse, not inside it, because the
//! single-statement judges still need the chain: `if (c) l: function
//! f() {}` is a SyntaxError under every goal (§14.6.1 IsLabelledFunction)
//! while the bare `if (c) function f() {}` is Annex B §B.3.4. Every
//! labelled declaration that survives the parse therefore sits in a
//! StatementList, which is the one position this rewrite is right for.

use crate::ast::stmt_nested_lists::for_each_nested_vec_mut;
use crate::ast::{Ast, Expr, Stmt};

/// Unwrap every labelled function declaration this parse appended: the
/// statements from `stmt_offset` on and the function bodies in the
/// arena from `id_offset` on.
pub(super) fn unwrap_labelled_fn_decls(ast: &mut Ast, stmt_offset: usize, id_offset: usize) {
    let mut top = ast.stmts.split_off(stmt_offset);
    unwrap_list(&mut top);
    for s in top.iter_mut() {
        for_each_nested_vec_mut(s, &mut unwrap_list);
    }
    ast.stmts.append(&mut top);
    for e in ast.exprs[id_offset..].iter_mut() {
        if let Expr::ArrowFn { body, .. } = e {
            unwrap_list(body);
            for s in body.iter_mut() {
                for_each_nested_vec_mut(s, &mut unwrap_list);
            }
        }
    }
}

fn unwrap_list(list: &mut Vec<Stmt>) {
    for s in list.iter_mut() {
        if labels_fn_decl(s) {
            let mut inner = std::mem::replace(s, Stmt::Block(Vec::new()));
            while let Stmt::Labeled { body, .. } = inner {
                inner = *body;
            }
            *s = inner;
        }
    }
}

fn labels_fn_decl(s: &Stmt) -> bool {
    match s {
        Stmt::Labeled { body, .. } => matches!(**body, Stmt::FnDecl { .. }) || labels_fn_decl(body),
        _ => false,
    }
}
