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
///
/// A body can also hand the sentinel back on purpose — `return
/// xs.find(...)` passes along whatever a miss answered. The call site
/// has to route the same way either way, so both reasons put the
/// function on one table.
pub(super) fn seed_fallthrough_return(
    a: &mut Analysis<'_>,
    rk: SlotKey,
    return_ann: &str,
    fn_name: &str,
    body: &[Stmt],
    out: &mut HashSet<String>,
) {
    if return_ann == "void" {
        return;
    }
    let falls_through = !crate::ast::body_always_terminates(body);
    if !falls_through && !body_returns_sentinel(a, body) {
        return;
    }
    out.insert(fn_name.to_string());
    if return_ann == "number" && falls_through {
        a.seeds.push(rk);
    }
}

/// True when some `return` in `body` hands back a value that may be
/// the `undefined` answer: an index read past the end, a `find` /
/// `findLast` / `at` miss, or a `pop` / `shift` on an empty array.
/// Those already put their width's sentinel in the slot; without this
/// the caller reads it as a plain value and prints NaN (or answers
/// `typeof` "string" for the immortal cell).
///
/// A value parked in a local first (`const m = xs.find(...); return m`)
/// counts too — the binding is recorded on the way past, mirroring how
/// the in-function consumers track the same shape through
/// `undefable_f64_lets` / `nullable_str_lets`.
///
/// Receiver-type-agnostic on purpose — the shape alone is the gate, and
/// being on the table only costs one predictable compare at the call
/// site.
fn body_returns_sentinel(a: &Analysis<'_>, body: &[Stmt]) -> bool {
    fn is_sentinel_source(a: &Analysis<'_>, eid: crate::ast::ExprId) -> bool {
        match a.ast.get_expr(eid) {
            // Reading past the end answers `undefined` (ES §10.4.2.1)
            // exactly as a miss does, and `xs.at(i)` is on the list
            // right below only because it takes that same exit under
            // another spelling. Handing the read straight back left the
            // caller reading the sentinel as a plain value and printing
            // NaN, while the identical read at the call site itself has
            // always answered `undefined`.
            Expr::Index { .. } | Expr::OptIndex { .. } => true,
            Expr::Call { callee, .. } => matches!(
                a.ast.get_expr(*callee),
                Expr::Member { name, .. }
                    if matches!(name.as_str(), "find" | "findLast" | "at" | "pop" | "shift")
            ),
            _ => false,
        }
    }
    fn walk(a: &Analysis<'_>, stmts: &[Stmt], lets: &mut HashSet<String>) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::LetDecl { name, init, .. } => {
                if is_sentinel_source(a, *init) {
                    lets.insert(name.clone());
                }
                false
            }
            Stmt::Return(Some(eid)) => {
                is_sentinel_source(a, *eid)
                    || matches!(a.ast.get_expr(*eid), Expr::Ident(n) if lets.contains(n))
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => walk(a, inner, lets),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk(a, std::slice::from_ref(then_branch), lets)
                    || else_branch
                        .as_deref()
                        .is_some_and(|e| walk(a, std::slice::from_ref(e), lets))
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Labeled { body, .. } => walk(a, std::slice::from_ref(body), lets),
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                walk(a, body, lets)
                    || walk(a, catch_body, lets)
                    || finally_body.as_deref().is_some_and(|f| walk(a, f, lets))
            }
            _ => false,
        })
    }
    walk(a, body, &mut HashSet::new())
}
