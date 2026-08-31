//! RFC 20260725-fallthrough-return — the fall-through table: which
//! functions can run off the end of their body, and therefore answer
//! `undefined` there (ES §10.2.1.4 step 11) rather than through a
//! `return`. The call site reads it to know a result may hold that
//! return width's sentinel.
//!
//! Split out of `mod.rs` when knife 4's arrow-binding alias pass pushed
//! that file past the 500-line limit; the name sets themselves, and
//! the predicate that fills them, moved on to `sentinel_tables.rs`
//! when the field set made three of them.

use super::sentinel_tables::{SentinelTables, body_returns_sentinel};
use super::{Analysis, Scope, SlotKey, let_names};
use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashSet;

/// One top-level `FnDecl`: seed its param / return escape faces, put
/// it on the fall-through table when it can answer `undefined`, then
/// walk its body under its own scope.
pub(super) fn seed_and_walk_fn(
    a: &mut Analysis<'_>,
    stmt: &Stmt,
    tables: &SentinelTables,
    fallthrough_fns: &mut HashSet<String>,
) {
    let Stmt::FnDecl {
        name,
        params,
        body,
        return_type,
        ..
    } = stmt
    else {
        return;
    };
    // W-ESC — any-annotated param / return faces are escape sinks
    // (escape.rs).
    for p in params {
        if let Some(ann) = &p.type_ann {
            let pk = SlotKey::Param(name.clone(), p.name.clone());
            a.seed_any_face(&pk, ann);
        }
    }
    if let Some(r) = return_type {
        let rk = SlotKey::Ret(name.clone());
        a.seed_any_face(&rk, r);
        seed_fallthrough_return(a, rk, r, name, body, tables, fallthrough_fns);
    }
    let scope = Scope {
        fn_name: name,
        params: params.iter().map(|p| p.name.clone()).collect(),
        locals: {
            let mut s = HashSet::new();
            for b in body {
                let_names::collect_let_names(b, &mut s);
            }
            s
        },
    };
    for b in body {
        a.walk_stmt(b, &scope);
    }
}

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
    tables: &SentinelTables,
    out: &mut HashSet<String>,
) {
    if return_ann == "void" {
        return;
    }
    let falls_through = !crate::ast::body_always_terminates(body);
    if !falls_through && !body_returns_sentinel(a, fn_name, body, tables) {
        return;
    }
    out.insert(fn_name.to_string());
    if return_ann == "number" && falls_through {
        a.seeds.push(rk);
    }
}
