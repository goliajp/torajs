//! `Arr<Obj(sid)>` release where `sid` has no reference fields: one
//! call into the torajs-arr kernel instead of the inline last-owner
//! walk plus the trailing `arr_drop`.
//!
//! The kernel makes each element's release exactly as the inline
//! `emit_drop_obj` sequence does (decrement; a dying cell's WeakRef
//! notice, expando bag, cycle-buffer flag and sized free; a shared
//! class instance's bag buffered), with the per-slot tests in one
//! compiled loop. The sid rides along as the last operand, as on the
//! inline `obj_drop_sized`: `cell_size::settle` keeps the sized kernel
//! or switches to the `*_BY_CELL` twin once every layout is known.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) const LEAF_OBJS: &str = "__torajs_arr_drop_leaf_objs";
pub(crate) const LEAF_OBJS_BY_CELL: &str = "__torajs_arr_drop_leaf_objs_by_cell";
pub(crate) const LEAF_INSTANCES: &str = "__torajs_arr_drop_leaf_instances";
pub(crate) const LEAF_INSTANCES_BY_CELL: &str = "__torajs_arr_drop_leaf_instances_by_cell";

/// Emit the whole release of the array `val` (nullish already ruled
/// out) when its elements are cells of a fields-all-scalar sid;
/// false leaves `cur_block` untouched.
pub(super) fn try_emit(ctx: &mut LowerCtx, val: Operand, elem_ty: Type) -> bool {
    let Type::Obj(sid) = elem_ty else {
        return false;
    };
    // the elements that release through a call of their own: an
    // interface value by its cell's tag, a class with a `__drop_<C>`
    if crate::ssa_lower_iface::sid_is_iface(ctx, sid)
        || super::class_drop_setup::class_drop_fid(ctx, sid).is_some()
    {
        return false;
    }
    let layout = &ctx.struct_layouts[sid.0 as usize];
    if !layout.iter().all(|(_, fty)| fty.is_copy()) {
        return false;
    }
    let size = OBJ_HEADER_SIZE + layout.len() as u64 * 8;
    let name = if super::obj::is_class_sid(ctx, sid) {
        LEAF_INSTANCES
    } else {
        LEAF_OBJS
    };
    let fid = *ctx
        .fn_table
        .get(name)
        .unwrap_or_else(|| panic!("{name} intrinsic missing"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            fid,
            vec![
                val,
                Operand::ConstI64(size as i64),
                Operand::ConstI64(sid.0 as i64),
            ],
        ),
    );
    true
}
