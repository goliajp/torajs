//! `Stmt::While { cond, body }` + `Stmt::DoWhile { body, cond }`
//! typecheck pulled out of [`crate::check::Checker::check_stmt`]'s
//! corresponding arms as chunk-102 of the check_stmt decomp. 5th
//! check_stmt sibling.
//!
//! - **While** — V3-18 narrow wedge on `(x !== null)` conditions:
//!   narrow `x` for the body. A body that writes `x` keeps the narrow
//!   only when the single walk sees every write before a use it could
//!   invalidate (`check_assigns_to`); the write is then checked
//!   against the declared type and retires the narrow from that point
//!   on (`t = q; q = q.next` types `t = q` under the narrow and
//!   `q = q.next` against `T | null`). Polarity-false (`x === null`)
//!   wouldn't enter the loop, so no narrow.
//! - **DoWhile** — body runs first (no narrow); condition typechecks
//!   after, from the end of the body joined with every `continue`.
//!
//! Chunk 782 adds the member-path mirror (`while (o.cb)` narrows
//! `o.cb` for the body), under the same region rule: a write to the
//! receiver or the member retires it (`check_type_of_assign`), so
//! `while (t.next !== null) t = t.next` types its right-hand side
//! under the narrow.

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, DiagPush, Type, js_truthy_acceptable, stmt_diverges};
use crate::check_assigns_to::{Guarded, Region, binding_hit, member_hit, scan};
use crate::check_stmt_diverges::continues_out;

pub(crate) fn check_while(checker: &mut Checker, ast: &Ast, cond: ExprId, body: &Stmt) {
    checker.enter_loop(ast, &[cond], body);
    match checker.type_of(ast, cond) {
        Ok(t) if js_truthy_acceptable(&t) => {}
        Ok(other) => checker.errors.push_err(format!(
            "while condition must be boolean (or coercible), got {other:?}"
        )),
        Err(e) => checker.errors.push_err(e),
    }
    checker.note_unmodeled_guard(ast, cond);
    let narrow = checker.collect_null_narrow(ast, cond);
    let mut guard = None;
    let saved = match &narrow {
        Some((name, inner, true)) => {
            let hit = binding_hit(ast, name);
            match scan(
                ast,
                body,
                &Guarded {
                    root: name,
                    hit: &hit,
                },
            ) {
                Region::NoWrite => checker.apply_narrow(name, inner.clone()),
                Region::StraightLine => {
                    guard = checker.open_guard(name, Type::Nullable(Box::new(inner.clone())));
                    checker.apply_narrow(name, inner.clone())
                }
                Region::Unsafe => None,
            }
        }
        _ => None,
    };
    let member_narrow = checker.collect_member_narrow(ast, cond);
    let member_saved = match &member_narrow {
        Some((key, inner, true)) if member_region(ast, body, key) != Region::Unsafe => {
            Some(checker.apply_member_narrow(key, inner.clone()))
        }
        _ => None,
    };
    checker.check_stmt(ast, body);
    if let (Some((name, inner, _)), Some(prev)) = (&narrow, saved) {
        // a body that writes the binding leaves the loop with whatever
        // the write stored, or with the null that ended it
        let after = if guard.is_some() {
            Type::Nullable(Box::new(inner.clone()))
        } else {
            prev
        };
        checker.restore_narrow(name, after);
    }
    checker.close_guard(guard);
    if let (Some((key, _, _)), Some(prev)) = (&member_narrow, member_saved) {
        checker.restore_member_narrow(key, prev);
    }
}

/// How `s` writes the member path `key` guards (its receiver or the
/// member itself).
pub(crate) fn member_region(ast: &Ast, s: &Stmt, key: &(String, String)) -> Region {
    let root = key.0.split(['.', '[']).next().unwrap_or(&key.0);
    let hit = member_hit(ast, &key.0, &key.1);
    scan(ast, s, &Guarded { root, hit: &hit })
}

pub(crate) fn check_do_while(checker: &mut Checker, ast: &Ast, body: &Stmt, cond: ExprId) {
    checker.enter_loop(ast, &[cond], body);
    let entry = checker.flow_entry();
    checker.check_stmt(ast, body);
    // a `continue` reaches the condition from inside the body
    if continues_out(body) {
        let end = checker.flow_at(&entry);
        checker.join_region_exit(ast, &entry, &[body]);
        if !stmt_diverges(body) {
            let joined = checker.join_flow(&checker.flow_at(&entry), &end);
            checker.restore_flow(&joined);
        }
    }
    checker.note_unmodeled_guard(ast, cond);
    match checker.type_of(ast, cond) {
        Ok(t) if js_truthy_acceptable(&t) => {}
        Ok(other) => checker.errors.push_err(format!(
            "do-while condition must be boolean (or coercible), got {other:?}"
        )),
        Err(e) => checker.errors.push_err(e),
    }
}
