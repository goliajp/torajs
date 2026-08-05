//! `Object.defineProperty` / `Object.defineProperties` lowering —
//! carved out of `ssa_lower.rs::lower_expr_inner` so the Object
//! property-descriptor trunk (RFC
//! `.claude/rfcs/20260613-object-property-descriptors/`) grows here
//! instead of the 27k-line god-file.
//!
//! [`emit_define_one`] is the shared single-property core: a literal
//! `ObjectLit` descriptor is extracted at compile time and routed to
//! `dynobj_define` (Any obj) / `arr_set_length_validate` +
//! `arrprops_set` (Array obj) per spec §10.1.6.3 (see the [`literal`]
//! submodule); a runtime descriptor expression (RFC C1) is routed to
//! `dynobj_define_from_desc`, which reads the fields off the `desc`
//! dynobj at runtime.
//!
//! - [`try_lower_define_property`] — `Object.defineProperty(obj, key,
//!   desc)`: one `emit_define_one` over `(args[0], args[1], args[2])`.
//! - [`try_lower_define_properties`] — `Object.defineProperties(obj,
//!   { k1: d1, ... })` (RFC C2): when `props` is a compile-time
//!   `ObjectLit` and `obj` is an Ident, unfold to one `emit_define_one`
//!   per field. Nested descriptors stored inside an `any` object aren't
//!   readable as dynobjs (their fields don't round-trip through dynamic
//!   member access), so a runtime `props` variable can't be walked —
//!   that shape falls through to the prior no-op.

mod literal;

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Property key for [`emit_define_one`] — either an expression
/// (`defineProperty`'s `key` arg) or a literal field name
/// (`defineProperties`' unfolded keys).
pub(crate) enum DefineKey<'a> {
    Expr(ExprId),
    Name(&'a str),
}

/// RFC C4b — spec §10.1.6.3 step 1 receiver `Type(O) is Object` check
/// for `Object.defineProperty(O, ...)` / `Object.defineProperties(O, ...)`.
/// Strict Type(O) (no ToObject wrapper boxing): every primitive throws,
/// including `string` / `number` / `boolean` / `bigint` / `symbol`.
///
/// Static dispatch by the receiver's frontend type so the existing
/// typed-object / typed-Array / Any-dynobj paths downstream stay
/// reachable through their original `obj_op`:
/// * `Any` — call the runtime guard; the helper throws on primitive
///   imm / cell tags and returns silently on real object cells.
/// * Typed object — already an object, no guard needed.
/// * Anything else (`Undefined` / `Null` literal or typed primitive
///   Number / Bool / Str / Substr / Symbol / BigInt) — box the
///   receiver once and call the helper; the helper always throws on
///   these AnyValue patterns, so the post-`throw_check` normal block
///   is unreachable at runtime and `box_to_any`'s payload inc cannot
///   accumulate to a leak.
pub(crate) fn emit_receiver_typecheck(
    ctx: &mut LowerCtx,
    obj_eid: ExprId,
    obj_op: &Operand,
    obj_ty: Type,
) {
    if matches!(obj_ty, Type::Any) {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.throw_typeerror_if_not_object,
                vec![obj_op.clone()],
            ),
        );
        ctx.emit_throw_check(None);
        return;
    }
    if is_typed_object(obj_ty) {
        return;
    }
    let boxed = ctx.box_to_any_from_expr(obj_eid, obj_op.clone());
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_typeerror_if_not_object, vec![boxed]),
    );
    ctx.emit_throw_check(None);
}

/// Frontend types that are spec-`Type(O) is Object` so RFC C4b's
/// receiver guard can skip them — the SSA-level type already proves
/// the receiver is an object cell.
pub(crate) fn is_typed_object(ty: Type) -> bool {
    matches!(
        ty,
        Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::FnSig(_)
            | Type::RegExp
            | Type::Date
            | Type::Promise
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
    )
}

