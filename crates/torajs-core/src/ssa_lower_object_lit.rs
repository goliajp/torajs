//! `Expr::ObjectLit { fields }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-75
//! of the decomp (chunks 1-74 = ... + `Expr::PostIncr`).
//!
//! Lowers each field; spread members (sentinel name `__spread__`)
//! are unfolded by reading each source-struct field offset and
//! copying it into the destination. Inline members win on key
//! collision (later occurrences replace earlier slots). Spread
//! sources are lowered once; their values are read field-by-field.
//!
//! Five phases:
//!
//! 1. **Field lowering** — walk `fields` entries; `__spread__`
//!    sentinel unfolds a typed-struct source's layout; regular
//!    members lower the value with refcounted-borrow rc_inc
//!    discipline (same shape as the array-literal fix; without it,
//!    two struct lits sharing a refcounted Obj field
//!    `{x:a}; {x:a}` would double-walk-drop the shared element).
//!    (chunk 572: non-inc shapes need no move marker — all no-ops).
//! 2. **W4 field-width widen** — the literal's field widths come
//!    from its alias class (the Anon origin key unions with the
//!    receiving slot during analysis): `{x:1}` with a later
//!    `o.x = 0.5` carries an F64 `x` slot up front. `num_f64_slots`
//!    side-channel via `SlotKey::Anon(eid.0)` lookup; per-field
//!    `coerce_to_f64` when widening kicks in.
//! 3. **Layout resolution** — delegates to
//!    `ssa_lower_objlit_layout::resolve_objlit_layout` for exact
//!    match / numeric-width coercion / anonymous auto-register.
//! 4. **Allocation dispatch** — 11-A2-a: if `let_stack_alloc_hint`
//!    is set AND no field is refcounted, swap heap `obj_alloc` for
//!    stack `AllocaBytes`. Refcounted fields force back to heap
//!    because end-of-scope drop emission skips stack locals; a
//!    stack-alloc obj with refcounted children would leak the
//!    children's rc.
//! 5. **Header + field writes** — Phase 2B universal heap header
//!    init (refcount=1, type_tag=OBJ, flags=0); Error-derived
//!    factory class flag store (FLAG_ERROR in u16 @+6 so the
//!    uncaught-throw reporter renders `name: message` from
//!    Error layout prefix; MUST precede the class_tag store
//!    because the +4 64-bit Store spills its high half into +8,
//!    overwritten by the class_tag store at +8); Phase H.1.b
//!    class_tag at OBJ_CLASS_TAG_OFF (factory tag from
//!    `__new_<C>` name strip + W-J Phase A1 anon stamp pool
//!    fallback for inline `{x:1,y:2}` literals + generic mono
//!    shapes); T-24 vtable pointer at OBJ_VTABLE_OFF (when chain
//!    methods exist AND we're inside a `__new_<C>` factory of a
//!    known class). Then per-field Store at
//!    `OBJ_HEADER_SIZE + i*8`.
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::ObjectLit` match arm bottoms out here).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE, OBJ_VTABLE_OFF};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, fields: Vec<(String, ExprId)>, eid: ExprId) -> Operand {
    // Chunk 760 — claim the let-decl stack hint BEFORE the field
    // lowering: a nested object literal in a field value otherwise
    // consumed it inside `lower_field_entries` — the INNER block got
    // stack-alloc'd (its pointer escaped into the outer heap
    // object's slot, dangling on frame reuse) AND the let name
    // landed in `stack_alloced_locals`, so the OUTER heap alloc was
    // never scope-dropped (churn c3: the whole per-iteration field
    // graph leaked, ~120B/iter).
    let stack_hint = ctx.let_stack_alloc_hint.take();
    // Chunk 780 — claim the declared-layout hint HERE for the same
    // reason as the stack hint above: a nested object literal in a
    // field value would otherwise consume it inside
    // `lower_field_entries` and pin the OUTER declared layout onto
    // the inner literal.
    let declared_hint = ctx.let_declared_obj_layout.take();
    // RFC 20260710-optional-undefined-repr C1 — fields initialized
    // with an undefined LITERAL (frontend-distinguished from null)
    // must land the per-type undefined sentinel in the slot, not the
    // shared NULL (which means JS null): remember which field names
    // carried one before lowering collapses both to ConstPtrNull.
    let undef_fields: std::collections::HashSet<String> = fields
        .iter()
        .filter(|(_, veid)| {
            matches!(
                ctx.expr_types.get(veid),
                Some(crate::check::Type::Undefined)
            )
        })
        .map(|(n, _)| n.clone())
        .collect();
    let (mut field_tys, mut field_vals) = lower_field_entries(ctx, &fields);
    apply_w4_widen(ctx, &mut field_tys, &mut field_vals, eid);
    let sid = crate::ssa_lower_objlit_layout::resolve_objlit_layout(
        &mut ctx.struct_layouts,
        &mut ctx.f,
        ctx.cur_block,
        &mut field_tys,
        &mut field_vals,
        declared_hint,
    );
    let any_refcounted = field_tys.iter().any(|(_, ty)| ty.is_refcounted());
    let obj_ptr = alloc_obj(ctx, sid, field_tys.len(), any_refcounted, stack_hint);
    init_header(ctx, obj_ptr, sid);
    write_class_tag(ctx, obj_ptr, sid);
    write_vtable_ptr(ctx, obj_ptr);
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    for (i, (fname, slot_ty)) in layout.iter().enumerate() {
        if undef_fields.contains(fname)
            && let Some(sentinel) = ctx.str_undef_sentinel_for(*slot_ty)
        {
            field_vals[i] = sentinel;
        }
        // RFC 20260710 C4 — a declared-Any slot (`__nullable(number|
        // boolean)` optional field, plain `any` field) takes a
        // NaN-box, never a raw scalar: resolve_objlit_layout only
        // retyped the slot, the value boxes here. The expr-aware
        // variant keeps null vs undefined distinct (ANY_NULL vs
        // ANY_UNDEF from the same lowered ConstPtrNull). Sources
        // outside the scalar/nullish set (heap values behind an
        // `any` field) keep their pre-RFC raw store — their
        // ownership story is a separate face.
        if *slot_ty == Type::Any {
            let src_ty = ctx.operand_ty(&field_vals[i]);
            let boxable = matches!(src_ty, Type::I64 | Type::I32 | Type::F64 | Type::Bool)
                || (src_ty == Type::Ptr && matches!(field_vals[i], Operand::ConstPtrNull));
            if boxable {
                let veid = fields
                    .iter()
                    .rev()
                    .find(|(n, _)| n == fname)
                    .map(|(_, veid)| *veid);
                field_vals[i] = match veid {
                    Some(e) => ctx.box_to_any_from_expr(e, field_vals[i].clone()),
                    // spread-unfolded field — no source expr; the
                    // plain box maps ConstPtrNull to ANY_NULL.
                    None => ctx.box_to_any(field_vals[i].clone()),
                };
            }
        }
    }
    for (i, val) in field_vals.iter().enumerate() {
        // L3b #6 crash fix — a typed Array stored into a struct field
        // reaches runtime walkers (inspect's Tag::Obj field printer,
        // the cycle collector's child walk) that read the header's
        // elem-kind to pick a slot interpretation. Record it here,
        // same as the `any`-boxing boundary (RFC 20260704 S1) —
        // without the mark a raw-i64 array walks as NaN-box cells
        // and SIGSEGVs on the first small-int deref. No-op for
        // non-Arr fields; chunk 621 derives the chain from the
        // value's own type inside the helper (an `any[]` field
        // taking a typed array marked chain 0 off the field type).
        ctx.emit_arr_mark_kind(val);
        let offset = OBJ_HEADER_SIZE + i as u64 * 8;
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(*val, Operand::Value(obj_ptr), offset),
        );
    }
    Operand::Value(obj_ptr)
}

