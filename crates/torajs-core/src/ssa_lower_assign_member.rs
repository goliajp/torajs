//! `Expr::Assign { target: Expr::Member { obj, name: field }, value }`
//! lowering pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Assign` match arm as chunk-80 of the decomp.
//!
//! Dispatch ladder (each path returns the assigned value as the
//! expression result, modulo the dynobj / Closure / Arr-prop paths
//! that return `ConstI64(0)` to match the legacy in-line emit shape):
//!
//! 1. **Type::Any** (`P3.2`) — dynobj substrate: unbox the receiver ptr,
//!    pack RHS as `(tag, value)`, call `dynobj_set`. Nested Type::Any
//!    payload routes through `any_payload_rc_inc`. Frozen / non-writable
//!    write throws via `emit_throw_check`. Post-resize ptr writeback
//!    via `emit_any_dynobj_writeback` honoured when the receiver was a
//!    plain Ident.
//! 2. **Type::Closure** (`T-27`) — `f.x = v` writes to the closure's
//!    lazy `props_dynobj` at `CLOSURE_PROPS_OFF` via `fn_props_set`.
//! 3. **Type::FnSig** (`T-27.b`) — top-level FnDecl routes through
//!    the `fnprops` side table keyed by fn pointer.
//! 4. **Type::Arr + field=="length"** (`S133-3`) — spec §9.4.2.4 length
//!    setter: route to `arr_set_length_validate` (refcount elem types)
//!    or `arr_set_length_truncate_scalar` (i64/f64/bool elements).
//! 5. **Type::Arr** (other field, `T-29`) — `arr.x = v` writes to the
//!    array's side-table `props_dynobj` (keyed by ptr) via
//!    `arrprops_set`. `arr_drop` / `arr_drop_any` drop_entry hook
//!    cleans the bucket at refcount == 0.
//! 6. **Type::RegExp + field=="lastIndex"** (`P9.4`) — coerce RHS to i64
//!    (ToInteger), call `regex_set_last_index`, return the coerced
//!    value as the expression result (mirrors a struct field store).
//! 7. **Type::Obj** (struct receiver) — two sub-paths:
//!    - **Accessor setter** (`P8.2`) — desugar_classes renamed the
//!      setter to `__cm_<C>__<name>_set` and registered
//!      `accessor_setters[(C, name)] → fn_name`. Emit Call with
//!      `[obj_val, value]`, widen i64→f64 when the setter param is
//!      f64 (F2-fix). Return the original value.
//!    - **Direct struct field store** — locate `(idx, field_ty)` in
//!      the struct layout, emit `obj_check_not_frozen` (`T-09.d`
//!      v0.4.0 frozen guard) with a real TypeError throw-check
//!      (`P7.4-frozen`), evaluate RHS with `V3-06` empty-array-from-
//!      field-Type override, width-align (`W4`) i64→f64 if the
//!      field is f64, drop the old non-Copy field value, then Store.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, obj: ExprId, field: String, value: ExprId) -> Operand {
    // Step 7d-A — capture the LHS variable name (if `obj` is a plain
    // Ident) so the Type::Any dynobj-set / dynobj_define paths below
    // can write the post-resize ptr back to the variable's storage
    // as a fresh NaN-box `AnyValue`.
    let obj_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        Some(n.clone())
    } else {
        None
    };
    let obj_val = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_val);

    if matches!(obj_ty, Type::Any) {
        return lower_dynobj_assign(ctx, obj_val, &field, value, &obj_ident);
    }
    if matches!(obj_ty, Type::Closure(_)) {
        return lower_closure_props_assign(ctx, obj_val, &field, value);
    }
    if matches!(obj_ty, Type::FnSig(_)) {
        return lower_fnsig_props_assign(ctx, obj_val, &field, value);
    }
    if matches!(obj_ty, Type::Arr(_)) && field == "length" {
        return lower_arr_length_assign(ctx, obj_val, obj_ty, value);
    }
    if matches!(obj_ty, Type::Arr(_)) {
        return lower_arr_props_assign(ctx, obj_val, &field, value);
    }
    if obj_ty == Type::RegExp && field == "lastIndex" {
        return lower_regex_last_index_assign(ctx, obj_val, value);
    }
    let sid = match obj_ty {
        Type::Obj(sid) => sid,
        other => panic!("ssa-lower: field assign on non-obj {other:?}"),
    };
    lower_obj_assign(ctx, obj_val, sid, &field, value)
}

