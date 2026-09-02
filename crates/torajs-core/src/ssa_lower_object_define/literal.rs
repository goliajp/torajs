//! Compile-time literal-descriptor arm of [`super::emit_define_one`] —
//! the `Expr::ObjectLit` descriptor is decoded at compile time and
//! routed per receiver type: accessor pair (RFC C3) / Array
//! DefineOwnProperty kernel (RFC 20260712-arr-exotic-define — length
//! lock, index shadow flags, expando) / typed no-op / dynobj_define
//! main path (spec §10.1.6.3).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

use super::{DefineKey, lower_key};

/// Compile-time literal descriptor — extract value + the three data
/// flags from the ObjectLit at compile time, then route per receiver
/// type. Returns `true` when handled (same contract as
/// [`super::emit_define_one`]).
pub(super) fn emit_define_literal(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: Type,
    key: &DefineKey,
    receiver_ident: &Option<String>,
    desc_eid: ExprId,
) -> bool {
    // The compile-time flags byte can only represent Bool-literal
    // flag fields; any other expression carries §6.2.6.5 ToBoolean
    // semantics (`enumerable: -9` is present + true) that a silent
    // absent-treatment would drop. Decline — the caller falls back
    // to the runtime ToPropertyDescriptor path.
    if !flags_statically_decodable(ctx, desc_eid) {
        return false;
    }
    let value_eid = descriptor_field(ctx, desc_eid, "value");

    // RFC C3 — accessor (get/set) descriptor. Per spec §6.2.5 an
    // accessor descriptor is mutually exclusive with a data
    // `value`; when the literal carries a `get` and/or `set`
    // function, store an `AccessorPair` cell instead of a data
    // value. Dynobj-backed Any objects and typed Arrays carry
    // accessor storage (RFC 20260713 chunk C routes the Arr receiver
    // through `dynobj_define`'s TAG_ARR dispatch into the index
    // kernel); typed Structs stay the prior no-op.
    let get_eid = descriptor_field(ctx, desc_eid, "get");
    let set_eid = descriptor_field(ctx, desc_eid, "set");
    if (get_eid.is_some() || set_eid.is_some()) && matches!(obj_ty, Type::Any | Type::Arr(_)) {
        // §6.2.6.5 — a present face that is not statically a plausible
        // callable (`get: []` / `get: false`) declines the fast path;
        // the runtime ToPropertyDescriptor's IsCallable check throws
        // the spec TypeError (pre-fix the face's pointer was stored in
        // the AccessorPair verbatim — an Arr cell invoked as a
        // closure). RFC 20260713-defprop-residual-cluster chunk B.
        if !face_statically_callable(ctx, get_eid) || !face_statically_callable(ctx, set_eid) {
            return false;
        }
        // §6.2.6.5 steps 9/10 — a literal mixing an accessor face
        // with `value` / `writable` is the spec TypeError; decline
        // so the runtime ToPropertyDescriptor's mix rejection
        // throws it (pre-fix the fast path stored the AccessorPair
        // and silently DROPPED the value field).
        if value_eid.is_some() || descriptor_field(ctx, desc_eid, "writable").is_some() {
            return false;
        }
        let acc_enum = lookup_bool_field(ctx, desc_eid, "enumerable");
        let acc_config = lookup_bool_field(ctx, desc_eid, "configurable");
        return crate::ssa_lower_accessor::emit_accessor_define(
            ctx,
            obj_op,
            key,
            receiver_ident,
            get_eid,
            set_eid,
            acc_enum,
            acc_config,
        );
    }

    let flags_byte = compute_flags_byte(ctx, desc_eid, value_eid.is_some());

    // Arr receiver — every key (length included) routes through the
    // DefineOwnProperty kernel: chunk D moved the §10.4.2.4 length
    // arm (writable lock + e/c validation) inside it, so the old
    // value-only length shortcut would drop flag-only descriptors.
    if matches!(obj_ty, Type::Arr(_)) {
        emit_define_arr_prop(ctx, obj_op, key, value_eid, flags_byte);
        return true;
    }
    // Typed Closure receiver (T-27 Function-as-Object, RFC 20260721
    // 刀 2) — the operand is the cell ptr; the kernel's closure arm
    // defines onto the +24 expando dynobj.
    if matches!(obj_ty, Type::Closure(_) | Type::FnSig(_)) {
        emit_define_dynobj(
            ctx,
            obj_op,
            &obj_ty,
            key,
            receiver_ident,
            value_eid,
            flags_byte,
        );
        return true;
    }
    // A class instance DECLINES rather than claiming a no-op: the
    // objlit-runtime road defines into its `+24` expando dict, and
    // sending every struct spelling down that one road is what keeps
    // them agreeing. It costs a descriptor materialization the
    // compile-time flag extraction would have saved — worth it while
    // there is one writer; a struct-aware fast path can come back
    // once the read side stops being the only thing that has one.
    if matches!(obj_ty, Type::Obj(_)) {
        return false;
    }
    // Non-Any/non-Arr obj (typed Date / RegExp / Error) — no expando
    // define storage yet (RFC 20260721 刀 2b backlog). Handled
    // (no-op).
    if !matches!(obj_ty, Type::Any) {
        return true;
    }
    emit_define_dynobj(
        ctx,
        obj_op,
        &Type::Any,
        key,
        receiver_ident,
        value_eid,
        flags_byte,
    );
    true
}

