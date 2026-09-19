//! The fourth stage of the call dispatcher cascade (`ssa_lower_call`):
//! the typed-receiver method families (Promise statics, sibling-class
//! dispatch, WeakRef / Set / Map / Date / RegExp / string / array
//! methods) and the callable-value calls. Arm order is load-bearing —
//! an earlier arm shadows a later, more general one — so the stage
//! moved here whole.

use crate::ast::ExprId;
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_dispatch_d(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    // T-17.a/b/c/d + P10.2-A1 + T-15.g.1/5 — Promise.all/race/any/allSettled + resolve/reject (needs eid).
    if let Some(op) = crate::ssa_lower_call_promise_static::try_lower(ctx, eid, callee, args) {
        return Some(op);
    }
    // Phase I.1 — sibling-class static dispatch for cross-class methods without shared __dispatch_<M>.
    if let Some(op) =
        crate::ssa_lower_call_sibling_class_dispatch::try_lower(ctx, eid, callee, args)
    {
        return Some(op);
    }
    // T-26 / T-26.B — `wr.deref()` on Type::WeakRef + WeakMap/WeakSet typed-receiver methods.
    if let Some(op) = crate::ssa_lower_call_weakref_collections::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // P6.2 — `<Set>.{add|has|delete|clear|keys|...|symmetricDifference}` typed-receiver carve-out.
    if let Some(op) = crate::ssa_lower_call_set_dispatch::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // P6.4b/c-C3 — `iter.next()` on Type::MapIter / Type::ArrIter receiver.
    if let Some(op) = crate::ssa_lower_call_iter_next::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // P6.1 — `<Map>.{set|get|has|delete|clear|keys|values|entries|forEach}` carve-out.
    if let Some(op) = crate::ssa_lower_call_map_dispatch::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // v0.2 #2 — Date instance methods (.getTime / .valueOf / .toISOString + 36 sibling methods).
    if let Some(op) = crate::ssa_lower_call_date_methods::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // v0.2 #1 — RegExp instance methods (.test / .exec / .toString) on Type::RegExp.
    if let Some(op) = crate::ssa_lower_call_regex_methods::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // v0.2 #1 Phase 1b — `<Str>.{replace|replaceAll|split|match|matchAll}(re, ...)` regex receiver.
    if let Some(op) = crate::ssa_lower_call_str_regex_methods::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // M6.1 — `<recv>.<method>(args)` for String / Substr / Array stdlib slice (ssa_lower_str sidekick).
    if let Some(v) = crate::ssa_lower_str::try_lower_method_call(ctx, eid, callee, args) {
        return Some(v);
    }
    // `xs.findIndex|findLastIndex|find|findLast|some|every(p)` short-circuit predicate iteration.
    if let Some(op) = crate::ssa_lower_call_arr_predicate::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `xs.flatMap(fn)` — outer xs loop + per-elem callback Array<T> push (S319 widening).
    if let Some(op) = crate::ssa_lower_call_arr_flat_map::try_lower(ctx, eid, callee, args) {
        return Some(op);
    }
    // P6.4c-C3 — `xs.{keys|values|entries}()` ArrIter ctor for Type::Arr<Any> receivers.
    if let Some(op) = crate::ssa_lower_call_arr_iter_ctor::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // M6.2 — `xs.{map|filter|reduce|reduceRight|forEach}(fn[, init?])` carve-out.
    if let Some(op) = crate::ssa_lower_call_arr_ho::try_lower(ctx, eid, callee, args) {
        return Some(op);
    }
    // L3b ⑥ — `f.call(thisArg, ...)` on a fn-typed VALUE: thisArg drops, rest replays the value-callee arms.
    if let Some(op) = crate::ssa_lower_call_fn_call_value::try_lower(ctx, eid, callee, args) {
        return Some(op);
    }
    // M2 — call a Closure-typed local. Load env_ptr + fn_ptr, indirect-call with env prepended.
    if let Some(op) = crate::ssa_lower_call_closure_local::try_lower(ctx, eid, callee, args) {
        return Some(op);
    }
    // M2 Phase B Stage 4 — fn-typed local indirect call + generalized indirect.
    if let Some(op) = crate::ssa_lower_call_fn_indirect::try_lower(ctx, eid, callee, args) {
        return Some(op);
    }
    None
}
