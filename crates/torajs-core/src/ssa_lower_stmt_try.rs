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
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};
use crate::ssa_lower_parse_type::parse_type;

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

    if let Some(fb) = finally_blk {
        ctx.try_finally_stack.push(fb);
        ctx.try_finally_loop_depth.push(ctx.loop_stack.len());
    }

    ctx.cur_block = body_blk;
    let body_throw_target = catch_blk.or(finally_blk);
    if let Some(t) = body_throw_target {
        ctx.try_stack.push(t);
    }
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    for s in body {
        ctx.lower_stmt(s);
        if !ctx.cur_open() {
            break;
        }
    }
    if ctx.cur_open() {
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(post_target));
    }
    ctx.scope_stack.pop();
    let body_shadows = ctx.shadow_stack.pop().unwrap_or_default();
    for (name, prev) in body_shadows {
        ctx.locals.insert(name, prev);
    }
    if body_throw_target.is_some() {
        ctx.try_stack.pop();
    }

    if let Some(catch_blk) = catch_blk {
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
                Operand::Value(v)
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
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.throw_take, vec![]),
            );
        }
        if let Some(fb) = finally_blk {
            ctx.try_stack.push(fb);
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
        if ctx.cur_open() {
            let frame_names: Vec<String> = ctx
                .scope_stack
                .last()
                .map(|f| f.clone())
                .unwrap_or_default();
            for name in &frame_names {
                let info = match ctx.locals.get(name) {
                    Some(i) => *i,
                    None => continue,
                };
                if info.moved || info.ty.is_copy() || ctx.stack_alloced_locals.contains(name) {
                    continue;
                }
                let val = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                    info.ty,
                    None,
                );
                ctx.emit_drop_value(Operand::Value(val), info.ty);
            }
            let cb = ctx.cur_block;
            ctx.f.set_term(cb, Terminator::Br(post_target));
        }
        let catch_frame = ctx.scope_stack.pop().unwrap_or_default();
        let catch_shadows = ctx.shadow_stack.pop().unwrap_or_default();
        for name in catch_frame {
            ctx.locals.remove(&name);
        }
        for (name, prev) in catch_shadows {
            ctx.locals.insert(name, prev);
        }
    }

    if let (Some(fb), Some(fbody)) = (finally_blk, finally_body) {
        ctx.try_finally_stack.pop();
        ctx.try_finally_loop_depth.pop();
        ctx.cur_block = fb;
        ctx.scope_stack.push(Vec::new());
        ctx.shadow_stack.push(Vec::new());

        let saved_active_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_active"));
        let saved_tag_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_tag"));
        let saved_value_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_value"));
        let snap_active = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.throw_check, vec![]),
            Type::I64,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(
                Operand::Value(snap_active),
                Operand::Value(saved_active_slot),
                0,
            ),
        );
        let snap_tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
            Type::I64,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(snap_tag), Operand::Value(saved_tag_slot), 0),
        );
        let snap_value = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.throw_take, vec![]),
            Type::I64,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(
                Operand::Value(snap_value),
                Operand::Value(saved_value_slot),
                0,
            ),
        );
        for s in fbody {
            ctx.lower_stmt(s);
            if !ctx.cur_open() {
                break;
            }
        }
        if ctx.cur_open() {
            let probe_active = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.throw_check, vec![]),
                Type::I64,
                None,
            );
            let probe_cmp = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(
                    IPred::Ne,
                    Operand::Value(probe_active),
                    Operand::ConstI64(0),
                ),
                Type::Bool,
                None,
            );
            let dispatch_blk = ctx.f.add_block();
            let check_saved_blk = ctx.f.add_block();
            let cbr = ctx.cur_block;
            ctx.f.set_term(
                cbr,
                Terminator::CondBr {
                    cond: Operand::Value(probe_cmp),
                    then_blk: dispatch_blk,
                    else_blk: check_saved_blk,
                },
            );
            ctx.cur_block = check_saved_blk;
            let sa = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(saved_active_slot), 0),
                Type::I64,
                None,
            );
            let sa_cmp = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(sa), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let do_restore_blk = ctx.f.add_block();
            let cbr2 = ctx.cur_block;
            ctx.f.set_term(
                cbr2,
                Terminator::CondBr {
                    cond: Operand::Value(sa_cmp),
                    then_blk: do_restore_blk,
                    else_blk: dispatch_blk,
                },
            );
            ctx.cur_block = do_restore_blk;
            let st = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(saved_tag_slot), 0),
                Type::I64,
                None,
            );
            let sv = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(saved_value_slot), 0),
                Type::I64,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.throw_set,
                    vec![Operand::Value(st), Operand::Value(sv)],
                ),
            );
            let cbr3 = ctx.cur_block;
            ctx.f.set_term(cbr3, Terminator::Br(dispatch_blk));
            ctx.cur_block = dispatch_blk;

            let active = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.throw_check, vec![]),
                Type::I64,
                None,
            );
            let throw_cmp = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let prop_blk = ctx.f.add_block();
            let no_throw_blk = ctx.f.add_block();
            let cb = ctx.cur_block;
            ctx.f.set_term(
                cb,
                Terminator::CondBr {
                    cond: Operand::Value(throw_cmp),
                    then_blk: prop_blk,
                    else_blk: no_throw_blk,
                },
            );
            ctx.cur_block = prop_blk;
            if let Some(handler) = ctx.try_stack.last().copied() {
                let cb2 = ctx.cur_block;
                ctx.f.set_term(cb2, Terminator::Br(handler));
            } else {
                ctx.emit_drops_for_owned_locals();
                let cb2 = ctx.cur_block;
                let ret_ty = ctx.f.ret;
                let prop_term = match ret_ty {
                    Type::Void => Terminator::Ret(None),
                    Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
                    Type::I32 => Terminator::Ret(Some(Operand::ConstI32(0))),
                    Type::Bool => Terminator::Ret(Some(Operand::ConstBool(false))),
                    _ => Terminator::Ret(Some(Operand::ConstI64(0))),
                };
                ctx.f.set_term(cb2, prop_term);
            }

            ctx.cur_block = no_throw_blk;
            if let (Some(slot), Some(flag)) = (ctx.pending_return_slot, ctx.pending_return_flag) {
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                    Type::Bool,
                    None,
                );
                let ret_blk = ctx.f.add_block();
                let no_ret_blk = ctx.f.add_block();
                let cb3 = ctx.cur_block;
                ctx.f.set_term(
                    cb3,
                    Terminator::CondBr {
                        cond: Operand::Value(f),
                        then_blk: ret_blk,
                        else_blk: no_ret_blk,
                    },
                );
                ctx.cur_block = ret_blk;
                if let Some(outer_fb) = ctx.try_finally_stack.last().copied() {
                    let cb4 = ctx.cur_block;
                    ctx.f.set_term(cb4, Terminator::Br(outer_fb));
                } else {
                    let fn_ret_ty = ctx.f.ret;
                    let v = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Load(fn_ret_ty, Operand::Value(slot), 0),
                        fn_ret_ty,
                        None,
                    );
                    ctx.emit_drops_for_owned_locals();
                    let cb4 = ctx.cur_block;
                    ctx.f
                        .set_term(cb4, Terminator::Ret(Some(Operand::Value(v))));
                }
                ctx.cur_block = no_ret_blk;
            }
            if let Some(flag) = ctx.pending_break_flag {
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                    Type::Bool,
                    None,
                );
                let brk_blk = ctx.f.add_block();
                let no_brk_blk = ctx.f.add_block();
                let cb3 = ctx.cur_block;
                ctx.f.set_term(
                    cb3,
                    Terminator::CondBr {
                        cond: Operand::Value(f),
                        then_blk: brk_blk,
                        else_blk: no_brk_blk,
                    },
                );
                ctx.cur_block = brk_blk;
                let cur_loop_len = ctx.loop_stack.len();
                let outer_in_same_loop =
                    ctx.try_finally_loop_depth.last().copied() == Some(cur_loop_len);
                if outer_in_same_loop && let Some(outer_fb) = ctx.try_finally_stack.last().copied()
                {
                    let cb4 = ctx.cur_block;
                    ctx.f.set_term(cb4, Terminator::Br(outer_fb));
                } else if let Some((_, brk_target)) = ctx.loop_stack.last().copied() {
                    ctx.f.append_void(
                        ctx.cur_block,
                        InstKind::Store(Operand::ConstBool(false), Operand::Value(flag), 0),
                    );
                    let cb4 = ctx.cur_block;
                    ctx.f.set_term(cb4, Terminator::Br(brk_target));
                } else {
                    let cb4 = ctx.cur_block;
                    ctx.f.set_term(cb4, Terminator::Br(after_blk));
                }
                ctx.cur_block = no_brk_blk;
            }
            if let Some(flag) = ctx.pending_continue_flag {
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                    Type::Bool,
                    None,
                );
                let cont_blk = ctx.f.add_block();
                let no_cont_blk = ctx.f.add_block();
                let cb3 = ctx.cur_block;
                ctx.f.set_term(
                    cb3,
                    Terminator::CondBr {
                        cond: Operand::Value(f),
                        then_blk: cont_blk,
                        else_blk: no_cont_blk,
                    },
                );
                ctx.cur_block = cont_blk;
                let cur_loop_len = ctx.loop_stack.len();
                let outer_in_same_loop =
                    ctx.try_finally_loop_depth.last().copied() == Some(cur_loop_len);
                if outer_in_same_loop && let Some(outer_fb) = ctx.try_finally_stack.last().copied()
                {
                    let cb4 = ctx.cur_block;
                    ctx.f.set_term(cb4, Terminator::Br(outer_fb));
                } else if let Some((cont_target, _)) = ctx.loop_stack.last().copied() {
                    ctx.f.append_void(
                        ctx.cur_block,
                        InstKind::Store(Operand::ConstBool(false), Operand::Value(flag), 0),
                    );
                    let cb4 = ctx.cur_block;
                    ctx.f.set_term(cb4, Terminator::Br(cont_target));
                } else {
                    let cb4 = ctx.cur_block;
                    ctx.f.set_term(cb4, Terminator::Br(after_blk));
                }
                ctx.cur_block = no_cont_blk;
            }
            let cb4 = ctx.cur_block;
            ctx.f.set_term(cb4, Terminator::Br(after_blk));
        }
        ctx.scope_stack.pop();
        let finally_shadows = ctx.shadow_stack.pop().unwrap_or_default();
        for (name, prev) in finally_shadows {
            ctx.locals.insert(name, prev);
        }
    }
    ctx.cur_block = after_blk;
}
