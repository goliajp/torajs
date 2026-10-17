//! The return types the compiler inferred, read again over a retyped
//! program. A plain function's return type is inferred from its body
//! before any check round runs; a round that retypes what the body
//! returns (a binding annotated `any`, an object literal field spelled
//! `as any`, a callee that now returns `any`) changes the layout of the
//! value the function hands back, and the inferred annotation has to
//! follow it, or the caller reads the value through the old layout.

use std::collections::HashMap;

use crate::ast::{Ast, Stmt, body_always_terminates, body_has_value_return, infer_return_ann};

/// Infer again, over `retyped` (a clone of `ast` whose statements are
/// retyped in place), the return type of each function whose return
/// type was inferred. Where the same inference over `ast` (the program
/// the annotation was inferred from) still answers the annotation, and
/// over `retyped` answers something else, the retyping changed it, and
/// `retyped` takes the new answer. A body the inference cannot type any
/// more returns `any`, as an untypeable body does when it is first
/// inferred. Functions are read in source order, so a call to an
/// earlier function reads its new answer (a type variable is not
/// published, as at the first inference).
pub(super) fn reinfer_returns(ast: &Ast, retyped: &mut Ast) {
    let mut sigs_before: HashMap<String, String> = HashMap::new();
    let mut sigs_after: HashMap<String, String> = HashMap::new();
    let mut changed = false;
    for i in 0..ast.stmts.len() {
        let Stmt::FnDecl {
            name,
            params,
            return_type: Some(rt),
            body,
            span,
            ..
        } = &ast.stmts[i]
        else {
            continue;
        };
        let Stmt::FnDecl {
            params: r_params,
            return_type: Some(r_rt),
            body: r_body,
            ..
        } = &retyped.stmts[i]
        else {
            continue;
        };
        let mut new_rt = r_rt.clone();
        if r_rt != "any"
            && inferred(ast, name, rt, body, *span)
            && let Some(old) = infer_return_ann(&ast.exprs, body, params, &sigs_before)
            && old == *rt
        {
            let new = infer_return_ann(&retyped.exprs, r_body, r_params, &sigs_after);
            if new.as_ref() != Some(&old) {
                new_rt = new.unwrap_or_else(|| "any".to_string());
                changed = true;
            }
        }
        // the inference reads a closure's whole `__fn(..)` signature under
        // its name, never its bare return type
        let publish = !crate::ast_desugar_implicit_generics::is_synth_closure_name(name);
        if publish && !rt.starts_with("__T") {
            sigs_before.insert(name.clone(), rt.clone());
        }
        if publish && !new_rt.starts_with("__T") {
            sigs_after.insert(name.clone(), new_rt.clone());
        }
        if let Stmt::FnDecl { return_type, .. } = &mut retyped.stmts[i] {
            *return_type = Some(new_rt);
        }
    }
    // a method that answers otherwise now leaves the rows of its slot
    // disagreeing, which the slot's join settles as it did at first
    if changed {
        crate::ast::join_vtable_slot_returns(retyped);
    }
}

/// Whether `rt`, the return type of the function `name`, is the one
/// the compiler inferred from its body: the program did not write it
/// and no call position seeded it, the body ends in value returns only
/// (a body that can fall through returns `any`), and the function is
/// not a vtable slot's dispatcher, which answers what its rows answer.
fn inferred(ast: &Ast, name: &str, rt: &str, body: &[Stmt], span: crate::lexer::Span) -> bool {
    !(name.starts_with("__dispatch_")
        || name.starts_with("__new_")
        || rt == "any"
        || ast.fn_ret_written(name, span)
        || ast.contextual_ret_fns.contains(name)
        || !body_has_value_return(body)
        || !body_always_terminates(body))
}
