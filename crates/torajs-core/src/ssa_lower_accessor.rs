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
//! - [`emit_dynobj_get_result`] — GET: branch a dynobj property read on
//!   the `ANY_ACCESSOR` `get_tag` sentinel, dispatching the getter
//!   (`accessor_invoke_getter`) vs the data-property `any_box`.

use crate::ast::ExprId;
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_object_define::{DefineKey, lower_key};

/// `get_tag` sentinel marking an accessor entry — mirrors
/// `torajs_dynobj::layout::ANY_ACCESSOR`. When a dynobj property GET
/// reads this tag, `value` is the `AccessorPair` pointer and the emit
/// branches to `accessor_invoke_getter` instead of `any_box`.
const ANY_ACCESSOR_TAG: i64 = 6;

/// Box a dynobj GET result, dispatching the getter when the entry is an
/// accessor. `tag` / `value` are the `dynobj_get_tag` /
/// `dynobj_get_value` results: an [`ANY_ACCESSOR_TAG`] tag means
/// `value` is the `AccessorPair` pointer — call `accessor_invoke_getter`
/// (returns the getter's already-boxed result); any other tag NaN-boxes
/// `(tag, value)` as a data property. Emits a 2-way branch, leaves
/// `ctx.cur_block` at the merge, and returns the `Type::Any` result.
/// Data-property reads pay one well-predicted `tag == 6` compare.
///
/// Chunk 717 — the result is OWNED on every arm: the accessor arm's
/// getter answer already carries its own ref, and the data arm takes
/// `any_payload_rc_inc` on the borrowed `(tag, value)` pair before
/// boxing (heap payloads +1, immediates no-op). Pre-717 the data arm
/// was borrow-shaped while the accessor arm was owned — the join mixed
/// ownership and the owned arms' +1 had no release site (32B leaked
/// per accessor/special-prop read). Consumers key off
/// `owned_member_reads` to take the release over.
pub(crate) fn emit_dynobj_get_result(ctx: &mut LowerCtx, tag: ValueId, value: ValueId) -> Operand {
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
    // accessor path: `value` is the AccessorPair pointer.
    ctx.cur_block = acc_blk;
    let pair = ctx.f.append_inst(
        acc_blk,
        InstKind::IntToPtr(Operand::Value(value)),
        Type::Ptr,
        None,
    );
    let getr = ctx.f.append_inst(
        acc_blk,
        InstKind::Call(
            ctx.intrinsics.accessor_invoke_getter,
            vec![Operand::Value(pair)],
        ),
        Type::Any,
        None,
    );
    ctx.f.append_void(
        acc_blk,
        InstKind::Store(Operand::Value(getr), Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(acc_blk, Terminator::Br(after));
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

/// Map an SSA value type to the accessor value-ABI kind the runtime
/// invoke path keys on (mirrors `torajs-dynobj::accessor`'s
/// `ACC_KIND_*`: 0=Any 1=I64 2=F64 3=Bool 4=Ptr). Drives which
/// fn-pointer transmute + box the getter/setter dispatch uses across
/// the register-class boundary.
fn accessor_kind_of(ty: &Type) -> i64 {
    match ty {
        Type::I64 | Type::I32 => 1,
        Type::F64 => 2,
        Type::Bool => 3,
        Type::Any => 0,
        t if t.is_refcounted() => 4,
        _ => 0,
    }
}

/// Getter ret kind — the closure's SSA return type → [`accessor_kind_of`].
fn accessor_ret_kind(ctx: &LowerCtx, op: &Operand) -> i64 {
    match ctx.operand_ty(op) {
        Type::Closure(sid) | Type::FnSig(sid) => accessor_kind_of(&ctx.fn_sigs[sid.0 as usize].1),
        _ => 0,
    }
}

/// Setter param kind — the closure's user value parameter (param 1,
/// after the env-first lifted `__env`) → [`accessor_kind_of`].
fn accessor_param_kind(ctx: &LowerCtx, op: &Operand) -> i64 {
    match ctx.operand_ty(op) {
        Type::Closure(sid) | Type::FnSig(sid) => ctx.fn_sigs[sid.0 as usize]
            .0
            .get(1)
            .map_or(0, accessor_kind_of),
        _ => 0,
    }
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
    let key_op = lower_key(ctx, key);
    let (get_op, get_kind) = match get_eid {
        Some(e) => {
            let op = ctx.lower_expr(e);
            let k = accessor_ret_kind(ctx, &op);
            (op, k)
        }
        None => (Operand::ConstPtrNull, 0),
    };
    let (set_op, set_kind) = match set_eid {
        Some(e) => {
            let op = ctx.lower_expr(e);
            let k = accessor_param_kind(ctx, &op);
            (op, k)
        }
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
    // false when absent). Accessors have no `writable` attribute.
    let mut flags: i64 = 1 << 6;
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

    let dynobj = ctx.any_unbox_value_as_ptr(obj_op);
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_define,
            vec![
                Operand::Value(slot),
                key_op,
                Operand::ConstI64(4), // ANY_HEAP — the pair stored as a cell
                Operand::Value(pair),
                Operand::ConstI64(flags),
            ],
        ),
    );
    ctx.emit_throw_check(None);
    ctx.emit_any_dynobj_writeback(receiver_ident, slot);
    true
}
