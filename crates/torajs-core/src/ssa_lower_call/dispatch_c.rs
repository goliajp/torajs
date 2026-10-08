//! The third stretch of the call-lowering cascade, split out of `ssa_lower_call`.

use super::*;

pub(super) fn try_dispatch_c(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    // ES §28.1.6 — `Reflect.get(target, key)` compile-time fold.
    if let Some(op) = crate::ssa_lower_call_reflect_get::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // v0.2 #3 — `Object.hasOwn(obj, key)` / `Reflect.has(obj, key)` compile-time fold.
    if let Some(op) = crate::ssa_lower_call_has_own::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // V3-18 m1.h.4 — Object.keys / Object.getOwnPropertyNames / Reflect.ownKeys namespace statics.
    if let Some(op) = crate::ssa_lower_call_object_keys::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // T-13.b — `Symbol.for(key)` / `Symbol.keyFor(s)` registry helpers.
    if let Some(op) = crate::ssa_lower_call_symbol_registry::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // T-15.g.3 / T-19.k/l/n / ②.6b — built-in Promise `.then/.catch/.finally` chain lowering.
    if let Some(v) = ctx.try_lower_promise_chain_call(callee, args) {
        return Some(v);
    }
    // Bun runtime cluster (Bun.file / Bun.gc / fetch / <response>.text() / weakref_create).
    if let Some(op) = crate::ssa_lower_call_bun_runtime::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // T-18.a — `fs_promises.<method>Async` wrappers (readFile / writeFile / ...).
    if let Some(op) = crate::ssa_lower_call_fs_promises::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // Object.* integrity/meta trap cluster (freeze / isFrozen / create / preventExtensions / ...).
    if let Some(op) = crate::ssa_lower_call_object_integrity::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // T-09.b — `Object.entries(obj)` compile-time unfold via struct_layouts.
    if let Some(op) = crate::ssa_lower_call_object_entries::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // T-09 chunk 693 — `Object.fromEntries(entries)` runtime dynobj build
    // (Arr / Any receivers; the annotated-let fast-path ran earlier).
    if let Some(op) = crate::ssa_lower_call_object_fromentries::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // v0.2 #3 — `Object.is(a, b)` SameValue dispatch (F64 → object_is_f64; Str → str_eq; ...).
    if let Some(op) = crate::ssa_lower_call_object_is::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `console.<m>(...)` single-arg + multi-arg dispatch (Substr substr_to_owned + borrow detect).
    if let Some(op) = crate::ssa_lower_call_console::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `xs.push(a, b)` / `xs.unshift(a, b)` / `xs.unshift()`.
    if let Some(r) = crate::ssa_lower_arr_variadic::try_lower(ctx, callee, args) {
        return Some(r);
    }
    if let Some(r) = ctx.try_lower_arr_pop_shift_unshift(callee, args) {
        return Some(r);
    }
    // `xs.splice(start, deleteCount)` — in-place remove + return removed slice (ES §23.1.3.31).
    if let Some(v) = crate::ssa_lower_splice::try_lower(ctx, callee, args) {
        return Some(v);
    }
    if let Some(v) = crate::ssa_lower_tospliced::try_lower(ctx, callee, args) {
        return Some(v);
    }
    // `<Arr>.push(v)` in-place — 3 receiver shapes (Ident local / K.8 global / `obj.field`).
    if let Some(op) = crate::ssa_lower_call_arr_push::try_lower(ctx, callee, args) {
        return Some(op);
    }
    None
}
