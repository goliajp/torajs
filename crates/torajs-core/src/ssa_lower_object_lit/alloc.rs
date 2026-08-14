//! The cell-construction tail of [`super`]'s object-literal
//! lowering — allocation dispatch (11-A2-a stack hint), universal
//! header init (+ the Error-derived factory flag), the class-tag
//! store and the vtable pointer. Split verbatim at the 500-line
//! file cap; the phase-5 ordering rationale lives in the parent's
//! module doc.

use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE, OBJ_VTABLE_OFF};

pub(super) fn alloc_obj(
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

pub(super) fn init_header(ctx: &mut LowerCtx<'_>, obj_ptr: crate::ssa::ValueId, _sid: StructId) {
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

pub(super) fn write_class_tag(ctx: &mut LowerCtx<'_>, obj_ptr: crate::ssa::ValueId, sid: StructId) {
    let factory_tag = ctx
        .f
        .name
        .strip_prefix("__new_")
        .and_then(|cname| ctx.class_name_to_tag.get(cname).copied());
    let tag = factory_tag.unwrap_or_else(|| ctx.anon_stamp_pool.borrow_mut().assign_or_get(sid));
    // 404-01 — a GENERIC class's mono factory (`__new_G$$_number` /
    // `__new_G$$anywv`) misses the name lookup above, so its
    // instances wear a per-specialization anon tag (correct: field
    // layouts differ per specialization, and the cycle collector /
    // reflection read by tag). Record the sid → base-class link here,
    // at the one site that knows the ACTUAL alloc sid — the row
    // emitters dress that row with the class's name + method table.
    if factory_tag.is_none()
        && let Some(rest) = ctx.f.name.strip_prefix("__new_")
        && let Some((base, _mono)) = rest.split_once("$$")
        && ctx.class_name_to_tag.contains_key(base)
    {
        ctx.anon_stamp_pool
            .borrow_mut()
            .record_generic_class_sid(sid, base.to_string());
    }
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

pub(super) fn write_vtable_ptr(ctx: &mut LowerCtx<'_>, obj_ptr: crate::ssa::ValueId) {
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
