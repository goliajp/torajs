//! The checker's verdicts on fn-typed bindings that hold a value of
//! another face (`check_stmt_let_decl::fn_face_slot`), written onto the
//! owned AST the lowerer reads: an immutable binding's declaration takes
//! the annotation spelled in its value's face, a mutable one joins the
//! boxed-entry census.

use crate::ast::{Ast, Stmt};
use crate::check::Checker;

pub(super) fn apply(c: &Checker, owned_ast: &mut Ast) {
    owned_ast
        .fnsig_mismatch_bindings
        .extend(c.face_mismatch_bindings.iter().cloned());
    if c.let_face_anns.is_empty() {
        return;
    }
    let mut stack: Vec<&mut Stmt> = owned_ast.stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl { type_ann, init, .. } = s
            && let Some(face) = c.let_face_anns.get(init)
        {
            *type_ann = Some(face.clone());
        }
        crate::ast_closure_param_tag_retag::push_child_stmts_mut(s, &mut stack);
    }
}