/// Whether a present accessor face is statically a plausible callable
/// so the compile-time accessor arm may store it in an AccessorPair:
/// fn expressions, `undefined` (present-and-clearing), and bindings
/// whose static type is Closure / FnSig / Any (Any resolves at the
/// runtime IsCallable check). Anything else — literals, array/object
/// literals, arbitrary exprs — answers false and the caller declines
/// to the runtime ToPropertyDescriptor path, whose
/// `take_accessor_closure` throws the §6.2.6.5 "not callable"
/// TypeError. Conservative: a false negative only costs the slow
/// path.
fn face_statically_callable(ctx: &LowerCtx, face_eid: Option<ExprId>) -> bool {
    let Some(eid) = face_eid else {
        return true;
    };
    match ctx.ast.get_expr(eid) {
        Expr::Closure { .. } => true,
        Expr::Ident(n) if n == "undefined" && !ctx.locals.contains_key("undefined") => true,
        Expr::Ident(n) => {
            ctx.locals.get(n).is_some_and(|info| {
                matches!(info.ty, Type::Closure(_) | Type::FnSig(_) | Type::Any)
            }) || ctx.fn_table.contains_key(n)
        }
        _ => false,
    }
}

/// Field expr of the literal descriptor by name (`value` / `get` /
/// `set`), or `None` when absent (or the descriptor isn't an ObjectLit).
fn descriptor_field(ctx: &LowerCtx, desc_eid: ExprId, name: &str) -> Option<ExprId> {
    match ctx.ast.get_expr(desc_eid) {
        Expr::ObjectLit { fields } => fields.iter().find(|(n, _)| n == name).map(|(_, e)| *e),
        _ => None,
    }
}

/// Whether every attribute flag field of the literal descriptor is a
/// Bool literal (or absent) — the fast path's legality condition.
/// `value` / `get` / `set` are unconstrained (arbitrary exprs lower
/// normally).
fn flags_statically_decodable(ctx: &LowerCtx, desc_eid: ExprId) -> bool {
    if let Expr::ObjectLit { fields } = ctx.ast.get_expr(desc_eid) {
        for (n, e) in fields {
            if matches!(n.as_str(), Some("writable" | "enumerable" | "configurable"))
                && !matches!(ctx.ast.get_expr(*e), Expr::Bool(_))
            {
                return false;
            }
        }
    }
    true
}

/// Boolean flag field of the literal descriptor. Each flag is
/// `Bool(true)` / `Bool(false)` when present; absent (or non-literal,
/// treated as absent) fields stay `None`.
fn lookup_bool_field(ctx: &LowerCtx, desc_eid: ExprId, field_name: &str) -> Option<bool> {
    if let Expr::ObjectLit { fields } = ctx.ast.get_expr(desc_eid) {
        for (n, e) in fields {
            if n == field_name {
                if let Expr::Bool(b) = ctx.ast.get_expr(*e) {
                    return Some(*b);
                }
                return None;
            }
        }
    }
    None
}

