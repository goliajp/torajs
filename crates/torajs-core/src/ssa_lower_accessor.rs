//! Accessor (get/set) property dispatch — RFC C3
//! (`.claude/rfcs/20260613-object-property-descriptors/`).
//!
//! A property defined via `Object.defineProperty(o, k, { get, set })`
//! stores an `AccessorPair` cell (torajs-dynobj `accessor.rs`) as its
//! dynobj entry value. This module holds the two SSA-emit halves,
//! carved out of `ssa_lower.rs` / `ssa_lower_object_define.rs` so the
//! accessor trunk grows here instead of the 25k-line god-file:
//!
//! - [`emit_accessor_define`] — DEFINE: lower the getter/setter
//!   closures, build the `AccessorPair`, store it with the descriptor's
//!   enumerable/configurable attributes.
//! - [`emit_any_get_result`] — GET: branch a dynobj property read on
//!   the `ANY_ACCESSOR` `get_tag` sentinel, dispatching the getter
//!   (`any_accessor_get`) vs the data-property `any_box`.

use crate::ast::ExprId;
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_accessor_face::lower_accessor_face;
use crate::ssa_lower_object_define::{DefineKey, lower_key};

/// `get_tag` sentinel marking an accessor entry — mirrors
/// `torajs_dynobj::layout::ANY_ACCESSOR`. When a dynobj property GET
/// reads this tag, `value` is the `AccessorPair` pointer and the emit
/// branches to `any_accessor_get` instead of `any_box`.
const ANY_ACCESSOR_TAG: i64 = 6;

/// Box a dynobj GET result for a probe over an `any` receiver (RFC
/// 20260714-objlit-accessor blade 5): a `tag == 6` (`ANY_ACCESSOR`)
/// entry dispatches the getter, any other tag NaN-boxes `(tag, value)`
/// as an OWNED data property (chunk 717 — the data arm takes
/// `any_payload_rc_inc` on the borrowed pair; consumers key off
/// `owned_member_reads` to take the release over). Leaves
/// `ctx.cur_block` at the merge. The accessor arm routes through `__torajs_any_accessor_get(recv,
/// key, pair)` so a STRUCT accessor — which has no `AccessorPair` cell
/// to invoke, only a layout slot / dispatch-table adapter keyed off the
/// receiver — reaches its getter with `this` bound. A dynobj receiver
/// answers a non-zero pair and the kernel invokes it exactly as before.
///
/// The probe pair itself never invokes: it runs TWICE (once per
/// channel), and a getter runs once per read (ES §10.1.8).
pub(crate) fn emit_any_get_result(
    ctx: &mut LowerCtx,
    recv: &Operand,
    key: ValueId,
    tag: ValueId,
    value: ValueId,
) -> Operand {
    emit_get_result(ctx, tag, value, recv.clone(), key)
}

