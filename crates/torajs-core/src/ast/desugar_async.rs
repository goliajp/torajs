//! `async function f(...): T { ... }` body rewrite pass.
//!
//! Extracted from `ast.rs::desugar_async` (2026-06-03, god-file
//! decomp — P10.5-A1 prerequisite per `file-size.md` "先拆再加"
//! rule). The pass walks every `Stmt::FnDecl` whose name is in
//! `ast.async_fns`, rewrites each `return e;` inside its body to
//! `return Promise.resolve(e);`, appends a tail-safety return if
//! control flow falls through, and collapses the declared return
//! type to `Promise<inner_ty>`.
//!
//! Runs between `desugar_generators` and `desugar_classes` so the
//! generated `Promise.resolve(...)` Member-access expressions are
//! still in pre-desugar shape — `desugar_classes` rewrites
//! `Promise.X` static-member calls to `__sm_Promise__X` afterwards,
//! tying everything to the runtime's static dispatch table.
//!
//! Helpers `body_ends_in_return` / `rewrite_returns_for_async` are
//! re-exported from the parent `ast` module so sibling sub-modules
//! (`ast::desugar_classes_emit`) keep their existing `use super::*`
//! access path unchanged.

mod resolve;
mod sniff;
pub use sniff::ReturnProof;

use super::{Ast, Expr, ExprId, Stmt, default_init_for_type};
use resolve::{build_async_resolve, fallthrough_undefined_for};

/// Prefix of the marker ident an async body's fall-through tail carries,
/// suffixed with the declared inner type (`__undef_slot__number`). The
/// checker types it as that annotation and the SSA lowering materializes
/// that width's undefined sentinel — see [`fallthrough_undefined_for`].
pub(crate) const UNDEF_SLOT_MARKER: &str = "__undef_slot__";

pub fn desugar_async(ast: &mut Ast) {
    rewrite_async_fn_value_exprs(ast);
    if ast.async_fns.is_empty() {
        return;
    }
    // Walk every FnDecl at any nesting depth. An async fn declared
    // inside another fn's body parses through the same parse_fn and
    // registers in async_fns like a top-level one, but the original
    // top-level index scan never reached it: annotated nested bodies
    // then failed check (`Promise(Number)` vs bare `Number`), and
    // unannotated ones leaked bare values out of the Promise lane
    // (the probe-aw6 crash family). The taken tree keeps `ast.exprs`
    // free for the rewrites to append to.
    let mut stmts = std::mem::take(&mut ast.stmts);
    for s in &mut stmts {
        rewrite_async_decls(ast, s);
    }
    ast.stmts = stmts;
}

/// Depth-first FnDecl rewrite: children first, so an async fn nested
/// inside an async fn carries its own Promise wrap before the outer
/// body is rebuilt around it (`rewrite_returns_for_async` never
/// descends into a nested FnDecl — returns attribute to their own
/// fn — so the outer rebuild leaves the finished inner wrap alone).
fn rewrite_async_decls(ast: &mut Ast, s: &mut Stmt) {
    // `stmt_children` descends into FnDecl bodies, exported decls and
    // every body a ClassDecl carries — the last matters twice: a
    // method body is not a FnDecl until `desugar_classes` runs (after
    // this pass), and `desugar_generators` parks a generator's whole
    // body, nested declarations included, inside `__Gen_<name>.next`.
    for c in super::desugar_with::walk::stmt_children(s) {
        rewrite_async_decls(ast, c);
    }
    rewrite_in_arrow_bodies(ast, s);
    let Stmt::FnDecl { name, .. } = s else {
        return;
    };
    if !ast.async_fns.contains(name) {
        return;
    }
    // Blade 1 moved this body into a generator; the wrapper's `return
    // __async_drive(...)` already IS the promise.
    if ast.async_sm_fns.contains(name) {
        return;
    }
    // Rebuild this async FnDecl in place.
    let (name, type_params, params, return_type, body, span) =
        match std::mem::replace(s, Stmt::Block(Vec::new())) {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                span,
                ..
            } => (name, type_params, params, return_type, body, span),
            _ => unreachable!(),
        };
    // P10.7 — Default-Any async fn: with no return-type annotation and
    // no primitive the body names (`sniff`), infer `Promise<any>` so the
    // body's `return e` rewrites produce `Promise.resolve(e as any)` (the
    // `As`-wrap happens inside `rewrite_returns_for_async` when inner_ty
    // == "any"). `Promise.resolve` accepts Any at `check/promise_static.rs`;
    // `.then` / `.catch` accept `Promise<Any>` at `check.rs:5008` / `:5054`.
    let declared_ty = sniff::declared_or_sniffed(ast, &name, return_type, &params, &body);
    let ssa = ast.async_ssa.fns.contains(&name);
    let (wrapped_body, promise_ty) = build_async_body(ast, body, &declared_ty, ssa);
    *s = Stmt::FnDecl {
        name,
        type_params,
        params,
        return_type: Some(promise_ty),
        body: wrapped_body,
        is_generator: false,
        // in-place rewrite of the user's declaration -- toString
        // answers the source text they wrote (B1b)
        span,
    };
}

