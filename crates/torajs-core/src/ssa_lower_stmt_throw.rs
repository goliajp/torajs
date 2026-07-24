//! `Stmt::Throw` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 148).
//!
//! Pre-extract this arm was 180 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! M4 `throw v`:
//!
//! 1. Call `__torajs_throw_set(tag, value)` with a (tag, payload)
//!    pair packed per `v`'s static type (P4.7 — so catch `: any`
//!    sites can reconstruct an Any-box). Computation does NOT
//!    rc_inc by default — throw transfers ownership; the global
//!    `throw_value` slot now owns v's refcount stake. Borrowed
//!    sources get a retain (Swift ARC's +0-parameter / +1-result
//!    convention) so the catch binding can take ownership cleanly.
//! 2. If there's an active try (`try_stack` non-empty), `br`
//!    directly to the catch handler — ensures finally / catch
//!    inside the same fn runs before propagating.
//! 3. Otherwise:
//!    - **main fn** (bug-327 C2.5) → uncaught-exit-1 path: drop
//!      owned locals, ret the code from `__torajs_uncaught_exit_code`.
//!    - **other fns** → drop owned locals + ret a typed sentinel
//!      (0 / 0.0 / false / null) so the caller's
//!      `emit_throw_check` picks up the propagate.
//!
//! Tag packing:
//! - `Undefined` (frontend-distinguished from null) → ANY_UNDEF=5,
//!   payload 0.
//! - `I64` / `I32` → tag 2, value passes through.
//! - `F64` → tag 3, bit-cast to i64.
//! - `Bool` → tag 1, zext to i64.
//! - `Any` (already boxed) → extract tag+value via
//!   `any_unbox_tag` / `any_unbox_value` shims (decouples from
//!   AnyBox struct layout). Forwards `(tag, value)` to throw_set
//!   and rc_inc's the inner payload so the source-binding drop
//!   doesn't release it before catch reads it.
//! - `Ptr` const-null → tag 0, payload 0.
//! - refcounted → ANY_HEAP=4; if source ident is borrowed, retain
//!   (`emit_rc_inc`) before forwarding so throw_value carries its
//!   own +1.
//! - Fallback → tag 4, value as-is (matches pre-P4.7 behaviour for
//!   unusual operand types).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, eid: ExprId) {
    let v = ctx.lower_expr(eid);
    let v_ty = ctx.operand_ty(&v);
    // Chunk 752 — same Copy-result gate as the Return site: a thrown
    // scalar packs by value into the throw slot and cannot alias any
    // local's heap (`throw v.length` stranded v — probe vT 15.97MB
    // vs 6.37MB flat).
    if !v_ty.is_copy() {
        ctx.consume_all_idents_in_return(eid);
    }
    let is_undef = matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Undefined)
    );
    let (tag_op, val_op): (Operand, Operand) = if is_undef && matches!(v_ty, Type::Ptr) {
        (Operand::ConstI64(5), Operand::ConstI64(0))
    } else {
        match v_ty {
            Type::I64 | Type::I32 => (Operand::ConstI64(2), v),
            Type::F64 => {
                let bits =
                    ctx.f
                        .append_inst(ctx.cur_block, InstKind::BitCastF64ToI64(v), Type::I64, None);
                (Operand::ConstI64(3), Operand::Value(bits))
            }
            Type::Bool => {
                let zext =
                    ctx.f
                        .append_inst(ctx.cur_block, InstKind::ZExtBoolToI64(v), Type::I64, None);
                (Operand::ConstI64(1), Operand::Value(zext))
            }
            Type::Any => {
                // Chunk 610 — owned unbox fuses unbox_value +
                // payload_rc_inc (ShortStr materialize was
                // double-counted by the separate inc and leaked).
                let tag_v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![v.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_unbox_value_owned, vec![v.clone()]),
                    Type::I64,
                    None,
                );
                (Operand::Value(tag_v), Operand::Value(val_v))
            }
            Type::Ptr if matches!(v, Operand::ConstPtrNull) => {
                (Operand::ConstI64(0), Operand::ConstI64(0))
            }
            _ if v_ty.is_refcounted() => {
                let needs_retain = if let Expr::Ident(name) = ctx.ast.get_expr(eid) {
                    ctx.locals.get(name).is_some_and(|info| info.borrowed)
                } else {
                    false
                };
                if needs_retain {
                    ctx.emit_rc_inc(v.clone());
                }
                (Operand::ConstI64(4), v)
            }
            _ => (Operand::ConstI64(4), v),
        }
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_set, vec![tag_op, val_op]),
    );
    if let Some(handler) = ctx.try_stack.last().copied() {
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(handler));
    } else if ctx.is_main_fn {
        ctx.emit_drops_for_owned_locals();
        let uncaught_fid = *ctx
            .fn_table
            .get("__torajs_uncaught_exit_code")
            .expect("__torajs_uncaught_exit_code declared in module setup");
        let code = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(uncaught_fid, vec![]),
            Type::I32,
            None,
        );
        let cb = ctx.cur_block;
        ctx.f
            .set_term(cb, Terminator::Ret(Some(Operand::Value(code))));
    } else {
        ctx.emit_drops_for_owned_locals();
        let cb = ctx.cur_block;
        let ret_ty = ctx.f.ret;
        let term = match ret_ty {
            Type::Void => Terminator::Ret(None),
            Type::I64 => Terminator::Ret(Some(Operand::ConstI64(0))),
            Type::I32 => Terminator::Ret(Some(Operand::ConstI32(0))),
            Type::Bool => Terminator::Ret(Some(Operand::ConstBool(false))),
            Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
            _ => Terminator::Ret(Some(Operand::ConstI64(0))),
        };
        ctx.f.set_term(cb, term);
    }
    // Throw stmt 三 arm 都已 set_term(Br handler / Ret uncaught_code / Ret zero),
    // 后续 sibling stmt 若继续 lower 到 cur_block 会 append 进已 terminated block
    // 的 term 前,codegen 按物理序 emit 导致 dead code 真被 exec(实测:top-level
    // `throw ERR; console.log("after")` after 会 print,fn body throw + sibling
    // stmt 同漏)。切 cur_block 到 orphan block,后续 stmt lower 挂 dead block
    // 由 codegen/egraph dead-strip 剔除。
    let dead = ctx.f.add_block();
    ctx.cur_block = dead;
}
