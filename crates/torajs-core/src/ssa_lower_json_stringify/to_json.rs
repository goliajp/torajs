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

/// True when the layout's `toJSON` field types `Any` — a hook this
/// lane cannot call, because the call it emits needs a static
/// signature and the slot has none.
///
/// It is not "no hook". §25.5.2 step 2 asks IsCallable of whatever
/// the property holds, and the value in an Any slot is routinely a
/// function: `{ toJSON: new Proxy(fn, {}) }` serialized to
/// `{"toJSON":{}}` — the hook itself, as data, from a program that
/// compiled and ran. The `any` spelling of the same object had been
/// answering correctly since rotation 683; the struct lane is the
/// other layer of that fix.
/// Does the value's CLASS carry a `toJSON` method? The hook then
/// lives in the class's method table, and the layout — which is all
/// [`to_json_slot`] can see — does not mention it. So
/// `class C { a = 1; toJSON() { return 9 } }` serialized as
/// `{"a":1}`: not a refusal, a wrong document, in plain JavaScript
/// with no proxy anywhere near it.
///
/// Read off the FRONTEND type, because the layout id cannot answer
/// it: `class C { a }` and `class D { a }` intern to one StructId
/// (the note on `ssa_lower_member_obj_field::class_name_of_expr`).
/// An inherited hook counts, so the parent chain is walked.
fn class_has_to_json(ctx: &LowerCtx, fe: Option<&crate::check::Type>) -> bool {
    match fe {
        // A named class answers precisely, chain included.
        Some(crate::check::Type::ClassRef(name)) => {
            let mut cur = Some(name.clone());
            while let Some(c) = cur {
                if ctx.fn_table.contains_key(&format!("__cm_{c}__toJSON")) {
                    return true;
                }
                cur = ctx.ast.class_parents.get(&c).cloned().flatten();
            }
            false
        }
        // A structural type IS its layout, so `to_json_slot` above has
        // already seen everything there is to see.
        Some(crate::check::Type::Struct(_)) => false,
        // Otherwise the frontend type is gone — erased by an `as any`,
        // or lost through a hook's own result — and the layout id
        // cannot stand in for it, because same-shape classes intern to
        // one StructId. Unfolding here is a claim that no hook exists,
        // and that claim is exactly what cannot be made.
        //
        // So the question narrows to one the module CAN answer: does
        // any class in this program declare `toJSON` at all? Almost
        // never — and when the answer is no, every value takes the
        // unfold it took before, unchanged.
        _ => ctx
            .fn_table
            .keys()
            .any(|k| k.starts_with("__cm_") && k.ends_with("__toJSON")),
    }
}

fn to_json_slot_is_any(ctx: &LowerCtx, sid: StructId) -> bool {
    ctx.struct_layouts[sid.0 as usize]
        .iter()
        .any(|(fname, fty)| matches!((fname.as_str(), fty), (Some("toJSON"), Type::Any)))
}

/// Does this value's hook go to the RUNTIME walk rather than to
/// either compiled road — the layout's own call, or the field
/// unfold?
///
/// Callers need this before the walk, not only inside it: the
/// runtime walk reports "serializes to nothing" as the undefined-Str
/// sentinel, and the composite walks translate that into §25.5.2.4
/// step 8.b's omission. They gate that translation on the static
/// shape, so a value that changes road has to change their gate too
/// — otherwise a hook answering `undefined` writes the word into the
/// document.
pub(super) fn takes_runtime_walk(
    ctx: &LowerCtx,
    ty: &Type,
    fe: Option<&crate::check::Type>,
) -> bool {
    matches!(ty, Type::Obj(sid) if to_json_slot_is_any(ctx, *sid) || class_has_to_json(ctx, fe))
}

/// Emit `ToString(? Call(value.toJSON, value, «key»))` when the
/// layout has one, else `None` so the caller unfolds the fields as
/// before.
///
/// An Any-typed slot takes neither road: the value boxes and the
/// runtime walk serializes it, which is where §25.5.2 step 2's
/// IsCallable already lives. Same posture as the BigInt / Symbol arm
/// of `lower_shape` — hand the case to the lane that implements it
/// rather than grow a second copy here.
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
    fe: Option<&crate::check::Type>,
    gap: Option<Operand>,
    depth: u32,
) -> Option<Operand> {
    if takes_runtime_walk(ctx, &Type::Obj(sid), fe) {
        let boxed = ctx.box_to_any(*val_op);
        let out = match key {
            Some(key) => super::emit_keyed_walk(ctx, boxed, key, gap, depth),
            None => super::emit_any_walk(ctx, boxed, gap, depth),
        };
        ctx.emit_throw_check(None);
        return Some(out);
    }
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
    // §25.5.2.3 — step 2 just ran, so what follows is the VALUE half
    // of the operation and must not run it again. An `Any` answer
    // used to take `lower_shape`'s any arm, which is the whole
    // property operation including step 2: a hook whose result has a
    // `toJSON` of its own answered the INNER hook's value, and
    // `{p:{toJSON:()=>({toJSON:()=>17})}}` came out `{"p":17}` where
    // the spec and every engine say `{"p":{}}`. The runtime has the
    // two halves as separate entries now; every other answer type
    // keeps the static unfold, which never ran step 2 to begin with.
    let out = if matches!(ret_ty, Type::Any) {
        super::emit_any_walk_value(ctx, got, gap, depth)
    } else {
        super::lower_shape(ctx, got, ret_ty, None, gap, depth)
    };
    ctx.emit_drop_value(got, ret_ty);
    Some(out)
}
