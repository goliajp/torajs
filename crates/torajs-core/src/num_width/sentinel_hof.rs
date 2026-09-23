//! The shapes where a sentinel and the parameter it reaches are not
//! the same argument of the same call.
//!
//! [`super::sentinel_tables::collect_undef_sentinel_params`] answers
//! the positional case: argument `i` of a direct call names parameter
//! `i` of the callee. A higher-order call does not work that way — the
//! value comes off the receiver or off another argument, and the
//! parameter it lands in belongs to a callback. One arm per shape,
//! because there is no rule covering them all.
//!
//! Split out of the tables module when the reduce arm brought it to
//! the 500-line edge; that module owns the name sets and the predicate
//! over them, this one owns the call shapes that feed them.

use super::Analysis;
use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashSet;

use super::sentinel_tables::{SentinelTables, is_sentinel_source};

/// `xs.reduce(cb, seed)` — the seed lands in `cb`'s FIRST parameter,
/// the accumulator, and §23.1.3.24 hands it over untouched. So a
/// sentinel-bearing seed makes the accumulator sentinel-capable,
/// exactly as a sentinel-bearing argument makes a parameter so above.
///
/// The positional rule up there cannot express this: the sentinel
/// source and the parameter it reaches are in different arguments of
/// different calls. Without it `[1].reduce((acc, x) => …, zs[9])`
/// reported `typeof acc` "number" for an `undefined`.
///
/// Only the seed. The element parameter is the receiver's business,
/// which this pass does not track — see the HOF families still open.
pub(super) fn collect_reduce_seed(
    a: &Analysis<'_>,
    t: &SentinelTables,
    callee: crate::ast::ExprId,
    args: &[crate::ast::ExprId],
    lifted: &std::collections::HashMap<String, String>,
    out: &mut HashSet<(String, String)>,
) {
    let Expr::Member { name, .. } = a.ast.get_expr(callee) else {
        return;
    };
    if !matches!(name.as_str(), "reduce" | "reduceRight") {
        return;
    }
    let (Some(cb), Some(seed)) = (args.first(), args.get(1)) else {
        return;
    };
    if !is_sentinel_source(a, *seed, t) {
        return;
    }
    let fn_name = match a.ast.get_expr(*cb) {
        Expr::Closure { fn_name, .. } => fn_name.clone(),
        Expr::Ident(n) => lifted.get(n).cloned().unwrap_or_else(|| n.clone()),
        _ => return,
    };
    if let Some(p) = a.user_params(&fn_name).first() {
        out.insert((fn_name.clone(), p.clone()));
    }
}

/// Binding name → the `__closure_N` FnDecl an arrow assigned to it was
/// lifted into, following plain re-bindings (`const j = h`) to a
/// fixpoint so a chain declared out of walk order still resolves.
/// The alias direction the fall-through table needs is the reverse of
/// this one, which is why the two are separate walks.
pub(super) fn lifted_closure_names(ast: &Ast) -> std::collections::HashMap<String, String> {
    fn walk(
        ast: &Ast,
        stmts: &[Stmt],
        out: &mut std::collections::HashMap<String, String>,
        grew: &mut bool,
    ) {
        for s in stmts {
            match s {
                Stmt::LetDecl { name, init, .. } => {
                    if out.contains_key(name) {
                        continue;
                    }
                    let target = match ast.get_expr(*init) {
                        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
                        Expr::Ident(n) => out.get(n).cloned(),
                        _ => None,
                    };
                    if let Some(t) = target {
                        out.insert(name.clone(), t);
                        *grew = true;
                    }
                }
                Stmt::FnDecl { body, .. } | Stmt::Block(body) | Stmt::Multi(body) => {
                    walk(ast, body, out, grew)
                }
                _ => {}
            }
        }
    }
    let mut out = std::collections::HashMap::new();
    loop {
        let mut grew = false;
        walk(ast, &ast.stmts, &mut out, &mut grew);
        if !grew {
            break;
        }
    }
    out
}