fn lower_dynobj_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    value: ExprId,
    obj_ident: &Option<String>,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    ctx.consume_if_ident(value);
    let v_ty = ctx.operand_ty(&v_raw);
    let (tag, val_op): (i64, Operand) = match v_ty {
        Type::I64 | Type::I32 => (2, v_raw),
        Type::F64 => {
            let cur_block = ctx.cur_block;
            let bits =
                ctx.f
                    .append_inst(cur_block, InstKind::BitCastF64ToI64(v_raw), Type::I64, None);
            (3, Operand::Value(bits))
        }
        Type::Bool => {
            let cur_block = ctx.cur_block;
            let zext =
                ctx.f
                    .append_inst(cur_block, InstKind::ZExtBoolToI64(v_raw), Type::I64, None);
            (1, Operand::Value(zext))
        }
        // P4.0 — Type::Any must be unboxed BEFORE the is_refcounted
        // catch-all (see matching arm-order fix in
        // lower_dynobj_init). Step 7c: shim Call instead of inline
        // +8/+16 direct-offset Load (layout-decoupling).
        Type::Any => {
            return lower_dynobj_assign_any_payload(ctx, obj_val, field, v_raw);
        }
        _ if v_ty.is_refcounted() => {
            ctx.emit_rc_inc(v_raw);
            (4, v_raw)
        }
        Type::Ptr if matches!(v_raw, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
        _ => panic!("ssa-lower: dynobj assign unsupported value type {v_ty:?}"),
    };
    let dynobj = ctx.any_unbox_value_as_ptr(obj_val);
    let key_str = ctx.intern_string_literal(field);
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
    );
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_set,
            vec![
                Operand::Value(slot),
                Operand::Value(key_str),
                Operand::ConstI64(tag),
                val_op,
            ],
        ),
    );
    // P3.attribute-flag-tracking — implicit assign now throws on
    // writable=false.
    ctx.emit_throw_check(None);
    ctx.emit_any_dynobj_writeback(obj_ident, slot);
    Operand::ConstI64(0)
}

fn lower_dynobj_assign_any_payload(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    v_raw: Operand,
) -> Operand {
    let cur_block = ctx.cur_block;
    let tag_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![v_raw]),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    let val_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![v_raw]),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_payload_rc_inc,
            vec![Operand::Value(tag_v), Operand::Value(val_v)],
        ),
    );
    let dynobj = ctx.any_unbox_value_as_ptr(obj_val);
    let key_str = ctx.intern_string_literal(field);
    let slot = ctx.alloca(Type::Ptr, Some("__dynobj_slot"));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
    );
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.dynobj_set,
            vec![
                Operand::Value(slot),
                Operand::Value(key_str),
                Operand::Value(tag_v),
                Operand::Value(val_v),
            ],
        ),
    );
    // P3.attribute-flag-tracking — implicit assign now throws on
    // writable=false.
    ctx.emit_throw_check(None);
    v_raw
}

fn lower_closure_props_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    ctx.consume_if_ident(value);
    let (tag, val_op) = ctx.box_to_tag_value(v_raw);
    ctx.fn_props_set(obj_val, field, tag, val_op);
    Operand::ConstI64(0)
}

fn lower_fnsig_props_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    ctx.consume_if_ident(value);
    let (tag, val_op) = ctx.box_to_tag_value(v_raw);
    let key_str = ctx.intern_string_literal(field);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.fnprops_set,
            vec![obj_val, Operand::Value(key_str), tag, val_op],
        ),
    );
    Operand::ConstI64(0)
}

fn lower_arr_length_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    obj_ty: Type,
    value: ExprId,
) -> Operand {
    let (tag, val_op) = ctx.lower_to_tag_value(value);
    ctx.consume_if_ident(value);
    // ES §10.4.2.5 step 4 — for non-refcounted scalar element types
    // we route to the truncate-aware helper that also writes
    // `len = N` when `N < oldLen`. Refcounted element types (Str /
    // Substr / Arr / ...) still go through validate-only until the
    // per-slot rc_dec truncate path lands.
    let elem_ty = if let Type::Arr(elem_arr_id) = obj_ty {
        Some(ctx.arr_layouts[elem_arr_id.0 as usize])
    } else {
        None
    };
    let truncate_scalar = matches!(
        elem_ty,
        Some(Type::I64) | Some(Type::F64) | Some(Type::Bool)
    );
    let helper = if truncate_scalar {
        ctx.intrinsics.arr_set_length_truncate_scalar
    } else {
        ctx.intrinsics.arr_set_length_validate
    };
    let argv = if truncate_scalar {
        vec![obj_val, tag, val_op]
    } else {
        vec![tag, val_op]
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(cur_block, InstKind::Call(helper, argv));
    ctx.emit_throw_check(None);
    Operand::ConstI64(0)
}

fn lower_arr_props_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    value: ExprId,
) -> Operand {
    // lower_to_tag_value keeps `undefined` ANY_UNDEF (plain pair
    // would collapse to null)
    let (tag, val_op) = ctx.lower_to_tag_value(value);
    ctx.consume_if_ident(value);
    let key_str = ctx.intern_string_literal(field);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.arrprops_set,
            vec![obj_val, Operand::Value(key_str), tag, val_op],
        ),
    );
    Operand::ConstI64(0)
}

