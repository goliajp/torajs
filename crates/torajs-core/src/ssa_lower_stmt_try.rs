//! `Stmt::Try` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 155).
//!
//! Pre-extract this arm was 667 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line. This is the single biggest arm in
//! lower_stmt.
//!
//! ## Control-flow shape (M4.1 + M4.2)
//!
//! ```text
//!   <pre>  ──br→ body
//!   body   ──throw→ catch (if had_catch) OR finally OR fn-propagate
//!          ──fall→ post_target (= finally if present, else after)
//!   catch  ──throw→ post_target (= finally if present, else fn-propagate)
//!          ──fall→ post_target
//!   finally  body lowered; on fall-through, three-way dispatch on
//!          throw_active / pending_return / pending_break /
//!          pending_continue → propagate vs after_blk vs ret vs
//!          loop-target
//!   after  rest of program
//! ```
//!
//! ## Key invariants
//!
//! - `try {} finally {}` (no catch) must let the throw propagate
//!   THROUGH finally to outer catch / fn-propagate (review test262
//!   fix — previously synthesized an empty catch_blk that cleared
//!   the flag via throw_take).
//! - `catch (e)` without annotation binds `Any` (P7.2b-2 — TS spec
//!   says implicit `any` for an unannotated catch parameter); the
//!   Any path reconstructs via (tag, value) read in that order
//!   (read tag first — throw_take's body zeroes active but leaves
//!   tag/value globals untouched).
//! - Catch param is OWNED by the catch local (throw_take cleared
//!   the global, the heap belongs to us); scope-close drop frees
//!   it if not consumed by return/throw.
//!
//! ## review #0001 — return-through-finally
//!
//! Push finally onto `try_finally_stack` so `Stmt::Return` inside
//! body / catch routes through the finally before actually
//! returning. Pop AFTER body+catch so finally body itself doesn't
//! see itself as the return target.
//!
//! ## P7.5 O5 — suspend pending throw at finally entry
//!
//! ECMA §14.13.3: finally executes regardless of try's completion;
//! if finally completes normally, try's pending completion (throw)
//! re-applies. Snapshot (active, tag, value) into entry-block
//! allocas, then clear active via throw_take. Finally body now
//! runs with active=0; tail dispatch restores the pending iff
//! finally body completed without re-throwing. Without this, any
//! may-throw call inside finally (e.g. `new Error(...)`) would
//! emit emit_throw_check which sees the outer pending=1 →
//! spurious propagation before the call could complete.
//!
//! ## Tail dispatch priority
//!
//! 1. `throw_active` → propagate (catch / next-outer / fn-end)
//!    - inner outer-catch route prevents the f3() throw-7-but-
//!      caller-got-0 bug from finally always ret'ing
//! 2. `pending_return`:
//!    - still wrapping finallies → br to next outer finally
//!    - outermost → load slot + ret
//! 3. `pending_break` / `pending_continue` (per loop-depth chaining)
//!    - clear the flag when jumping to loop-target to prevent a
//!      same-iteration re-fire
//! 4. Fall through to `after_blk`

use crate::ast::Stmt;
use crate::ssa::{BlockId, InstKind, Operand, Terminator};
use crate::ssa_lower::{LocalInfo, LowerCtx};
use crate::ssa_lower_scope_exit::ExitTarget;

mod bind;

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower(
    ctx: &mut LowerCtx,
    body: &[Stmt],
    had_catch: bool,
    catch_param: Option<&String>,
    catch_type: Option<&String>,
    catch_body: &[Stmt],
    finally_body: Option<&Vec<Stmt>>,
) {
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let finally_blk = if finally_body.is_some() {
        Some(ctx.f.add_block())
    } else {
        None
    };
    let post_target = finally_blk.unwrap_or(after_blk);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(body_blk));

    let catch_blk: Option<BlockId> = if had_catch {
        Some(ctx.f.add_block())
    } else {
        None
    };

    // RFC 20260901-scope-exit-drops — the body frame is about to be
    // pushed at this index; every route out of the body (throw to
    // catch / finally, return / break / continue through the finally)
    // releases the frames from here up before it branches.
    let body_depth = ctx.scope_stack.len();
    let teardown_depth = ctx.for_of_teardown_stack.len();
    if let Some(fb) = finally_blk {
        ctx.try_finally_stack.push(ExitTarget {
            blk: fb,
            scope_depth: body_depth,
            teardown_depth,
        });
        ctx.try_finally_loop_depth.push(ctx.loop_stack.len());
    }

    ctx.cur_block = body_blk;
    let body_throw_target = catch_blk.or(finally_blk);
    if let Some(t) = body_throw_target {
        ctx.try_stack.push(ExitTarget {
            blk: t,
            scope_depth: body_depth,
            teardown_depth,
        });
    }
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    // A try body is a block (§14.15): a closure in it that names a
    // binding declared later in it needs that binding's box up first,
    // exactly as `ssa_lower_stmt_block::lower` does for a bare block.
    crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, body.iter());
    let saved = ctx.proofs.region_enter();
    let body_mark = ctx.proofs.raise_mark(&ctx.f.name);
    for s in body {
        ctx.lower_stmt(s);
        if !ctx.cur_open() {
            break;
        }
    }
    ctx.proofs.region_leave(&saved);
    // Fall-through closes the body frame like any block: drop its
    // owners, remove them from `locals` (the fn-exit / finally-tail
    // walks must not see them again), restore shadows. Pre-RFC the
    // names stayed in `locals` and only the fn-exit walk released
    // them — once per fn activation, so a `try` in a main loop
    // stranded every iteration but the last.
    ctx.close_scope_frame();
    if ctx.cur_open() {
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(post_target));
    }
    if body_throw_target.is_some() {
        ctx.try_stack.pop();
    }

    if let Some(catch_blk) = catch_blk {
        if let Some(p) = catch_param {
            ctx.proofs.kill(p);
        }
        lower_catch(
            ctx,
            (catch_blk, body_mark),
            catch_param,
            catch_type,
            catch_body,
            finally_blk,
            post_target,
        );
    }

    ctx.proofs.region_leave(&saved);
    if let (Some(fb), Some(fbody)) = (finally_blk, finally_body) {
        crate::ssa_lower_stmt_try_finally::lower(ctx, fb, fbody, after_blk);
        ctx.proofs.region_leave(&saved);
    }
    ctx.cur_block = after_blk;
}