fn lower_field_entries(
    ctx: &mut LowerCtx<'_>,
    entries: &[(String, ExprId)],
) -> (Vec<(String, Type)>, Vec<Operand>) {
    let mut field_tys: Vec<(String, Type)> = Vec::new();
    let mut field_vals: Vec<Operand> = Vec::new();
    for (n, eid) in entries {
        if let Some(omit) = crate::check_type_of_object_lit::spread_omit_set(n) {
            let omit: Vec<String> = omit.iter().map(|s| s.to_string()).collect();
            unfold_spread(ctx, *eid, &omit, &mut field_tys, &mut field_vals);
            continue;
        }
        lower_regular_field(ctx, n, *eid, &mut field_tys, &mut field_vals);
    }
    (field_tys, field_vals)
}

/// `omit` — the destructuring-rest desugar's excluded keys
/// (chunk 707, decoded from the `__spread_omit__:` sentinel);
/// omitted fields are neither loaded nor rc-inc'd.
fn unfold_spread(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    omit: &[String],
    field_tys: &mut Vec<(String, Type)>,
    field_vals: &mut Vec<Operand>,
) {
    let src_op = ctx.lower_expr(eid);
    let src_ty = ctx.operand_ty(&src_op);
    let Type::Obj(sid) = src_ty else {
        panic!("ssa-lower: object spread source must be a struct, got {src_ty:?}");
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    for (idx, (sn, st)) in layout.iter().enumerate() {
        if omit.contains(sn) {
            continue;
        }
        let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
        let cur_block = ctx.cur_block;
        let v = ctx
            .f
            .append_inst(cur_block, InstKind::Load(*st, src_op, off), *st, None);
        if st.is_refcounted() {
            ctx.emit_rc_inc(Operand::Value(v));
        }
        let v_op = Operand::Value(v);
        if let Some(pos) = field_tys.iter().position(|(k, _)| k == sn) {
            field_tys[pos] = (sn.clone(), *st);
            field_vals[pos] = v_op;
        } else {
            field_tys.push((sn.clone(), *st));
            field_vals.push(v_op);
        }
    }
}

fn lower_regular_field(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    eid: ExprId,
    field_tys: &mut Vec<(String, Type)>,
    field_vals: &mut Vec<Operand>,
) {
    let v = ctx.lower_expr(eid);
    let ty = ctx.operand_ty(&v);
    let needs_inc = ty.is_refcounted()
        && match ctx.ast.get_expr(eid) {
            Expr::Ident(name) => ctx
                .locals
                .get(name)
                .map(|info| !info.moved)
                .unwrap_or(false),
            Expr::Member { .. } | Expr::Index { .. } => true,
            _ => false,
        };
    // No consume on the else side: every shape reaching it is a no-op
    // for the move-walk (Copy binding / non-local Ident / already-moved
    // Ident / non-Ident expr) — chunk 572 removed the dead marker.
    if needs_inc {
        ctx.emit_rc_inc(v);
    }
    if let Some(pos) = field_tys.iter().position(|(k, _)| k == name) {
        field_tys[pos] = (name.to_string(), ty);
        field_vals[pos] = v;
    } else {
        field_tys.push((name.to_string(), ty));
        field_vals.push(v);
    }
}

fn apply_w4_widen(
    ctx: &mut LowerCtx<'_>,
    field_tys: &mut [(String, Type)],
    field_vals: &mut [Operand],
    eid: ExprId,
) {
    let key = crate::num_width::SlotKey::Anon(eid.0);
    for (i, (fname, fty)) in field_tys.iter_mut().enumerate() {
        if *fty == Type::I64 && ctx.num_f64_slots.field_is_f64(&key, fname) {
            *fty = Type::F64;
            let coerced = ctx.coerce_to_f64(field_vals[i].clone());
            field_vals[i] = coerced;
        }
    }
}

fn alloc_obj(
    ctx: &mut LowerCtx<'_>,
    sid: StructId,
    n_fields: usize,
    any_refcounted: bool,
    stack_hint: Option<String>,
) -> crate::ssa::ValueId {
    let size = n_fields as i64 * 8 + OBJ_HEADER_SIZE as i64;
    // Chunk 760 — the hint arrives from lower()'s entry (claimed
    // before field lowering so nested literals can't steal it).
    let stack_alloc_name = stack_hint.filter(|_| !any_refcounted);
    let cur_block = ctx.cur_block;
    if let Some(let_name) = stack_alloc_name {
        let p = ctx.f.append_inst(
            cur_block,
            InstKind::AllocaBytes(size as u64),
            Type::Obj(sid),
            None,
        );
        ctx.stack_alloced_locals.insert(let_name);
        return p;
    }
    let alloc_fid = ctx.intrinsics.obj_alloc;
    ctx.f.append_inst(
        cur_block,
        InstKind::Call(alloc_fid, vec![Operand::ConstI64(size)]),
        Type::Obj(sid),
        None,
    )
}

fn init_header(ctx: &mut LowerCtx<'_>, obj_ptr: crate::ssa::ValueId, _sid: StructId) {
    ctx.emit_obj_header_init(Operand::Value(obj_ptr));
    let factory_class = ctx.f.name.strip_prefix("__new_").map(str::to_owned);
    if let Some(cname) = factory_class
        && ctx.class_is_error_derived(&cname)
    {
        let packed = 1_i32 | ((torajs_rc::FLAG_ERROR as i32) << 16);
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(Operand::ConstI32(packed), Operand::Value(obj_ptr), 4),
        );
    }
}

