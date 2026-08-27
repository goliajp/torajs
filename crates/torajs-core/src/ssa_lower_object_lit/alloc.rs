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

/// The named class whose own sid is `sid`, if any. Deterministic:
/// two structurally identical classes intern to one sid, so the
/// lowest tag wins rather than whichever the map happened to yield.
fn named_class_tag_of_sid(ctx: &LowerCtx<'_>, sid: StructId) -> Option<u32> {
    ctx.aliases
        .iter()
        .filter(|(_, ty)| matches!(ty, Type::Obj(s) if *s == sid))
        .filter_map(|(name, _)| ctx.class_name_to_tag.get(name).copied())
        .min()
}

pub(super) fn write_class_tag(ctx: &mut LowerCtx<'_>, obj_ptr: crate::ssa::ValueId, sid: StructId) {
    // The tag names which class this CELL is — not which factory is
    // being lowered. Those coincide for the factory's own instance
    // and NOT for a nested one: a factory that default-initializes
    // an `Obj`-typed field builds that field's instance in the same
    // body, and `ctx.f.name` stamped the ENCLOSING class on it. A
    // 3-field `__Gen_rf` then wore `__Gen_gen`'s six-field layout,
    // and the cycle collector — the one consumer that reads
    // `child_offsets` — walked 24 bytes past a 56-byte allocation
    // and dereferenced whatever was there (rotation 515; the async
    // generator × for-await crashes).
    //
    // Only withhold the factory tag where the sid PROVES the cell is
    // not the factory's class: a class with no alias sid (generic —
    // see the note below) keeps the old answer untouched.
    let factory_name = ctx.f.name.strip_prefix("__new_").map(str::to_owned);
    let factory_own_sid = factory_name
        .as_deref()
        .and_then(|cname| match ctx.aliases.get(cname) {
            Some(Type::Obj(s)) => Some(*s),
            _ => None,
        });
    let factory_tag = match (factory_name.as_deref(), factory_own_sid) {
        (Some(_), Some(own)) if own != sid => named_class_tag_of_sid(ctx, sid),
        (Some(cname), _) => ctx.class_name_to_tag.get(cname).copied(),
        (None, _) => None,
    };
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
    // A GENERIC class's mono factory (`__new_Box$$_number`) misses
    // the bare-name tag lookup, but its instances still need a
    // vtable — `populate_vtables` emits one row per mono factory
    // under the full suffixed spelling (same `split_once` shape as
    // `write_class_tag`'s generic-tag branch above).
    let vtable_class: Option<&str> = if ctx.ast.method_index.is_empty() {
        None
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
