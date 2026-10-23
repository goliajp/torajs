//! `Stmt::Try { body, catch_param, catch_type, catch_body,
//! finally_body, .. }` typecheck pulled out of
//! [`crate::check::Checker::check_stmt`]'s `Stmt::Try` arm as
//! chunk-104 of the check_stmt decomp. 7th check_stmt sibling.
//!
//! 3-phase walker:
//!
//! 1. **Body** — fresh scope; walk each stmt via `check_stmt`.
//! 2. **Catch** — fresh scope with `e` bound. Param type from
//!    `(e: T)` annotation; P7.2b-2 — unannotated `catch (e)` is
//!    implicitly `any` per TS spec (an explicit non-any/unknown
//!    annotation is TS1196). Unknown type ann → push error + fall
//!    back to Any. Default `None => Any` mirrors ssa_lower so
//!    check-tier sees `e` as Any too (member access / arithmetic
//!    / return all go through Any paths).
//! 3. **Finally** (optional) — fresh scope; walk each stmt.
//!
//! Flow narrows: any statement of the body may throw, so the catch
//! starts from the state before the `try` joined with every write in
//! the body (`try { q = q.next; if (q === null) throw 0 } catch { … }`
//! must not see `q` non-null); `finally` likewise over the body and the
//! catch. After the statement the ends of the body and of the catch
//! that complete are joined.

use std::collections::HashMap;

use crate::ast::{Ast, Stmt};
use crate::check::{Checker, DiagPush, LocalInfo, Type, stmt_diverges};
use crate::check_type_ann::resolve_type_ann_full;

pub(crate) fn check(checker: &mut Checker, ast: &Ast, stmt: &Stmt) {
    let Stmt::Try {
        body,
        had_catch,
        catch_param,
        catch_type,
        catch_body,
        finally_body,
    } = stmt
    else {
        unreachable!("check_stmt_try::check takes a try statement");
    };
    let had_catch = *had_catch;
    let entry = checker.flow_entry();
    check_list(checker, ast, body, None);
    // ut3 — the catch entry point is any throw site inside the
    // body: no body assignment narrow survives into catch.
    checker.flush_assign_narrows();
    let body_end = checker.flow_at(&entry);
    let body_stmts: Vec<&Stmt> = body.iter().collect();
    checker.join_region_exit(ast, &entry, &body_stmts);
    // catch in a fresh scope with `e` injected.
    let e_ty = match catch_type {
        Some(ann) => {
            match resolve_type_ann_full(ann, &checker.aliases, &[], &checker.generic_alias_decls) {
                Some(t) => t,
                None => {
                    let msg = crate::check_type_ann::unknown_type(ann);
                    checker.errors.push_err(format!("{msg} in catch param"));
                    Type::Any
                }
            }
        }
        None => Type::Any,
    };
    let param = catch_param.as_ref().map(|p| (p, e_ty));
    check_list(checker, ast, catch_body, param);
    checker.flush_assign_narrows();
    let ends = |b: &[Stmt]| b.last().is_some_and(stmt_diverges);
    let after = if !had_catch || ends(catch_body) {
        body_end
    } else if ends(body) {
        checker.flow_at(&entry)
    } else {
        checker.join_flow(&body_end, &checker.flow_at(&entry))
    };
    checker.restore_flow(&after);
    if let Some(fb) = finally_body {
        let region: Vec<&Stmt> = body.iter().chain(catch_body).collect();
        checker.join_region_exit(ast, &entry, &region);
        check_list(checker, ast, fb, None);
        checker.flush_assign_narrows();
        // what follows runs after a body or catch that completed, then
        // the finally: the state they left, less what the finally wrote
        let fin: Vec<&Stmt> = fb.iter().collect();
        checker.resume_after(ast, &after, &fin);
    }
}

/// One of the three blocks in a scope of its own, the catch parameter
/// bound in it when given. Like any block (`check_block`), its
/// `let` / `const` are created on entry (§14.2.2), so a closure written
/// ahead of a declaration in the same block resolves the binding the
/// declaration makes (`try { const g = () => y; const y = 5; g() }`).
fn check_list(checker: &mut Checker, ast: &Ast, list: &[Stmt], param: Option<(&String, Type)>) {
    checker.scopes.push(HashMap::new());
    if let Some((p, ty)) = param {
        let _ = checker.declare(p.clone(), LocalInfo::plain(ty));
    }
    let saved_hoists = crate::check_hoist_closure_lets::enter(checker, ast, list);
    for s in list {
        checker.check_stmt(ast, s);
    }
    checker.hoisted_closure_lets = saved_hoists;
    checker.scopes.pop();
}
