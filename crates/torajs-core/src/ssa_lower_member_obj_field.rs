//! P8.2 accessor read + struct-field-layout fallback Member-access
//! arm for `Type::Obj(sid)` receivers pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-66 of the decomp (chunks 1-65 = ... + FnSig/Arr-as-
//! Object props-read cluster).
//!
//! This sibling is the **terminal Member-arm path**: by the time
//! lower_expr_inner reaches it, all earlier sibling chains have
//! returned `None` and `obj_ty` is expected to be
//! `Type::Obj(sid)`. The caller panics on any other obj_ty before
//! invoking the sibling, so `try_lower` assumes a struct receiver.
//!
//! Two sub-arms tried in source order:
//!
//! - **P8.2** — accessor read: `c.value` where C declares
//!   `get value(): T`. `desugar_classes` renamed the getter's
//!   FnDecl to `__cm_<C>__<name>_get` and recorded `(C, name) →
//!   fn_name` in `ast.accessor_getters`. Reverse-lookup obj's
//!   class from the struct's sid via the aliases table (same
//!   idiom as the Phase I.1 method-dispatch path); on hit, emit
//!   a Call to the getter and propagate any throw via
//!   `emit_throw_check(Some(fid))`. Bypasses the struct-layout
//!   field lookup (the accessor name is not a real field).
//! - **Struct-field-layout fallback** — walk
//!   `struct_layouts[sid]` for a field matching `name`, compute
//!   the offset (`OBJ_HEADER_SIZE + idx*8`), and emit a
//!   `Load(field_ty, obj_val, offset)`. Panics if the field is
//!   absent (typechecker upstream rejects).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

/// RFC 20260715-nominal-class-identity — the class a receiver belongs
/// to, read off the NAME in its checked type. The layout id can't
/// answer this: `class C { a }` and `class D { a }` intern to one
/// StructId, so a reverse lookup by shape hands back whichever
/// registered first — and a plain `{a: 1}` shares that id too.
pub(crate) fn class_name_of_expr(ctx: &LowerCtx<'_>, obj: ExprId) -> Option<String> {
    match ctx.expr_types.get(&obj)? {
        crate::check::Type::ClassRef(n) if ctx.ast.class_parents.contains_key(n) => Some(n.clone()),
        _ => None,
    }
}

/// Whether `sid`'s layout is the injected Error shape — `message` /
/// `name` / `stack` as the first three Str fields, which desugar
/// field-flattening puts ahead of any subclass's own fields. Every
/// Error-derived class matches; so would a user class declaring the
/// same trio, and that is precisely the case a compile-time test
/// cannot resolve (it shares the sid), so the emitted helper decides
/// off `FLAG_ERROR` instead.
fn layout_is_error_shape(ctx: &LowerCtx<'_>, sid: crate::ssa::StructId) -> bool {
    let layout = &ctx.struct_layouts[sid.0 as usize];
    layout.len() >= 3
        && ["message", "name", "stack"]
            .iter()
            .zip(layout.iter())
            .all(|(want, (have, ty))| have == want && *ty == Type::Str)
}

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    sid: StructId,
    name: &str,
) -> Operand {
    // RFC 20260722-find-miss chunk B — a find/findLast miss (or a
    // Nullable optional-field read) answers the generic undefined
    // cell; a field read through it must be a catchable TypeError
    // (bun/JSC wording), not a deref past the bare static header.
    // No-op for plain receivers.
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &obj_val);
    if let Some(op) = try_accessor_getter(ctx, obj, obj_val, sid, name) {
        return op;
    }
    if let Some(op) = try_objlit_getter(ctx, obj_val, sid, name) {
        return op;
    }
    // RFC 20260718-error-message-own-prop 刀 2 — an Error-derived
    // receiver's `message` is runtime own-state: own-present reads
    // the slot, own-absent walks the prototype chain
    // (`Err.prototype.message` shadow → `__proto_Error`'s spec "").
    // The helper answers a BORROWED Str, exactly like the field
    // `Load` it replaces.
    //
    // §20.5.3.2 gives `name` the same treatment for a different
    // reason: the class name lives on `<C>.prototype`, so the slot
    // holds the absence sentinel until user code assigns `this.name`
    // — the ordinary read is a chain walk, not a field load.
    //
    // The gate is the LAYOUT, not the receiver's class name. A
    // receiver annotated `any` still lowers through this typed arm
    // when SSA kept its Obj shape, and there `class_name_of_expr`
    // answers None — which used to leave `(err as any).name` reading
    // the raw slot, i.e. the sentinel. Layout is also the honest
    // granularity: structurally identical classes SHARE a sid, so no
    // compile-time test can separate an error from a hypothetical
    // class declaring the same trio. The helper settles that at
    // runtime off the header's error bit.
    if matches!(name, "message" | "name") && layout_is_error_shape(ctx, sid) {
        let target = if name == "message" {
            ctx.intrinsics.error_message_get
        } else {
            ctx.intrinsics.error_name_get
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(target, vec![obj_val]),
            Type::Str,
            None,
        );
        return Operand::Value(v);
    }
    lower_struct_field(ctx, eid, obj_val, sid, name)
}

