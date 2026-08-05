//! The runtime-descriptor arm of `Object.defineProperty` — split out
//! of [`crate::ssa_lower_object_define`], which sat at 502 lines as
//! registered debt, when the struct receiver gained a define surface.
//!
//! The descriptor here is a value, not a literal, so its fields are
//! read at runtime by a kernel; what this fn decides is which kernel,
//! which is a question about the RECEIVER's storage.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_object_define::{DefineKey, emit_key_release, is_typed_object, lower_key};

pub(crate) fn emit_define_runtime_desc(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: Type,
    key: &DefineKey,
    receiver_ident: &Option<String>,
    desc_eid: ExprId,
) -> bool {
    let (key_op, key_owned) = lower_key(ctx, key);
    let desc_op = ctx.lower_expr(desc_eid);
    let desc_ty = ctx.operand_ty(&desc_op);
    let desc_ptr: Operand = match desc_ty {
        // §6.2.6.5 step 1 gate BEFORE the unbox — an imm AnyValue's
        // payload is not a cell (the old path handed a number's bits
        // to define_from_desc as a pointer), and null / undefined /
        // primitive cells must throw (RFC 20260713 chunk B).
        Type::Any => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.throw_typeerror_if_not_desc_object,
                    vec![desc_op.clone()],
                ),
            );
            ctx.emit_throw_check(None);
            Operand::Value(ctx.any_unbox_value_as_ptr(desc_op.clone()))
        }
        // Heap cells whose shape the define_from_desc entry resolves
        // (Closure / Arr expando props dynobj at +24; a typed struct
        // reads through the kernel's `DescStore::Struct` layout walk
        // — RFC 20260721 刀 2 closed the old declined divergence).
        Type::Closure(_) | Type::Arr(_) | Type::Obj(_) => desc_op.clone(),
        _ if is_typed_object(desc_ty) => return false,
        // Statically-known primitive descriptor (`defineProperty(o,
        // "k", 5)` / `null` literal) — box once and let the helper
        // throw the §6.2.6.5 step 1 TypeError (same shape as
        // `emit_receiver_typecheck`'s primitive arm: the post-check
        // block is unreachable at runtime). Decided before the
        // obj-storage gate below: the throw needs no receiver storage,
        // so `defineProperty({}, "k", 5)` / an unfolded `{a: null}`
        // field throws instead of falling through.
        _ => {
            let boxed = ctx.box_to_any_from_expr(desc_eid, desc_op.clone());
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.throw_typeerror_if_not_desc_object,
                    vec![boxed],
                ),
            );
            ctx.emit_throw_check(None);
            return true;
        }
    };
    // Receiver storage gate — Any (dynobj-backed), typed Arr, and
    // typed Closure (kernel expando arm, RFC 20260721 刀 2)
    // receivers carry a define surface here; other shapes keep the
    // caller's fall-through.
    if !matches!(
        obj_ty,
        Type::Any | Type::Arr(_) | Type::Closure(_) | Type::FnSig(_) | Type::Obj(_)
    ) {
        return false;
    }
    let obj_ptr: Operand = match &obj_ty {
        Type::Any => Operand::Value(ctx.any_unbox_value_as_ptr(obj_op)),
        Type::Arr(_) => {
            // Kernel element writes are kind-aware — mark at the
            // reflection boundary (mirror of the literal Arr arm).
            ctx.emit_arr_mark_kind(&obj_op);
            obj_op
        }
        // Typed Closure, and a class instance — the operand is the
        // cell ptr, and the kernel's receiver dispatch descends into
        // whichever `+24` expando dict the tag names.
        _ => obj_op,
    };
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(obj_ptr, Operand::Value(slot), 0),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_define_from_desc,
            vec![Operand::Value(slot), key_op.clone(), desc_ptr],
        ),
    );
    // The helper borrows the descriptor; a fresh owned temp (call /
    // new product) still holds its own stake — release it here,
    // mirroring defineProperties' props release and the objlit
    // runtime arm's descriptor drop (pre-fix `defineProperty(o, k,
    // mkDesc())` leaked the descriptor cell per call).
    ctx.release_owned_temp(desc_eid, &desc_op);
    // 刀 18 — coerced key was owned Str; drop after helper borrowed it.
    emit_key_release(ctx, key_op, key_owned);
    ctx.emit_throw_check(None);
    if matches!(obj_ty, Type::Any) {
        ctx.emit_any_dynobj_writeback(receiver_ident, slot);
    }
    true
}
