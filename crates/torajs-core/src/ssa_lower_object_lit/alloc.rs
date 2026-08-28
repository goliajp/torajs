//! The cell-construction tail of [`super`]'s object-literal
//! lowering — allocation dispatch (11-A2-a stack hint), universal
//! header init (+ the Error-derived factory flag), the class-tag
//! store and the vtable pointer. Split verbatim at the 500-line
//! file cap; the phase-5 ordering rationale lives in the parent's
//! module doc.
//!
//! All three identity writes need the same fact — which class this
//! cell is — and all three used to read it off the enclosing
//! function's name. That answers "which factory am I inside", which
//! is the same thing only for the factory's OWN instance. The
//! `owner` argument carries the answer when it is not: a nested
//! field default belongs to the field's declared class (see
//! [`crate::ast::Ast::field_default_objlits`]).

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

pub(super) fn init_header(
    ctx: &mut LowerCtx<'_>,
    obj_ptr: crate::ssa::ValueId,
    _sid: StructId,
    owner: Option<&str>,
) {
    ctx.emit_obj_header_init(Operand::Value(obj_ptr));
    let cell_class = match owner {
        Some(cname) => Some(cname.to_owned()),
        None => ctx.f.name.strip_prefix("__new_").map(str::to_owned),
    };
    if let Some(cname) = cell_class
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

pub(super) fn write_class_tag(
    ctx: &mut LowerCtx<'_>,
    obj_ptr: crate::ssa::ValueId,
    sid: StructId,
    owner: Option<&str>,
) {
    // A nested field default names its own class, and neither the
    // factory tag nor the generic-factory pool applies to it — both
    // key off the enclosing factory. An owner that is an ALIAS
    // rather than a class has no tag of its own and falls through to
    // the sid-keyed anon stamp, which is what the same literal gets
    // outside a factory.
    if let Some(cname) = owner {
        let tag = ctx
            .class_name_to_tag
            .get(cname)
            .copied()
            .unwrap_or_else(|| ctx.anon_stamp_pool.borrow_mut().assign_or_get(sid));
        store_class_tag(ctx, obj_ptr, tag);
        return;
    }
    let factory_tag = ctx
        .f
        .name
        .strip_prefix("__new_")
        .and_then(|cname| ctx.class_name_to_tag.get(cname).copied());
    // 404-01 / 405-03 — a GENERIC class's mono factory
    // (`__new_G$$_number` / `__new_G$$anywv`) misses the name lookup
    // above. Its instances need a tag of their own — per FACTORY,
    // not per sid: the alloc sid is the ObjectLit's STRUCTURAL
    // intern, and two generic classes with the same field shape
    // share it (`G<number>` / `H<number>`), so a sid-keyed tag
    // cannot carry class identity. The pool mints one tag per
    // factory and records (tag, sid, base); the row emitters give
    // that tag the specialization's layout plus the class's name and
    // method table. Everything else keeps the sid-keyed anon stamp.
    let generic_tag = || {
        let rest = ctx.f.name.strip_prefix("__new_")?;
        let (base, _mono) = rest.split_once("$$")?;
        if !ctx.class_name_to_tag.contains_key(base) {
            return None;
        }
        Some(
            ctx.anon_stamp_pool
                .borrow_mut()
                .assign_generic(&ctx.f.name, sid, base.to_string()),
        )
    };
    let tag = factory_tag
        .or_else(generic_tag)
        .unwrap_or_else(|| ctx.anon_stamp_pool.borrow_mut().assign_or_get(sid));
    store_class_tag(ctx, obj_ptr, tag);
}

fn store_class_tag(ctx: &mut LowerCtx<'_>, obj_ptr: crate::ssa::ValueId, tag: u32) {
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

pub(super) fn write_vtable_ptr(
    ctx: &mut LowerCtx<'_>,
    obj_ptr: crate::ssa::ValueId,
    owner: Option<&str>,
) {
    // A GENERIC class's mono factory (`__new_Box$$_number`) misses
    // the bare-name tag lookup, but its instances still need a
    // vtable — `populate_vtables` emits one row per mono factory
    // under the full suffixed spelling (same `split_once` shape as
    // `write_class_tag`'s generic-tag branch above).
    let vtable_class: Option<&str> = if ctx.ast.method_index.is_empty() {
        None
    } else if let Some(cname) = owner {
        // Same reasoning as the tag: the cell is the field's class,
        // so it wants that class's method table. An alias owner has
        // none and takes the null a non-factory literal takes.
        Some(cname).filter(|c| ctx.class_name_to_tag.contains_key(*c))
    } else {
        ctx.f.name.strip_prefix("__new_").filter(|c| {
            ctx.class_name_to_tag.contains_key(*c)
                || c.split_once("$$")
                    .is_some_and(|(base, _)| ctx.class_name_to_tag.contains_key(base))
        })
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