/// The declarations in the bodies of the arrows `s`'s own expressions
/// hold: an arrow is still an expression here, so `stmt_children`
/// does not reach into it, and `lift_arrow_fns` only lifts it later.
fn rewrite_in_arrow_bodies(ast: &mut Ast, s: &Stmt) {
    for arrow in super::desugar_with::walk::arrow_nodes(ast, s) {
        let i = arrow.0 as usize;
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[i] else {
            continue;
        };
        let mut body = std::mem::take(body);
        for st in &mut body {
            rewrite_async_decls(ast, st);
        }
        if let Expr::ArrowFn { body: b, .. } = &mut ast.exprs[i] {
            *b = body;
        }
    }
}

/// RFC 20260713 blade 3 — async function-VALUE expressions (`async
/// () => ...` / `async x => ...` / `async function(){}`). Each marked
/// `Expr::ArrowFn` gets the same body transform as an async FnDecl,
/// applied in place while the body is still inside the expression —
/// `lift_arrow_fns` runs later, so the lifted `__closure_N` is an
/// ordinary Promise-returning closure and captures ride the normal
/// `__env` channel (no async_fns registration, no pass reordering).
fn rewrite_async_fn_value_exprs(ast: &mut Ast) {
    if ast.async_fn_value_exprs.is_empty() {
        return;
    }
    // Deterministic order: the rewrites append fresh exprs to the
    // arena, so iterating the HashSet directly would make the arena
    // layout (and the compiled artifact) run-to-run unstable.
    let mut ids: Vec<ExprId> = ast.async_fn_value_exprs.iter().copied().collect();
    ids.sort_by_key(|e| e.0);
    for eid in ids {
        // Blade 2 moved this body into a hoisted generator; the wrapper
        // arrow's `return __async_drive(...)` already IS the promise.
        if ast.async_sm_value_exprs.contains(&eid) {
            continue;
        }
        let i = eid.0 as usize;
        let arrow = std::mem::replace(&mut ast.exprs[i], Expr::Null);
        let Expr::ArrowFn {
            params,
            return_type,
            body,
        } = arrow
        else {
            panic!("async_fn_value_exprs marker on a non-ArrowFn ExprId {eid:?} (parser contract)");
        };
        let declared_ty = return_type.unwrap_or_else(|| "any".into());
        let ssa = ast.async_ssa.value_exprs.contains(&eid);
        let (wrapped_body, promise_ty) = build_async_body(ast, body, &declared_ty, ssa);
        ast.exprs[i] = Expr::ArrowFn {
            params,
            return_type: Some(promise_ty),
            body: wrapped_body,
        };
    }
}