fn lower_regex_last_index_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    ctx.consume_if_ident(value);
    let v_i64 = ctx.coerce_to_i64(v_raw);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_set_last_index,
            vec![obj_val, v_i64.clone()],
        ),
    );
    v_i64
}

fn lower_obj_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Operand {
    if let Some(v) = try_lower_setter_call(ctx, obj_val, sid, field, value) {
        return v;
    }
    lower_struct_field_store(ctx, obj_val, sid, field, value)
}

fn try_lower_setter_call(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Option<Operand> {
    // P8.2 — accessor write: `c.value = v` where C declares
    // `set value(n: T)`. desugar_classes renamed the setter's FnDecl
    // to `__cm_<C>__<name>_set` and recorded
    // `(C, name) → fn_name` in `ast.accessor_setters`. Emit a Call
    // to the setter with `[obj_val, value]` and return the value
    // (parallel to a normal Store which also evaluates to the
    // value). Skips the struct field lookup + Store path below.
    let mut setter_cname: Option<String> = None;
    for (n, ty) in ctx.aliases.iter() {
        if matches!(ty, Type::Obj(s) if s.0 == sid.0) && ctx.ast.class_parents.contains_key(n) {
            setter_cname = Some(n.clone());
            break;
        }
    }
    let cname = setter_cname?;
    let setter_fn = ctx
        .ast
        .accessor_setters
        .get(&(cname.clone(), field.to_string()))
        .cloned()?;
    let fid = ctx.fn_table.get(&setter_fn).copied()?;
    let v = ctx.lower_expr(value);
    ctx.consume_if_ident(value);
    // F2-fix — the accessor arm bypasses the width-aware direct-call
    // coercion; an i64 value must widen to the setter's f64 param
    // (raw bits read as a denormal).
    let mut arg = v;
    if let Some(sig_id) = ctx.fn_sig_ids.get(&fid).copied()
        && ctx.fn_sigs[sig_id.0 as usize].0.get(1) == Some(&Type::F64)
        && ctx.operand_ty(&arg) == Type::I64
    {
        arg = ctx.coerce_to_f64(arg);
    }
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Call(fid, vec![obj_val, arg]));
    ctx.emit_throw_check(Some(fid));
    Some(v)
}

fn lower_struct_field_store(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Operand {
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let (idx, field_ty) = layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| {
            if fname == field {
                Some((i, *fty))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("ssa-lower: struct {sid:?} has no field `{field}`"));
    let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
    // T-09.d (v0.4.0) — frozen mutation guard. Inline call to runtime
    // helper that panics with a TypeError-shaped message if the
    // object's universal heap header has the FROZEN bit set. Matches
    // bun's strict-mode throw on `Object.freeze(o); o.field = ...`.
    // ~3-cycle overhead on the unfrozen path (single load + and + cmp
    // + branch-not-taken after LLVM inlines the call body).
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.obj_check_not_frozen, vec![obj_val]),
    );
    // P7.4-frozen — obj_check_not_frozen now arms a real TypeError
    // (instead of process abort) when the target is frozen. Force
    // the throw-check here (intrinsic → emit_throw_check(Some) would
    // skip it) so it diverts to the try/catch or propagates BEFORE
    // the field store below — the illegal mutation must not happen.
    // Mirrors the a-2 dynobj writable=false pattern.
    ctx.emit_throw_check(None);
    // V3-06 — `this.kids = []` in a constructor. Mirrors the K.6
    // LetDecl-global path: empty array literals lack inferable
    // element type on their own, so we allocate from the field's
    // declared `Type::Arr` here.
    let v = if let Expr::Array(els) = ctx.ast.get_expr(value)
        && els.is_empty()
        && matches!(field_ty, Type::Arr(_))
    {
        let cur_block = ctx.cur_block;
        let alloc = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(0)]),
            field_ty,
            None,
        );
        Operand::Value(alloc)
    } else {
        let v = ctx.lower_expr(value);
        ctx.consume_if_ident(value);
        // W4 — align the stored value with the field width (mirrors
        // the index-assign site; the reverse direction means the
        // width analysis missed this write).
        match (field_ty, ctx.operand_ty(&v)) {
            (Type::F64, Type::I64) => ctx.coerce_to_f64(v),
            (Type::I64, Type::F64) => panic!(
                "ssa-lower: f64 value into i64 struct field `{field}` — \
                 container width analysis missed this write"
            ),
            _ => v,
        }
    };
    // Drop the old field value if non-Copy.
    if !field_ty.is_copy() {
        let cur_block = ctx.cur_block;
        let old = ctx.f.append_inst(
            cur_block,
            InstKind::Load(field_ty, obj_val, offset),
            field_ty,
            None,
        );
        ctx.emit_drop_value(Operand::Value(old), field_ty);
    }
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(v, obj_val, offset));
    v
}
