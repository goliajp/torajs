//! The relay shells of generic functions whose construction sites the
//! check never walked.
//!
//! A shell's body is checked at its construction site, and that walk
//! is where the shell's forwarding call to a generic target is
//! recorded as a call site, so the target gets its all-`any` instance
//! and the call is retargeted to it. When every site of the shell sits
//! in a generic body that is never instantiated (`export function
//! parse(s, o) { const dec = o?.decode || decode; ... }` with no call
//! of `parse`), the body was never walked, yet the shell still lowers
//! (it is shared, not owned by that body) and its call named a target
//! that has no lowered instance.
//!
//! A shell carries no captures, so its body checks the same at the top
//! level as at any site; such a shell is checked here, before the
//! worklist is seeded from the recorded call sites.

use crate::ast::{Ast, Expr, Stmt};
use crate::check::Checker;
use crate::ssa_lower_generics_monomorph::Generics;

pub(super) fn check_unvisited_generic_shells(c: &mut Checker, ast: &Ast, generics: &Generics) {
    let shells: Vec<String> = (ast.stmts.iter())
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. }
                if crate::ast::shell_target(name).is_some_and(|t| generics.contains_key(t))
                    && !c.closure_captures.contains_key(name) =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();
    for shell in shells {
        let site = (0..ast.exprs.len())
            .find(|&i| matches!(&ast.exprs[i], Expr::Closure { fn_name, .. } if *fn_name == shell));
        let Some(site) = site else { continue };
        let site = crate::ast::ExprId(site as u32);
        // a site the check walked typed the closure there
        if c.expr_types.contains_key(&site) {
            continue;
        }
        if let Err(e) =
            crate::check_type_of_fn::check_closure_body(c, ast, site, &shell, Vec::new())
        {
            crate::check::DiagPush::push_err(&mut c.errors, e);
        }
    }
}
