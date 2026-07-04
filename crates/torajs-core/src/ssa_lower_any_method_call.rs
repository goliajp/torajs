//! `recv.name(args…)` where the receiver types as `any` —
//! Any-method-call RFC 20260704 C1 lowering.
//!
//! The checker's route_early arm admits the call as `any`; this
//! dispatcher arm (first in `ssa_lower_call::lower`'s cascade)
//! packs it for the runtime method dispatcher:
//!
//! - method NAME interns to an `ANY_METHOD_*` id at compile time
//!   (torajs-rc `any_method_id`) — the runtime switches on an
//!   integer for the built-in Str/Arr arms; the name also rides as
//!   an interned static Str (C3a-2) so the dynobj arm probes user
//!   properties (`o.f(x)`) by key.
//! - each argument boxes to a NaN-box AnyValue into a stack argv
//!   (`AllocaBytes(argc*8)` in the entry block). Ledger per the
//!   chunk-496 three-shape rule: `box_to_any` is TRANSFER, so a
//!   borrowed operand (Ident / Member) rc-incs first; temps hand
//!   their reference to the slot; already-`any` operands pass
//!   through unboxed (borrowed, not dec'd after).
//! - after the call every slot WE boxed rc-decs — the runtime
//!   borrows argv (per-method glue incs what it keeps), so the
//!   dec releases exactly the box's reference.
//! - `recv_slot`: an Ident receiver's variable slot pointer rides
//!   along so growth-relocating methods (push) write the fresh
//!   block pointer back; every other receiver shape passes NULL.

use torajs_rc::any_method_id;

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `callee(args…)` as an any-receiver method call.
/// Returns `None` unless the callee is a Member read off an
/// `any`-typed object.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Any)) {
        return None;
    }
    let obj = *obj;
    let name = name.clone();
    let mid = any_method_id(&name);
    // C3a-2 — the method name rides along as an interned static Str
    // (rc no-op) so the runtime's dynobj arm can probe user
    // properties by key; built-in ids keep the integer fast path.
    let name_str = ctx.intern_string_literal(&name);

    let recv = ctx.lower_expr(obj);
    // Ident receivers ride their variable slot along so
    // growth-relocating methods write the fresh pointer back —
    // local alloca or K.3 top-level global slot (the same two
    // shapes as index-assign's WriteBack).
    let recv_slot = if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        if let Some(info) = ctx.locals.get(n) {
            Operand::Value(info.slot)
        } else if ctx.globals.contains_key(n) {
            let name = n.clone();
            let gref = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
            Operand::Value(gref)
        } else {
            Operand::ConstPtrNull
        }
    } else {
        Operand::ConstPtrNull
    };

    let (argv, boxed_slots) = pack_any_argv(ctx, args);
    let argc = args.len();

    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_call,
            vec![
                recv,
                Operand::ConstI64(mid),
                Operand::Value(name_str),
                Operand::ConstI64(0),
                recv_slot,
                Operand::Value(argv),
                Operand::ConstI64(argc as i64),
            ],
        ),
        Type::Any,
        None,
    );
    // Release the boxes' references BEFORE the throw check (the
    // runtime has returned — argv is dead either way, and the
    // throw-propagate branch must not leak the boxes). The runtime
    // borrowed argv; per-method glue inc'd whatever it stored.
    for slot in boxed_slots.into_iter().flatten() {
        ctx.emit_drop_value(slot, Type::Any);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(result))
}

/// Box the call arguments into a stack argv per the chunk-496
/// three-shape ledger (see module doc) — shared by the method-call
/// arm above and the bare any-call arm
/// ([`crate::ssa_lower_any_call`]). Returns the argv alloca plus
/// the slots WE boxed (the caller rc-decs each one after the call).
pub(crate) fn pack_any_argv(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> (crate::ssa::ValueId, Vec<Option<Operand>>) {
    let argc = args.len();
    let argv = ctx.f.append_inst(
        crate::ssa::BlockId(0),
        InstKind::AllocaBytes((argc.max(1) * 8) as u64),
        Type::Ptr,
        Some("__amc_argv"),
    );
    let mut boxed_slots: Vec<Option<Operand>> = Vec::with_capacity(argc);
    for (i, &aid) in args.iter().enumerate() {
        let is_borrow = matches!(ctx.ast.get_expr(aid), Expr::Ident(_) | Expr::Member { .. });
        let raw = ctx.lower_expr(aid);
        let raw_ty = ctx.operand_ty(&raw);
        let (slot_val, we_boxed) = if raw_ty == Type::Any {
            (raw, false)
        } else {
            if is_borrow && raw_ty.is_refcounted() {
                ctx.emit_rc_inc(raw.clone());
            }
            (ctx.box_to_any(raw), true)
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(slot_val.clone(), Operand::Value(argv), (i * 8) as u64),
        );
        boxed_slots.push(if we_boxed { Some(slot_val) } else { None });
    }
    (argv, boxed_slots)
}