fn write_class_tag(ctx: &mut LowerCtx<'_>, obj_ptr: crate::ssa::ValueId, sid: StructId) {
    let factory_tag = ctx
        .f
        .name
        .strip_prefix("__new_")
        .and_then(|cname| ctx.class_name_to_tag.get(cname).copied());
    let tag = factory_tag.unwrap_or_else(|| ctx.anon_stamp_pool.borrow_mut().assign_or_get(sid));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::ConstI64(tag as i64),
            Operand::Value(obj_ptr),
            OBJ_CLASS_TAG_OFF,
        ),
    );
}

fn write_vtable_ptr(ctx: &mut LowerCtx<'_>, obj_ptr: crate::ssa::ValueId) {
    let vtable_class: Option<&str> = if ctx.ast.method_index.is_empty() {
        None
    } else {
        ctx.f
            .name
            .strip_prefix("__new_")
            .filter(|c| ctx.class_name_to_tag.contains_key(*c))
    };
    let cur_block = ctx.cur_block;
    let vtable_ptr_op = match vtable_class {
        Some(cname) => {
            let g = ctx.f.append_inst(
                cur_block,
                InstKind::GlobalRef(format!("__vtable_{cname}")),
                Type::Ptr,
                None,
            );
            Operand::Value(g)
        }
        None => Operand::ConstPtrNull,
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(vtable_ptr_op, Operand::Value(obj_ptr), OBJ_VTABLE_OFF),
    );
}
