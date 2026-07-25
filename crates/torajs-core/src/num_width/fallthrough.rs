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
/// A binding aliased to another binding (`const j = h`) rides the same
/// rule — it reads as a plain Ident init — so the walk runs to a
/// fixpoint rather than once, which also covers a chain declared out of
/// order relative to the walk.
///
/// Same-named bindings merge conservatively, matching how `SlotKey`
/// treats them: naming one fall-through closure is enough for every
/// `h(...)` to take the sentinel-aware branch, which is the safe
/// direction (one predictable compare) rather than the silent one.
pub(super) fn alias_fallthrough_closures(ast: &Ast, out: &mut HashSet<String>) {
    fn walk(ast: &Ast, stmts: &[Stmt], out: &mut HashSet<String>, grew: &mut bool) {
        for s in stmts {
            match s {
                Stmt::LetDecl { name, init, .. } => {
                    if out.contains(name) {
                        continue;
                    }
                    let aliases_fallthrough = match ast.get_expr(*init) {
                        Expr::Closure { fn_name, .. } => out.contains(fn_name),
                        Expr::Ident(n) => out.contains(n),
                        _ => false,
                    };
                    if aliases_fallthrough {
                        out.insert(name.clone());
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
    loop {
        let mut grew = false;
        walk(ast, &ast.stmts, out, &mut grew);
        if !grew {
            break;
        }
    }
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
