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
use crate::ssa::{BlockId, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};
use crate::ssa_lower_parse_type::parse_type;
use crate::ssa_lower_scope_exit::ExitTarget;

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
    if let Some(fb) = finally_blk {
        ctx.try_finally_stack.push(ExitTarget {
            blk: fb,
            scope_depth: body_depth,
        });
        ctx.try_finally_loop_depth.push(ctx.loop_stack.len());
    }

    ctx.cur_block = body_blk;
    let body_throw_target = catch_blk.or(finally_blk);
    if let Some(t) = body_throw_target {
        ctx.try_stack.push(ExitTarget {
            blk: t,
            scope_depth: body_depth,
        });
    }
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    for s in body {
        ctx.lower_stmt(s);
        if !ctx.cur_open() {
            break;
        }
    }
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
        lower_catch(
            ctx,
            catch_blk,
            catch_param,
            catch_type,
            catch_body,
            finally_blk,
            post_target,
        );
    }

    if let (Some(fb), Some(fbody)) = (finally_blk, finally_body) {
        crate::ssa_lower_stmt_try_finally::lower(ctx, fb, fbody, after_blk);
    }
    ctx.cur_block = after_blk;
}

/// Emit the catch block: bind the catch parameter to the pending
/// throw value, run the handler, then hand control to the finally
/// block (or straight past the statement when there is none).
#[allow(clippy::too_many_arguments)]
fn lower_catch(
    ctx: &mut LowerCtx,
    catch_blk: BlockId,
    catch_param: Option<&String>,
    catch_type: Option<&String>,
    catch_body: &[Stmt],
    finally_blk: Option<BlockId>,
    post_target: BlockId,
) {
    ctx.cur_block = catch_blk;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    if let Some(p) = catch_param {
        let e_ty = match catch_type {
            Some(ann) => parse_type(
                Some(ann.as_str()),
                ctx.aliases,
                ctx.arr_layouts,
                ctx.fn_sigs,
                ctx.generic_struct_decls,
                ctx.struct_layouts,
                ctx.inst_memo,
            ),
            None => Type::Any,
        };
        // RFC 20260726-array-elem-width — `throw_take` hands back the
        // pending slot's raw 8 bytes, so the binding has to read them
        // at the width the throw sites wrote. A `let` asks the table
        // the same way; catch never did, and `throw xs[i]` on a widened
        // array wrote f64 bits that `catch (e: number)` read as an
        // integer.
        let e_ty = if e_ty == Type::I64
            && catch_type.map(|s| s.as_str()) == Some("number")
            && ctx
                .num_f64_slots
                .slot_is_f64(&crate::num_width::SlotKey::Thrown)
        {
            Type::F64
        } else {
            e_ty
        };
        let slot_v = if matches!(e_ty, Type::Any) {
            let tag_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
                Type::I64,
                None,
            );
            let val_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.throw_take, vec![]),
                Type::I64,
                Some(p),
            );
            let boxed = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::Value(tag_v), Operand::Value(val_v)],
                ),
                Type::Any,
                None,
            );
            Operand::Value(boxed)
        } else {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.throw_take, vec![]),
                Type::I64,
                Some(p),
            );
            if e_ty == Type::F64 {
                // The slot holds raw 8 bytes; decode them the way the
                // throw site encoded them rather than converting the
                // number (the symmetric `BitCastF64ToI64` the promise
                // value point uses).
                Operand::Value(ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BitCastI64ToF64(Operand::Value(v)),
                    Type::F64,
                    None,
                ))
            } else {
                Operand::Value(v)
            }
        };
        let slot = ctx.alloca(e_ty, Some(p));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(slot_v, Operand::Value(slot), 0),
        );
        ctx.locals.insert(
            p.clone(),
            LocalInfo {
                slot,
                ty: e_ty,
                moved: false,
                borrowed: false,
                scope_depth: ctx.scope_stack.len() - 1,
            },
        );
        ctx.scope_stack.last_mut().unwrap().push(p.clone());
    } else {
        // Unbound `catch {}` still owns the taken value — take
        // (tag, value), box, and drop it. Pre-fix this arm only
        // cleared the active flag via a discarded throw_take,
        // stranding the whole thrown heap payload (Error inst +
        // message + stack) per catch.
        let tag_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
            Type::I64,
            None,
        );
        let val_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.throw_take, vec![]),
            Type::I64,
            None,
        );
        let boxed = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::Value(tag_v), Operand::Value(val_v)],
            ),
            Type::Any,
            None,
        );
        ctx.emit_drop_value(Operand::Value(boxed), Type::Any);
    }
    if let Some(fb) = finally_blk {
        // A throw out of the catch body routes to the finally and
        // leaves the catch frame (pushed above, so its index is
        // `len() - 1`) — the same index the body frame had.
        ctx.try_stack.push(ExitTarget {
            blk: fb,
            scope_depth: ctx.scope_stack.len() - 1,
        });
    }
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