/// Lower the key to a property-key operand — a `Str` or, per §7.1.19
/// ToPropertyKey **step 2**, a `Symbol` passed through untouched. Call
/// at the spec evaluation point (after `obj`, before the descriptor) so
/// a side-effecting key expr orders correctly.
///
/// Returns `(key_op, owned)` — `owned == true` means the caller must
/// emit a `str_drop` on `key_op` after the runtime helper borrows it
/// (RFC 20260716 刀 18 — ToPropertyKey coerce). `false` covers the
/// interned-literal `DefineKey::Name` shape and the `Type::Str` /
/// `Type::Symbol` Expr fast paths (all borrow-shaped shares of a
/// stable key cell).
pub(crate) fn lower_key(ctx: &mut LowerCtx, key: &DefineKey) -> (Operand, bool) {
    match key {
        DefineKey::Expr(eid) => {
            let raw = ctx.lower_expr(*eid);
            let ty = ctx.operand_ty(&raw);
            match ty {
                // §7.1.19 step 2 — "If key is a Symbol, return key".
                // A symbol key never reaches step 3's ToString (which
                // §7.1.17 makes a TypeError for symbols anyway); it
                // lands in the dynobj key slot as its own cell, where
                // the pointed-to tag keeps it distinct from every
                // string key. Same borrow shape as a `Type::Str` key —
                // the kernel rc-incs its own share.
                Type::Str | Type::Symbol => (raw, false),
                // An `any` key only reveals its kind at run time, and
                // step 2 applies to it just as much: a symbol that
                // happens to be sitting in an `any` must arrive as
                // itself. Stringifying it stored the property under
                // "Symbol(x)" and put that name into `Object.keys`,
                // where no symbol belongs. The kernel answers the key
                // cell for either kind, owned.
                Type::Any => {
                    let k = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.anyv_to_property_key, vec![raw.clone()]),
                        Type::Ptr,
                        None,
                    );
                    // The answer carries its own share either way, so
                    // a key that was a fresh temp is done being read —
                    // released here rather than after the throw check,
                    // which is a path that never comes back.
                    ctx.release_owned_temp(*eid, &raw);
                    ctx.emit_throw_check(None);
                    (Operand::Value(k), true)
                }
                // RFC 20260716 刀 18 — ES §20.1.2.6 step 1 / §20.1.2.10
                // step 1 → §7.1.19 ToPropertyKey → §7.1.17 ToString.
                // StringWrapper / Number / Boolean / etc. keys route
                // through `emit_to_string`; returned Str is owned and
                // callers drop after the helper borrow read.
                _ => {
                    let coerced =
                        crate::ssa_lower_call_coercion::emit_to_string(ctx, *eid, raw, ty, false);
                    (coerced, true)
                }
            }
        }
        DefineKey::Name(n) => (Operand::Value(ctx.intern_string_literal(n)), false),
    }
}

/// Release what [`lower_key`] handed out, if it handed out a share.
///
/// Paired with the constructor because the answer's kind is no longer
/// knowable at the call site: an `any` key resolves to a Str or to a
/// Symbol at run time, and a Symbol released through `str_drop` walks
/// the wrong layout. Every consumer of `lower_key` releases here, so
/// the two cannot drift apart.
pub(crate) fn emit_key_release(ctx: &mut LowerCtx, key_op: Operand, key_owned: bool) {
    if !key_owned {
        return;
    }
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.anyv_property_key_drop, vec![key_op]),
    );
}

/// Lower a define-family receiver. An inline ObjectLit receiver
/// promotes to the dynobj lane and answers an ANY_HEAP face (twin of
/// the direct-ObjectLit call-arg route in `ssa_lower_call_terminal`
/// and the `{} as any` promote in `ssa_lower_any_cast`): the struct
/// lane has no dynobj backing store, so `Object.defineProperty({},
/// k, { get })` silently no-opped the accessor install and every
/// subsequent read answered undefined (test262 dstr poisoned-getter
/// cluster, exposed once §20.1.2.5 started returning O).
pub(crate) fn lower_define_receiver(ctx: &mut LowerCtx, obj_eid: ExprId) -> Operand {
    if matches!(ctx.ast.get_expr(obj_eid), Expr::ObjectLit { .. }) {
        let dynobj = ctx.lower_dynobj_init(obj_eid);
        return ctx.box_to_any(dynobj);
    }
    ctx.lower_expr(obj_eid)
}