/// The two-arm emit behind [`emit_any_get_result`]. (The dynobj-only
/// variant — a closure's props bag addressed without a receiver — retired
/// with the T-27 inline read in r505; every probe now carries the `any`
/// receiver and key.)
fn emit_get_result(
    ctx: &mut LowerCtx,
    tag: ValueId,
    value: ValueId,
    recv: Operand,
    key: ValueId,
) -> Operand {
    let is_acc = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(tag),
            Operand::ConstI64(ANY_ACCESSOR_TAG),
        ),
        Type::Bool,
        None,
    );
    let acc_blk = ctx.f.add_block();
    let data_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let res_slot = ctx.alloca(Type::Any, Some("__get_res"));
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_acc),
            then_blk: acc_blk,
            else_blk: data_blk,
        },
    );
    // accessor path: `value` is the AccessorPair pointer (0 = a struct
    // accessor, which the receiver-aware kernel resolves by key).
    ctx.cur_block = acc_blk;
    let getr = ctx.f.append_inst(
        acc_blk,
        InstKind::Call(
            ctx.intrinsics.any_accessor_get,
            vec![recv, Operand::Value(key), Operand::Value(value)],
        ),
        Type::Any,
        None,
    );
    // The getter runs user code — route its pending throw before the
    // result is touched (pre-fix the pending flag stayed latent and a
    // throwing getter read fell through as undefined; RFC
    // 20260713-accessor-void-kind). Data reads never take this arm.
    ctx.emit_throw_check(None);
    let acc_cont = ctx.cur_block;
    ctx.f.append_void(
        acc_cont,
        InstKind::Store(Operand::Value(getr), Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(acc_cont, Terminator::Br(after));
    // data path: take an owned stake on the heap payload (immediates
    // no-op), then NaN-box `(tag, value)`. `any_box` itself is a pure
    // bit-encode (`__torajs_anyv_box_from_pair` takes no ref), so the
    // inc is what turns the borrowed pair into an owned result.
    ctx.cur_block = data_blk;
    ctx.f.append_void(
        data_blk,
        InstKind::Call(
            ctx.intrinsics.any_payload_rc_inc,
            vec![Operand::Value(tag), Operand::Value(value)],
        ),
    );
    let box_v = ctx.f.append_inst(
        data_blk,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag), Operand::Value(value)],
        ),
        Type::Any,
        None,
    );
    ctx.f.append_void(
        data_blk,
        InstKind::Store(Operand::Value(box_v), Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(data_blk, Terminator::Br(after));
    ctx.cur_block = after;
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}

/// Emit an accessor (`{ get, set }`) `defineProperty` on a dynobj-backed
/// Any object: lower the getter/setter closures, build an `AccessorPair`
/// (its `+1` ref moves into the dynobj entry — no extra rc_inc), and
/// store the pair cell as the property's value with the descriptor's
/// enumerable/configurable attributes (accessors carry no `writable`).
/// Returns `true` (handled).
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_accessor_define(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    key: &DefineKey,
    receiver_ident: &Option<String>,
    get_eid: Option<ExprId>,
    set_eid: Option<ExprId>,
    enumerable: Option<bool>,
    configurable: Option<bool>,
) -> bool {
    // Receiver cell — an Any receiver unboxes to its dynobj/Arr cell;
    // a typed Arr receiver (RFC 20260713 chunk C) is already the cell
    // (kernel element writes are kind-aware — mark at the reflection
    // boundary). `dynobj_define`'s apply core carries the TAG_ARR
    // dispatch into the array index kernel either way.
    let obj_is_any = matches!(ctx.operand_ty(&obj_op), Type::Any);
    let obj_ptr: Operand = if obj_is_any {
        Operand::Value(ctx.any_unbox_value_as_ptr(obj_op))
    } else {
        ctx.emit_arr_mark_kind(&obj_op);
        obj_op
    };
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(obj_ptr, Operand::Value(slot), 0),
    );
    emit_accessor_define_into(ctx, slot, key, get_eid, set_eid, enumerable, configurable);
    if obj_is_any {
        ctx.emit_any_dynobj_writeback(receiver_ident, slot);
    }
    true
}

/// [`emit_accessor_define`] against a CALLER-owned relocation slot
/// holding the raw dynobj pointer — the define kernel's resize frees
/// the old block and writes the live pointer back through it
/// (rotation 174 chunk 3: an internal throwaway slot lost the
/// relocation when the accessor was the capacity-filling literal
/// field, probe /tmp/p8b-22.ts). Caller: the object-literal accessor
/// shorthand (`emit_dynobj_accessor_field`), which owns the shared
/// init slot.
pub(crate) fn emit_accessor_define_into(
    ctx: &mut LowerCtx,
    slot: ValueId,
    key: &DefineKey,
    get_eid: Option<ExprId>,
    set_eid: Option<ExprId>,
    enumerable: Option<bool>,
    configurable: Option<bool>,
) -> bool {
    let (key_op, key_owned) = lower_key(ctx, key);
    let (get_op, get_kind) = match get_eid {
        Some(e) => lower_accessor_face(ctx, e, true),
        None => (Operand::ConstPtrNull, 0),
    };
    let (set_op, set_kind) = match set_eid {
        Some(e) => lower_accessor_face(ctx, e, false),
        None => (Operand::ConstPtrNull, 0),
    };
    let kinds = get_kind | (set_kind << 8);
    let pair = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.accessor_pair_new,
            vec![get_op, set_op, Operand::ConstI64(kinds)],
        ),
        Type::Ptr,
        None,
    );

    // flags_byte: value present (the pair is the stored value) +
    // enumerable / configurable present per the descriptor (default
    // false when absent) + per-face present bits (chunk D — the
    // redefine merge keeps the current face when absent). Accessors
    // have no `writable` attribute.
    let mut flags: i64 = 1 << 6;
    if get_eid.is_some() {
        flags |= 1 << 7; // DEFINE_PRESENT_GET
    }
    if set_eid.is_some() {
        flags |= 1 << 8; // DEFINE_PRESENT_SET
    }
    if let Some(b) = enumerable {
        flags |= 1 << 4;
        if b {
            flags |= 1 << 1;
        }
    }
    if let Some(b) = configurable {
        flags |= 1 << 5;
        if b {
            flags |= 1 << 2;
        }
    }

    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_define,
            vec![
                Operand::Value(slot),
                key_op.clone(),
                Operand::ConstI64(4), // ANY_HEAP — the pair stored as a cell
                Operand::Value(pair),
                Operand::ConstI64(flags),
            ],
        ),
    );
    // 刀 18 — coerced key was owned Str; drop after helper borrowed it.
    crate::ssa_lower_object_define::emit_key_release(ctx, key_op, key_owned);
    ctx.emit_throw_check(None);
    true
}
