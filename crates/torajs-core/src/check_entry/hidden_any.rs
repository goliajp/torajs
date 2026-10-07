//! The check rounds that retype bindings `any`.
//!
//! A binding that may store a value of another kind
//! (`num_width::hidden_any`: an element of a typed array whose alias
//! escaped into the `any` world, or of an `any[]`) is annotated `any`,
//! and the program checked again, until no round asks for another. A
//! program that asks for none is checked once.

use std::collections::HashSet;

use super::{Round, check_round};
use crate::ast::{Ast, ExprId, Stmt};

/// Run rounds from `first`, the round of `ast` as written.
pub(super) fn settle(ast: &Ast, first: Round) -> Result<Round, String> {
    let mut round = first;
    let mut asked: HashSet<ExprId> = HashSet::new();
    let mut retyped: Option<Ast> = None;
    loop {
        let widths = &round.0.5.widths;
        // ids past `ast.exprs` belong to specializations the mono pass
        // clones; they name no declaration of `ast`
        let fresh: Vec<ExprId> = (widths.hidden_any_inits.iter().copied())
            .filter(|e| (e.0 as usize) < ast.exprs.len() && !asked.contains(e))
            .collect();
        if fresh.is_empty() {
            return Ok(round);
        }
        asked.extend(fresh);
        let r = retyped.get_or_insert_with(|| ast.clone());
        retype_any(&mut r.stmts, &asked);
        round = check_round(r)?;
    }
}

/// Annotate `any` every `let` / `const` whose initializer is in
/// `inits`. Such a binding can be handed a value of any kind at run
/// time, and §13.15.2 stores it as is; the annotation gives it the
/// boxed slot every `any` binding has, in the checker and the lowering
/// alike.
fn retype_any(stmts: &mut [Stmt], inits: &HashSet<ExprId>) {
    let mut stack: Vec<&mut Stmt> = stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl { init, type_ann, .. } = s
            && inits.contains(init)
        {
            *type_ann = Some("any".to_string());
        }
        crate::ast_closure_param_tag_retag::push_child_stmts_mut(s, &mut stack);
    }
}
