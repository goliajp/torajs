//! The twins of the callbacks a typed array hands its elements to once
//! that array has escaped into the `any` world.
//!
//! `const u: any = d; u[1] = "s"; d.map(v => typeof v)`: the width
//! analysis puts `d` in an alias class that reaches an `any` slot, so it
//! lowers as an `Arr<Any>` and its elements are whatever the alias
//! stored, while the checker still reads `d` as `number[]` and types
//! `v` as `number`. The typed callback sites (`typed_sites.rs`) would
//! leave such a callback without a twin; here it gets one, and the
//! iteration lowering hands an element the callback's slot cannot hold
//! to the callback's boxed entry, whose tag test calls the twin
//! (§10.2.1.3 binds an argument as it is: `typeof v` on "s" is
//! "string"). A function expression reading `this` keeps its receiver
//! head on the twin. The analysis runs over the AST the twins are appended
//! to, so the driver (`check_monomorph::finish`) asks again after every
//! mint until no escaped callback lacks one.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{closure, is_twin, twin_name};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Checker;
use crate::check_monomorph::WorkItem;
use crate::num_width::WidthTable;
use crate::ssa_lower_generics_monomorph::Generics;

/// Mint the twin of every lifted closure passed to an iteration over an
/// escaped array that has none yet. True when one was minted.
#[allow(clippy::too_many_arguments)]
pub(crate) fn mint_escaped_callback_twins(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    retired: &HashSet<String>,
    widths: &WidthTable,
) -> bool {
    let names = escaped_callbacks(c, owned_ast, widths);
    if names.is_empty() {
        return false;
    }
    closure::mint(
        c,
        owned_ast,
        generics,
        cache,
        worklist,
        call_retargets,
        mono_decls,
        retired,
        names,
        true,
    )
}

/// The lifted closures, in arena order, passed as the callback of an
/// iteration whose array escapes, without a twin.
fn escaped_callbacks(c: &Checker, ast: &Ast, widths: &WidthTable) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        let (recv, cb) = if name == "from" {
            (args.first(), args.get(1))
        } else {
            (Some(obj), args.first())
        };
        let (Some(&recv), Some(&cb)) = (recv, cb) else {
            continue;
        };
        if !widths.hof_receiver_escapes(recv) {
            continue;
        }
        if let Expr::Closure { fn_name, .. } = ast.get_expr(cb)
            && !is_twin(fn_name)
            && !c.globals.contains_key(&twin_name(fn_name))
            && !out.contains(fn_name)
        {
            out.push(fn_name.clone());
        }
    }
    out
}