/// Emit one `Object.defineProperty(obj, key, desc)`-equivalent. `obj` is
/// re-lowered from `obj_eid` (so `defineProperties` can re-read the
/// receiver variable after a prior field resized it). Returns
/// `Some((obj_op, obj_ty))` when handled — the lowered receiver, so
/// `try_lower_define_property` can answer `O` per §20.1.2.5 instead of
/// undefined (test262 `__lookupGetter__` cluster's `Object.create(
/// defineProperty(...), ...)` used to hit "Object prototype may only be
/// an Object or null" here); `None` when neither a literal nor a
/// runtime-Any descriptor applies (caller decides the fall-through).
pub(crate) fn emit_define_one(
    ctx: &mut LowerCtx,
    obj_eid: ExprId,
    key: DefineKey,
    desc_eid: ExprId,
) -> Option<(Operand, Type)> {
    // Step 7d-A — capture the receiver's Ident name (if any) so the
    // dynobj-define Any path can writeback the post-resize ptr to the
    // variable's storage as a fresh NaN-box AnyValue.
    let receiver_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(obj_eid) {
        Some(n.clone())
    } else {
        None
    };
    let obj_op = lower_define_receiver(ctx, obj_eid);
    let obj_ty = ctx.operand_ty(&obj_op);

    emit_receiver_typecheck(ctx, obj_eid, &obj_op, obj_ty.clone());
    if emit_define_one_core(
        ctx,
        obj_op.clone(),
        obj_ty.clone(),
        &receiver_ident,
        key,
        desc_eid,
    ) {
        Some((obj_op, obj_ty))
    } else {
        None
    }
}

/// [`emit_define_one`] with the receiver already lowered and
/// typechecked — `defineProperties`' non-Ident-receiver unfold lowers
/// the receiver once and feeds every field through here (a per-field
/// re-lower of an ObjectLit receiver would mint a fresh object each
/// time).
pub(crate) fn emit_define_one_core(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: Type,
    receiver_ident: &Option<String>,
    key: DefineKey,
    desc_eid: ExprId,
) -> bool {
    // Compile-time literal descriptor — extract value + the three data
    // flags from the ObjectLit at compile time. The fast path declines
    // (false) when a flag field is not a Bool literal; the descriptor
    // then materializes as a dynobj and takes the full runtime
    // ToPropertyDescriptor path (§6.2.6.5 ToBoolean semantics).
    if matches!(ctx.ast.get_expr(desc_eid), Expr::ObjectLit { .. }) {
        if literal::emit_define_literal(
            ctx,
            obj_op.clone(),
            obj_ty.clone(),
            &key,
            receiver_ident,
            desc_eid,
        ) {
            return true;
        }
        return emit_define_objlit_runtime(ctx, obj_op, obj_ty, &key, receiver_ident, desc_eid);
    }
    crate::ssa_lower_object_define_runtime::emit_define_runtime_desc(
        ctx,
        obj_op,
        obj_ty,
        &key,
        receiver_ident,
        desc_eid,
    )
}