/// Shared async body transform (FnDecl and ArrowFn shapes): rewrite
/// every `return e` to `return Promise.resolve(e)`, add the
/// tail-safety default return, and wrap the whole body in `try {...}
/// catch (__async_err: any) { return Promise.reject(__async_err) }`.
/// Returns the wrapped body plus the `Promise<T>` annotation for the
/// function's return slot.
///
/// P10.3-A2 — accept both annotation forms:
///   `async function f(): number { ... }`          → inner_ty = "number"
///   `async function f(): Promise<number> { ... }` → inner_ty = "number"
/// Pre-P10.3-A2 the desugar unconditionally prepended `Promise<...>`,
/// double-wrapping the idiomatic TS form into `Promise<Promise<T>>`.
///
/// P10.5-A1 — the try/catch wrap is ES spec §27.7.3.6
/// AsyncFunctionBody: an uncaught throw becomes a rejected Promise
/// instead of propagating synchronously to the caller. P10.5-A2 —
/// `catch_type` is `any` (spec-correct; `throw <expr>` may differ from
/// inner T), accepted by `Promise.reject` at `check/promise_static.rs`.
/// Shared with the class-method path
/// (`desugar_classes_emit::maybe_rewrite_async_method_body`) — the two
/// used to hold the same transform twice, and only this copy was taught
/// the P10.7 default-`any`, so an async METHOD without a return
/// annotation kept panicking long after the top-level requirement was
/// lifted.
///
/// `ssa` — a body the SSA lowering suspends over a frame
/// (`desugar_async_ssa_gate`): the frame turns a throw into the
/// rejection, so there is no try wrap, and the lowering returns each
/// `Promise.resolve(e)` built here as `e` (`async_ssa.return_calls`).
pub(super) fn build_async_body(
    ast: &mut Ast,
    body: Vec<Stmt>,
    declared_ty: &str,
    ssa: bool,
) -> (Vec<Stmt>, String) {
    let first_new_expr = ast.exprs.len();
    let inner_ty = if let Some(rest) = declared_ty.strip_prefix("Promise<")
        && let Some(inner) = rest.strip_suffix('>')
    {
        inner.to_string()
    } else {
        declared_ty.to_string()
    };
    let promise_ty = format!("Promise<{inner_ty}>");

    // T-15.h: rewrite each `return e;` to `return Promise.resolve(e);`.
    // No shared `__async_p` state — every return constructs a fresh
    // fulfilled built-in Promise.
    let mut new_body: Vec<Stmt> = Vec::with_capacity(body.len() + 1);
    for s in body {
        let mut s = s;
        rewrite_returns_for_async(ast, &mut s, &inner_ty);
        new_body.push(s);
    }
    // Tail safety: if control flow falls off the end, return
    // `Promise.resolve(<default T>)`.
    if !body_ends_in_return(&new_body) {
        let tail_id = ast.add_expr(fallthrough_undefined_for(&inner_ty));
        let call = build_async_resolve(ast, tail_id, &inner_ty);
        new_body.push(Stmt::Return(Some(call)));
    }
    if ssa {
        let made = (first_new_expr..ast.exprs.len()).map(|i| ExprId(i as u32));
        let calls: Vec<ExprId> = made
            .filter(|e| ast.synth_promise_static_calls.contains(e))
            .collect();
        ast.async_ssa.return_calls.extend(calls);
        return (new_body, promise_ty);
    }

    let err_ident_for_arg = ast.add_expr(Expr::Ident("__async_err".into()));
    let promise_ident_rj = ast.add_expr(Expr::Ident("Promise".into()));
    let reject_member = ast.add_expr(Expr::Member {
        obj: promise_ident_rj,
        name: "reject".into(),
    });
    let reject_call = ast.add_expr(Expr::Call {
        callee: reject_member,
        args: vec![err_ident_for_arg],
    });
    // Internal settle spelling — never the user's patchable slot
    // (see `Ast::synth_promise_static_calls`).
    ast.synth_promise_static_calls.insert(reject_call);
    let catch_body = vec![Stmt::Return(Some(reject_call))];
    // §11.2.2 — the body's strictness has to survive being wrapped.
    // Everything downstream reads it off the directive prologue
    // (`parser::strict_directive`), and the prologue is now one level
    // down, inside the Try: `cur_fn_is_sloppy` looked at the head of
    // the body it was lowering, found a `Stmt::Try`, and answered
    // "sloppy" for every async function — including one carrying its
    // own `"use strict"`. Re-minted at the head rather than moved:
    // the original is a pure string expression, harmless where it is.
    let strict = ast.body_says_use_strict(&new_body);
    let mut wrapped_body = Vec::with_capacity(2);
    if strict {
        let e = crate::ast::sloppy_this_prologue::mint_use_strict_directive(ast);
        wrapped_body.push(Stmt::Expr(e));
    }
    wrapped_body.push(Stmt::Try {
        body: new_body,
        had_catch: true,
        catch_param: Some("__async_err".into()),
        catch_type: Some("any".into()),
        catch_body,
        finally_body: None,
    });
    (wrapped_body, promise_ty)
}