/// Data-descriptor flags byte for `dynobj_define` — value bits 0..=2
/// (writable / enumerable / configurable) + presence bits 3..=6
/// (per-flag present + value present).
fn compute_flags_byte(ctx: &LowerCtx, desc_eid: ExprId, value_present: bool) -> i64 {
    let mut flags_byte: i64 = 0;
    if let Some(b) = lookup_bool_field(ctx, desc_eid, "writable") {
        flags_byte |= 1 << 3; // present
        if b {
            flags_byte |= 1 << 0;
        }
    }
    if let Some(b) = lookup_bool_field(ctx, desc_eid, "enumerable") {
        flags_byte |= 1 << 4;
        if b {
            flags_byte |= 1 << 1;
        }
    }
    if let Some(b) = lookup_bool_field(ctx, desc_eid, "configurable") {
        flags_byte |= 1 << 5;
        if b {
            flags_byte |= 1 << 2;
        }
    }
    if value_present {
        flags_byte |= 1 << 6; // value present
    }
    flags_byte
}

/// Tag-pack helper — same table the BinOp Any===concrete arm uses for
/// runtime tag values (`torajs_rc::AnySlotTag`).
///
/// The tag is an operand rather than a constant because one source
/// type cannot name it: an `Any` carries its own tag in its NaN box
/// and only says which at runtime.
///
/// Whatever a refcounted arm hands back is `+1`, which is the
/// kernel's transfer contract; the caller releases the temp's own
/// stake afterwards.
fn pack_tagged_value(
    ctx: &mut LowerCtx,
    v_eid: ExprId,
    v_raw: Operand,
    v_ty: Type,
) -> (Operand, Operand) {
    match v_ty {
        Type::I64 | Type::I32 => (Operand::ConstI64(2), v_raw),
        Type::F64 => {
            let bits = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BitCastF64ToI64(v_raw),
                Type::I64,
                None,
            );
            (Operand::ConstI64(3), Operand::Value(bits))
        }
        Type::Bool => {
            let zext = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ZExtBoolToI64(v_raw),
                Type::I64,
                None,
            );
            (Operand::ConstI64(1), Operand::Value(zext))
        }
        // An `Any` is a NaN box, not a cell pointer. Tagging it
        // `Heap` handed the kernel the box's BITS as the address to
        // store, and retaining it through the raw `rc_inc` — which
        // dereferences unconditionally — read a header out of an
        // immediate. `var v = 1; Object.defineProperty(a, "p",
        // { value: v, … })` therefore segfaulted, while the same
        // define with a literal or a `let` (both of which reach here
        // already typed) was fine. Ask the box what it holds instead:
        // `anyv_unbox_tag` answers in this very table, and the
        // any-aware retain is a no-op on an immediate.
        Type::Any => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_box_rc_inc, vec![v_raw.clone()]),
            );
            let tag = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![v_raw.clone()]),
                Type::I64,
                None,
            );
            let val = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_unbox_value, vec![v_raw]),
                Type::I64,
                None,
            );
            (Operand::Value(tag), Operand::Value(val))
        }
        // Mirror of `box_to_tag_value`'s refcounted arm: the slot
        // takes its +1 through the rc helper, and a typed array
        // crossing into the any world records its element kind on
        // the header — without the mark, `{ value: arr }` stored a
        // raw-i64 array that every any-side reader (print / index /
        // drop walkers) NaN-box-walked and SIGSEGVed on (rotation
        // 561).
        _ if v_ty.is_refcounted() => {
            ctx.emit_rc_inc(v_raw.clone());
            ctx.emit_arr_mark_kind(&v_raw);
            (Operand::ConstI64(4), v_raw)
        }
        // S127-1 twin — undefined and null both collapse to
        // ConstPtrNull at the value layer; the checker's static type
        // picks the tag (ToUint32(undefined)=0 vs ToNumber=NaN makes
        // `defineProperty(arr, "length", {value: undefined})` a
        // RangeError, which a null-tagged pack silently passed).
        Type::Ptr if matches!(v_raw, Operand::ConstPtrNull) => {
            if matches!(
                ctx.expr_types.get(&v_eid),
                Some(crate::check::Type::Undefined)
            ) {
                (Operand::ConstI64(5), Operand::ConstI64(0))
            } else {
                (Operand::ConstI64(0), Operand::ConstI64(0))
            }
        }
        _ => (Operand::ConstI64(0), Operand::ConstI64(0)),
    }
}