/// RFC 20260714-objlit-accessor blade 2 — `o.b` where the literal
/// declared `get b() { ... }`. The getter closure sits in the layout
/// under `__getter_b`; invoke it with the receiver (blade 1's `__mth(`
/// ABI — `(__env, __this)`) and hand back its result. The accessor being
/// a layout field is what makes it belong to the TYPE, so no
/// same-layout object can reach it (unlike the class lane above, which
/// reverse-looks-up a class name by structural equality).
fn try_objlit_getter(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    name: &str,
) -> Option<Operand> {
    let slot = format!("__getter_{name}");
    let layout = &ctx.struct_layouts[sid.0 as usize];
    let (idx, ty) = layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| (*fname == slot).then_some((i, *fty)))?;
    let Type::Closure(sig_id) = ty else {
        return None;
    };
    let offset = OBJ_HEADER_SIZE + idx as u64 * 8;
    Some(
        crate::ssa_lower_call_struct_method_dispatch::emit_receiver_closure_call(
            ctx,
            obj_val,
            offset,
            sig_id,
            &[],
        ),
    )
}

fn try_accessor_getter(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    obj_val: Operand,
    _sid: StructId,
    name: &str,
) -> Option<Operand> {
    let cname = class_name_of_expr(ctx, obj)?;
    // Blade 2 (rotation 413) — the pair may live on an ANCESTOR;
    // walk the chain the way [[Get]] would. A generic declarer's
    // getter has no fn_table entry under its bare name, so that hit
    // falls through to the any-lane on the `?` below (blade 4).
    let getter_fn = crate::ast::accessor_lookup::accessor_getter_in_chain(ctx.ast, &cname, name)?;
    let fid = ctx.fn_table.get(&getter_fn).copied()?;
    let ret_ty = ctx.f_ret_type_hint(fid);
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(fid, vec![obj_val]), ret_ty, None);
    ctx.emit_throw_check(Some(fid));
    Some(Operand::Value(v))
}

fn lower_struct_field(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj_val: Operand,
    sid: StructId,
    name: &str,
) -> Operand {
    let layout = &ctx.struct_layouts[sid.0 as usize];
    let Some((idx, field_ty)) = layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| if fname == name { Some((i, *fty)) } else { None })
    else {
        // Blade 4 (RFC 20260804-method-rebind-generic-body) — the
        // checker admitted this read as a class-instance terminal
        // miss (§10.1.8.1 [[Get]] on an absent property answers
        // undefined). Box the receiver and ride the any-member lane:
        // an expando / prototype write may have landed the name at
        // runtime, and a true miss answers ANY_UNDEF.
        let boxed_recv = ctx.box_to_any(obj_val);
        return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, boxed_recv, name);
    };
    let offset = OBJ_HEADER_SIZE + idx as u64 * 8;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    // RFC 20260809 B5 — an `any` slot's stake belongs to the SLOT
    // (chunk 563 ledger): the field-assign lane drop-olds it without
    // regard for live borrows, so a bare-Load borrow here dangles the
    // moment the field is overwritten (`const st: any = this.__s;
    // this.__s = [];` freed the array under the binding — closure
    // underflow in the at-exit drain, probe u6/u18/u23). Answer owned
    // instead — payload +1, eid recorded — exactly the contract the
    // any-RECEIVER member lane adopted in rotation 324; every consumer
    // already releases recorded owned member reads (chunk 717 track).
    // Typed slots (Str/Arr/Obj) keep the borrow contract: their
    // assign lane balances differently (probes u21/u22 clean).
    if field_ty == Type::Any {
        ctx.emit_owned_result_inc(Operand::Value(v), Type::Any);
        ctx.owned_member_reads.insert(eid);
    }
    Operand::Value(v)
}
