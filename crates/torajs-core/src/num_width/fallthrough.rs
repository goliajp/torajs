//! RFC 20260725-fallthrough-return — the fall-through table: which
//! functions can run off the end of their body, and therefore answer
//! `undefined` there (ES §10.2.1.4 step 11) rather than through a
//! `return`. The call site reads it to know a result may hold that
//! return width's sentinel.
//!
//! Split out of `mod.rs` when knife 4's arrow-binding alias pass pushed
//! that file past the 500-line limit.

use super::{Analysis, SlotKey};
use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashSet;

/// RFC 20260725-fallthrough-return knife 4 — an arrow function is
/// lifted to a `__closure_N` FnDecl before this analysis runs, so it
/// lands on the table under that synthetic name. The call site spells
/// it with the binding it was assigned to (`const h = (f) => …; h(x)`),
/// which would miss. Record the binding as an alias.
///
/// Same-named bindings merge conservatively, matching how `SlotKey`
/// treats them: naming one fall-through closure is enough for every
/// `h(...)` to take the sentinel-aware branch, which is the safe
/// direction (one predictable compare) rather than the silent one.
pub(super) fn alias_fallthrough_closures(ast: &Ast, out: &mut HashSet<String>) {
    fn walk(ast: &Ast, stmts: &[Stmt], out: &mut HashSet<String>) {
        for s in stmts {
            match s {
                Stmt::LetDecl { name, init, .. } => {
                    if let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
                        && out.contains(fn_name)
                    {
                        out.insert(name.clone());
                    }
                }
                Stmt::FnDecl { body, .. } | Stmt::Block(body) | Stmt::Multi(body) => {
                    walk(ast, body, out)
                }
                _ => {}
            }
        }
    }
    // One pass is enough for the direct shape; a binding aliased to
    // another binding (`const j = h`) would need a fixpoint and is not
    // covered — it reads as a plain Ident init, not a Closure.
    walk(ast, &ast.stmts, out);
}

/// RFC 20260725-fallthrough-return knives 1-2 — a body that can run
/// off its end answers `undefined` there (ES §10.2.1.4 step 11). Every
/// such function goes on the table, which the call site reads to know
/// a result may hold that answer's sentinel.
///
/// `number` additionally needs a WIDER slot to carry it: I64 has no
/// bit pattern to spare and F64 does, so seed the return slot and let
/// the fixpoint carry the width to every binding the result flows
/// into. Pointer-shaped returns need no seed — their slots already
/// decode three ways (NULL / sentinel / live cell).
pub(super) fn seed_fallthrough_return(
    a: &mut Analysis<'_>,
    rk: SlotKey,
    return_ann: &str,
    fn_name: &str,
    body: &[Stmt],
    out: &mut HashSet<String>,
) {
    if return_ann == "void" || crate::ast::body_always_terminates(body) {
        return;
    }
    out.insert(fn_name.to_string());
    if return_ann == "number" {
        a.seeds.push(rk);
    }
}
