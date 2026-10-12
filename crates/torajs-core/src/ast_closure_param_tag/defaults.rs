//! The default-value round of [`crate::ast_closure_param_tag`]'s
//! fixpoint.

use crate::ast::{Ast, Stmt};
use crate::ast_closure_param_tag::{is_closure_shaped, push_child_stmts};
use std::collections::{HashMap, HashSet};

/// r359 — a fn-typed param whose DEFAULT is closure-shaped receives
/// the cell through the call-site pad (`h2()` → `h2(a)`,
/// apply_default_args — which runs later in the pipeline), so the
/// argument round never sees it: the param kept __fn( and the padded
/// cell was dispatched as a raw code address (EXIT=138). One round of
/// [`crate::ast_closure_param_tag::tag_closure_arg_params`]'s
/// fixpoint; true when it marked a param.
pub(crate) fn mark_closure_defaults(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
    closure_idents: &HashSet<String>,
    ret_marked: &HashSet<String>,
    marked: &mut HashSet<(String, usize)>,
) -> bool {
    let mut changed = false;
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl { name, params, .. } = s
            && let Some(fps) = fn_params.get(name)
        {
            for (idx, _) in fps {
                if let Some(p) = params.get(*idx)
                    && let Some(d) = p.default
                    && is_closure_shaped(ast, d, closure_idents, ret_marked)
                    && marked.insert((name.clone(), *idx))
                {
                    changed = true;
                }
            }
        }
        push_child_stmts(s, &mut stack);
    }
    changed
}
