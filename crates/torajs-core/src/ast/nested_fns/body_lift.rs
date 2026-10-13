//! The lift of a function body's own nested declarations.

use super::super::annexb_fn_var::{LiftCtx, LiftMode};
use super::super::sloppy_this_prologue::directive_prologue_len;
use super::super::{Expr, Stmt};
use super::collect_nested_fns_to_lift;
use std::collections::{HashMap, HashSet};

/// [`collect_nested_fns_to_lift`] over a function body. The
/// declarations the lift mints at the head go below the directive
/// prologue, which has to stay the body's head.
pub(in crate::ast) fn collect_fn_body_lifts(
    exprs: &[Expr],
    body: &mut Vec<Stmt>,
    parent: &str,
    renames: &mut HashMap<String, String>,
    branch_scoped: &mut HashSet<String>,
    lifted: &mut Vec<Stmt>,
    ctx: &mut LiftCtx,
) {
    let prologue: Vec<Stmt> = body.drain(..directive_prologue_len(body, exprs)).collect();
    let mode = LiftMode {
        in_fn_body: true,
        nested: false,
    };
    collect_nested_fns_to_lift(body, parent, renames, branch_scoped, mode, lifted, ctx);
    body.splice(0..0, prologue);
}