/// Array obj (non-"length" key) — RFC 20260712-arr-exotic-define
/// chunk B: route through the Array DefineOwnProperty kernel
/// (§10.4.2.1 canonical-index vs expando dispatch + §10.1.6.3
/// validation + per-index attribute shadow entries). Pre-fix this
/// mis-routed to `arrprops_set` (index defines landed in the expando
/// dynobj where element reads never look) and dropped value-less
/// descriptors entirely (a generic descriptor must still create the
/// property).
fn emit_define_arr_prop(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    key: &DefineKey,
    value_eid: Option<ExprId>,
    flags_byte: i64,
) {
    // The kernel's element writes are kind-aware; a typed array that
    // never crossed the any boundary carries ARR_KIND_UNSET, so mark
    // it here (defineProperty is a reflection boundary like boxing).
    ctx.emit_arr_mark_kind(&obj_op);
    let (key_op, key_owned) = lower_key(ctx, key);
    // Rotation 549 — a coerced key is alive across the value lower
    // (which may throw); park it for those throw paths.
    let key_tok = if key_owned {
        Some(ctx.push_throw_temp(key_op.clone(), Type::Str))
    } else {
        None
    };
    let mut owned_val: Option<(ExprId, Operand)> = None;
    let (tag, val_op) = if let Some(val_eid) = value_eid {
        let v_raw = ctx.lower_expr(val_eid);
        let v_ty = ctx.operand_ty(&v_raw);
        if v_ty.is_refcounted() {
            owned_val = Some((val_eid, v_raw.clone()));
        }
        pack_tagged_value(ctx, val_eid, v_raw, v_ty)
    } else {
        (Operand::ConstI64(0), Operand::ConstI64(0))
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_define,
            vec![
                obj_op,
                key_op.clone(),
                tag,
                val_op,
                Operand::ConstI64(flags_byte),
            ],
        ),
    );
    // 刀 18 — coerced key was owned Str; drop after helper borrowed it.
    if let Some(t) = key_tok {
        ctx.pop_throw_temp(t);
    }
    super::emit_key_release(ctx, key_op, key_owned);
    // pack_tagged_value's +1 fed the kernel's transfer contract; an
    // owned-shape value temp (concat / call result) still holds its
    // own mint stake with no consumer — release it (borrow shapes
    // no-op inside).
    if let Some((val_eid, v_raw)) = owned_val {
        ctx.release_owned_temp(val_eid, &v_raw);
    }
    ctx.emit_throw_check(None);
}

/// Dynobj-backed Any obj (or a typed Closure cell — the kernel's
/// closure arm targets its expando) — route through dynobj_define so
/// spec §10.1.6.3 validates the transitions.
fn emit_define_dynobj(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: &Type,
    key: &DefineKey,
    receiver_ident: &Option<String>,
    value_eid: Option<ExprId>,
    flags_byte: i64,
) {
    let (key_op, key_owned) = lower_key(ctx, key);
    // Rotation 549 — a coerced key is alive across the value lower
    // (which may throw); park it for those throw paths.
    let key_tok = if key_owned {
        Some(ctx.push_throw_temp(key_op.clone(), Type::Str))
    } else {
        None
    };
    let mut owned_val: Option<(ExprId, Operand)> = None;
    let (tag, val_op) = if let Some(val_eid) = value_eid {
        let v_raw = ctx.lower_expr(val_eid);
        let v_ty = ctx.operand_ty(&v_raw);
        if v_ty.is_refcounted() {
            owned_val = Some((val_eid, v_raw.clone()));
        }
        pack_tagged_value(ctx, val_eid, v_raw, v_ty)
    } else {
        (Operand::ConstI64(0), Operand::ConstI64(0))
    };
    let dynobj = match obj_ty {
        Type::Any => ctx.any_unbox_value_as_ptr(obj_op),
        // Typed heap receiver — the operand IS the cell ptr (the
        // closure cell never relocates; no Any writeback below).
        _ => match obj_op {
            Operand::Value(v) => v,
            _ => ctx.any_unbox_value_as_ptr(obj_op),
        },
    };
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
                key_op.clone(),
                tag,
                val_op,
                Operand::ConstI64(flags_byte),
            ],
        ),
    );
    // 刀 18 — coerced key was owned Str; drop after helper borrowed it.
    if let Some(t) = key_tok {
        ctx.pop_throw_temp(t);
    }
    super::emit_key_release(ctx, key_op, key_owned);
    // pack_tagged_value's +1 fed the kernel's transfer contract; an
    // owned-shape value temp (concat / call result) still holds its
    // own mint stake with no consumer — release it (borrow shapes
    // no-op inside).
    if let Some((val_eid, v_raw)) = owned_val {
        ctx.release_owned_temp(val_eid, &v_raw);
    }
    ctx.emit_throw_check(None);
    if matches!(obj_ty, Type::Any) {
        ctx.emit_any_dynobj_writeback(receiver_ident, slot);
    }
}