/// Literal descriptor the fast path declined — materialize the
/// ObjectLit as a fresh dynobj (owned temp, released after the call)
/// and route through `__torajs_dynobj_define_from_desc`, whose
/// `define_apply` entry carries the Arr receiver dispatch.
fn emit_define_objlit_runtime(
    ctx: &mut LowerCtx,
    obj_op: Operand,
    obj_ty: Type,
    key: &DefineKey,
    receiver_ident: &Option<String>,
    desc_eid: ExprId,
) -> bool {
    let obj_ptr: Operand = match &obj_ty {
        Type::Any => Operand::Value(ctx.any_unbox_value_as_ptr(obj_op)),
        Type::Arr(_) => {
            // Kernel element writes are kind-aware — mark at the
            // reflection boundary (mirror of the literal Arr arm).
            ctx.emit_arr_mark_kind(&obj_op);
            obj_op
        }
        // Typed Closure receiver (RFC 20260721 刀 2) and a class
        // instance — the operand is the cell ptr; the kernel's
        // receiver dispatch targets whichever +24 expando dict the
        // tag names (neither cell relocates, no writeback below).
        Type::Closure(_) | Type::FnSig(_) | Type::Obj(_) => obj_op,
        // Typed Date / RegExp / Error — no expando define storage yet
        // (RFC 20260721 刀 2b backlog). Handled (no-op).
        _ => return true,
    };
    let (key_op, key_owned) = lower_key(ctx, key);
    let desc_ptr = ctx.lower_dynobj_init(desc_eid);
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(obj_ptr, Operand::Value(slot), 0),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_define_from_desc,
            vec![Operand::Value(slot), key_op.clone(), desc_ptr.clone()],
        ),
    );
    // Release the fresh descriptor before the throw-check (mirror of
    // Object.create's literal-props drop).
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.value_drop_heap, vec![desc_ptr]),
    );
    // 刀 18 — coerced key was owned Str; drop after helper borrowed it.
    emit_key_release(ctx, key_op, key_owned);
    ctx.emit_throw_check(None);
    if matches!(obj_ty, Type::Any) {
        ctx.emit_any_dynobj_writeback(receiver_ident, slot);
    }
    true
}

/// Runtime descriptor (RFC C1) — obj is Any (dynobj-backed) or a
/// typed Array (define_apply's TAG_ARR dispatch); desc is Any or a
/// typed Closure heap cell (its shape resolves at the
/// `define_from_desc` entry — a Closure descriptor reads through its
/// expando props dynobj). Key is lowered before desc to preserve
/// obj → key → desc evaluation order.
/// Dispatch `Object.defineProperty` / `Object.defineProperties` — the
/// single entry `lower_expr_inner` calls. `Some` when handled; `None` to
/// fall through.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(v) = try_lower_define_property(ctx, callee_eid, args) {
        return Some(v);
    }
    crate::ssa_lower_object_define_properties::try_lower_define_properties(ctx, callee_eid, args)
}

/// Lower `Object.defineProperty(obj, key, descriptor)`. Returns `Some`
/// when handled; `None` to fall through (non-Any obj+desc shapes keep
/// the prior "unsupported member call shape" panic).
fn try_lower_define_property(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee_eid)
        && m_name == "defineProperty"
        && let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id)
        && ns == "Object"
        && args.len() >= 3
        && let Some((obj_op, obj_ty)) =
            emit_define_one(ctx, args[0], DefineKey::Expr(args[1]), args[2])
    {
        // S317 — ES §20.1.2.6 silently ignores args past (obj, key,
        // desc). `emit_define_one` lowers args[0..3] (obj + key +
        // desc fields); lower-and-drop args[3..] after for spec
        // left-to-right side-effect order.
        for &a in args.iter().skip(3) {
            let _ = ctx.lower_expr(a);
        }
        // §20.1.2.5 step 4 — return O. Mirror `defineProperties`'
        // owned-result invariant so caller-side drop nets out with the
        // inc here (defineProperties' RFC 20260705 line: the receiver
        // carries its own ref through the dedicated path, bypassing
        // integrity's lower_noop).
        ctx.emit_owned_result_inc(obj_op.clone(), obj_ty);
        // An owned-temp receiver (inline-ObjectLit dynobj promote /
        // fresh Call result) hands its mint stake off here — the
        // result inc above is the consumer's stake (mirror of
        // `defineProperties`' release; Ident receivers self-gate as
        // borrows).
        ctx.release_owned_temp(args[0], &obj_op);
        return Some(obj_op);
    }
    None
}

// `try_lower_define_properties` extracted to sibling
// `ssa_lower_object_define_properties.rs` (file-size-debt trim);
// dispatch call above uses the sibling entry directly.
