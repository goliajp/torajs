//! ES §25.5.2 SerializeJSONProperty step 2 for the struct lane — a
//! value whose layout carries a callable `toJSON` serializes its
//! HOOK'S RESULT, not its own fields.
//!
//! The any lane got this in rotation 205; the struct lane used to
//! reach the field unfold and panic on the `Type::Closure` slot. The
//! machinery is the accessor arm's (`composite_obj` calls a Closure
//! slot and recurses on the result) — the difference is that this
//! hook replaces the whole value rather than one field, so it runs
//! before the unfold.

use crate::ssa::{Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

/// The `toJSON` slot of a struct layout — its field offset and
/// closure signature. Accessor slots (`__getter_toJSON`) are not
/// this: they are ordinary properties whose [[Get]] the field unfold
/// already routes, and a getter-provided toJSON is a shape the struct
/// lane does not model.
fn to_json_slot(ctx: &LowerCtx, sid: StructId) -> Option<(u64, crate::ssa::SigId)> {
    let layout = &ctx.struct_layouts[sid.0 as usize];
    layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| match (fname.as_str(), fty) {
            (Some("toJSON"), Type::Closure(sig)) => Some((OBJ_HEADER_SIZE + (i as u64) * 8, *sig)),
            _ => None,
        })
}

/// Emit `ToString(? Call(value.toJSON, value, «key»))` when the
/// layout has one, else `None` so the caller unfolds the fields as
/// before.
///
/// The hook's declared return type drives the recursion, so a
/// `toJSON` answering a string quotes, one answering an object
/// unfolds, and one answering nothing (`Void`) makes the whole value
/// `undefined` — §25.5.2.4 step 8.b then omits the key at the
/// caller, which is exactly what the `undefined` Str sentinel
/// expresses on this lane.
pub(super) fn try_lower_hook(
    ctx: &mut LowerCtx,
    val_op: &Operand,
    sid: StructId,
    key: Option<Operand>,
    gap: Option<Operand>,
    depth: u32,
) -> Option<Operand> {
    let (field_off, sig_id) = to_json_slot(ctx, sid)?;
    let obj_ptr = match val_op {
        Operand::Value(v) => *v,
        _ => return None,
    };
    // §25.5.2 step 2.b hands the hook the property key the value sits
    // under — "" at the top level. A zero-parameter hook (the common
    // `toJSON() { ... }`) takes no argument slot at all.
    let takes_key = !ctx.fn_sigs[sig_id.0 as usize].0.is_empty();
    let args: Vec<Operand> = if takes_key {
        vec![key.unwrap_or_else(|| Operand::Value(ctx.intern_string_literal("")))]
    } else {
        vec![]
    };
    // A hook that mentions `this` was promoted to the
    // `(__env, __this, ...user)` ABI; one that does not stays a plain
    // `(__env, ...user)` closure. Seeding the receiver slot
    // unconditionally would shift the key argument into it, and the
    // callee would read the env pointer as its key.
    let takes_recv =
        crate::ssa_lower_call_struct_method_dispatch::is_objlit_method_slot(ctx, sid, "toJSON");
    let got = crate::ssa_lower_call_struct_method_dispatch::emit_closure_call_ops(
        ctx,
        Operand::Value(obj_ptr),
        field_off,
        sig_id,
        args,
        takes_recv,
    );
    let ret_ty = ctx.operand_ty(&got);
    if matches!(ret_ty, Type::Void) {
        return Some(Operand::Value(ctx.intern_string_literal("null")));
    }
    let out = super::lower_shape(ctx, got.clone(), ret_ty, None, gap, depth);
    ctx.emit_drop_value(got, ret_ty);
    Some(out)
}
