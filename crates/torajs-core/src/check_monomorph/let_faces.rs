//! The checker's verdicts on fn-typed bindings that hold a value of
//! another face (`check_stmt_let_decl::fn_face_slot`), written onto the
//! owned AST the lowerer reads: an immutable binding's declaration takes
//! the annotation spelled in its value's face, a mutable one joins the
//! boxed-entry census by its declaration (and, declared at top level,
//! by its global name), a parameter by its function.

use crate::ast::{Ast, Stmt};
use crate::check::Checker;

pub(super) fn apply(c: &Checker, owned_ast: &mut Ast) {
    for (f, p) in &c.face_mismatch_params {
        let params = owned_ast.argv_boxed_params.entry(f.clone()).or_default();
        params.insert(p.clone());
    }
    if c.let_face_anns.is_empty() && c.face_mismatch_lets.is_empty() {
        return;
    }
    owned_ast
        .face_mismatch_lets
        .extend(c.face_mismatch_lets.iter().copied());
    // the flat view the lowerer's global registration walks: a
    // multi-declarator `let` is a global like a single one
    let globals: Vec<String> = crate::ast::toplevel_stmts_flat(owned_ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::LetDecl { name, init, .. } if c.face_mismatch_lets.contains(init) => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();
    owned_ast.face_mismatch_globals.extend(globals);
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