/// Emit the catch block: bind the catch parameter to the pending
/// throw value, run the handler, then hand control to the finally
/// block (or straight past the statement when there is none).
#[allow(clippy::too_many_arguments)]
fn lower_catch(
    ctx: &mut LowerCtx,
    (catch_blk, body_mark): (BlockId, usize),
    catch_param: Option<&String>,
    catch_type: Option<&String>,
    catch_body: &[Stmt],
    finally_blk: Option<BlockId>,
    post_target: BlockId,
) {
    ctx.cur_block = catch_blk;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    if let Some(fb) = finally_blk {
        // A throw out of the catch — its body, or a binding that
        // rejects the thrown value — routes to the finally and leaves
        // the catch frame (pushed above, so its index is `len() - 1`)
        // — the same index the body frame had.
        ctx.try_stack.push(ExitTarget {
            blk: fb,
            scope_depth: ctx.scope_stack.len() - 1,
            teardown_depth: ctx.for_of_teardown_stack.len(),
        });
    }
    // an unannotated parameter the program-wide throw analysis typed,
    // when the checks the body lowered to agree (`proofs::catch`)
    let inferred = match (catch_param, catch_type) {
        (Some(p), None) => (ctx.proofs.catch_type(ctx.ast, catch_body, p))
            .filter(|(_, want)| ctx.proofs.raises_confirm(&ctx.f.name, body_mark, want))
            .map(|(ann, _)| ann.clone()),
        _ => None,
    };
    let catch_type = catch_type.or(inferred.as_ref());
    // An annotated parameter the body never reads binds nothing: the
    // value is released as an unbound `catch {}` releases it, so its
    // type is never asked (`catch (e: number) {}` around a RangeError).
    let read = |p: &&String| {
        catch_type.is_none_or(|a| a == "any")
            || crate::ast::body_reads(ctx.ast, catch_body, p) != crate::ast::NameRead::No
    };
    if let Some(p) = catch_param.filter(read) {
        let ann = catch_type.map(|s| s.as_str());
        let (e_ty, slot_v) = bind::take(ctx, p, ann, inferred.is_some());
        let slot = ctx.alloca(e_ty, Some(p));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(slot_v, Operand::Value(slot), 0),
        );
        // A catch parameter shadows an outer binding of the same name
        // and must hand it back at the clause's close, the way a `let`
        // does (`ssa_lower_stmt_let_decl_general`). Without the save,
        // `close_scope_frame` removed the name from `locals` and the
        // OUTER binding went with it: after
        // `try {} catch (e) {}` a hoisted `var e` was gone, and
        // `var e = 3` past that point answered "ssa-lower: assign to
        // unknown ident `e`" — a program bun runs.
        let cur_depth = ctx.scope_stack.len() - 1;
        if let Some(prev) = ctx.locals.get(p).copied()
            && prev.scope_depth < cur_depth
        {
            let top_shadow = ctx.shadow_stack.last_mut().expect("shadow frame");
            top_shadow.push((p.clone(), prev));
        }
        ctx.locals.insert(
            p.clone(),
            LocalInfo {
                slot,
                ty: e_ty,
                moved: false,
                borrowed: false,
                boxed: false,
                scope_depth: cur_depth,
            },
        );
        ctx.scope_stack.last_mut().unwrap().push(p.clone());
    } else {
        bind::release_unbound(ctx);
    }
    crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, catch_body.iter());
    for s in catch_body {
        ctx.lower_stmt(s);
        if !ctx.cur_open() {
            break;
        }
    }
    if finally_blk.is_some() {
        ctx.try_stack.pop();
    }
    // Block-close protocol (RFC 20260901-scope-exit-drops): the catch
    // param and any catch-body local drop on fall-through, the frame's
    // names leave `locals`, shadows come back.
    ctx.close_scope_frame();
    if ctx.cur_open() {
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(post_target));
    }
}