/// True if the (possibly-empty) body's last reachable statement is a
/// `Stmt::Return`. Used by `desugar_async` to skip emitting the tail-
/// safety `return __async_p;` when the body already ends in one (a
/// second access of `__async_p` would trip tr's move tracker even on
/// the unreachable path).
fn body_ends_in_return(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return(_)) => true,
        // A trailing `throw` is a terminal completion too — control
        // never falls through, so the async tail-safety return is
        // both unreachable AND (for struct inner types like the
        // async-generator `__step_*`) wrongly typed: the
        // default_init_for_type catch-all answers Number(0.0), which
        // fails check as Promise(Number) vs Promise(Struct).
        Some(Stmt::Throw(_)) => true,
        Some(Stmt::Multi(stmts)) | Some(Stmt::Block(stmts)) => body_ends_in_return(stmts),
        _ => false,
    }
}

/// T-15.h (v0.5.0) — recursively rewrite `Stmt::Return(Some(e))` /
/// `Stmt::Return(None)` inside `s` into `Stmt::Return(Promise.resolve(e))`.
///
/// Pre-T-15.h MVP wrapped each return in a user-class `__async_p`
/// shared across the function body (`__async_p.do_resolve(e); return
/// __async_p;`). With the built-in Promise<T> from T-15, every return
/// just constructs a fresh fulfilled Promise — no shared state, no
/// move-tracker complications, no need for the user to declare
/// `class Promise<T>` themselves.
fn rewrite_returns_for_async(ast: &mut Ast, s: &mut Stmt, inner_ty: &str) {
    match s {
        Stmt::Return(maybe) => {
            let raw_value = match maybe {
                Some(eid) => *eid,
                None => {
                    let default = default_init_for_type(inner_ty);
                    ast.add_expr(default)
                }
            };
            // A returned object LITERAL binds to a `const` declared as
            // the inner type before it reaches `Promise.resolve`. The
            // rewrite turns the literal into a call argument, and the
            // one site that pins a declared struct layout onto a
            // literal — the ret-lowering of a fn declared to return
            // that struct — no longer sees it (the fn returns a
            // Promise now). Un-hinted, the literal took whichever
            // same-named layout registered first: an async generator's
            // `{value: <f64>, done}` boxed its value into a sync
            // generator's `{value: any, done}` step, and every reader
            // parsed the declared type and loaded the box bits as f64
            // (`a 128 false` for a stored 100; K5, rotation 641). Not
            // generator-specific — `async function f(): Promise<{value:
            // number; done: boolean}>` read the same way. The typed
            // `const` is the let-decl hint every other literal takes.
            if inner_ty != "any" && matches!(ast.get_expr(raw_value), Expr::ObjectLit { .. }) {
                let name = format!("__aret_{}", raw_value.0);
                let decl = Stmt::LetDecl {
                    mutable: false,
                    name: name.clone(),
                    type_ann: Some(inner_ty.to_string()),
                    init: raw_value,
                    is_var: false,
                };
                let read = ast.add_expr(Expr::Ident(name));
                let call = build_async_resolve(ast, read, inner_ty);
                *s = Stmt::Block(vec![decl, Stmt::Return(Some(call))]);
                return;
            }
            let call = build_async_resolve(ast, raw_value, inner_ty);
            *s = Stmt::Return(Some(call));
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_returns_for_async(ast, then_branch, inner_ty);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_returns_for_async(ast, eb, inner_ty);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        Stmt::Labeled { body, .. } => {
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        // probe aw7-forof — a `return` inside an async body's for-of
        // kept its bare value (this match simply never had the arm):
        // annotated fns failed check, unannotated ones leaked the
        // bare value out of the Promise lane.
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        Stmt::For { body, init, .. } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_returns_for_async(ast, i, inner_ty);
            }
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for s in &mut c.body {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
            if let Some(d) = default {
                for s in d {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
            for s in catch_body {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
        }
        _ => {}
    }
}
