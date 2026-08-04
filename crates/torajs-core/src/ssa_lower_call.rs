//! `Expr::Call { callee, args }` 61-layer dispatcher cascade pulled
//! out of [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` arm
//! as chunk-84b of the decomp. Each layer's purpose is documented
//! at its own sibling module; this entry only orchestrates the
//! short-circuit cascade.
//!
//! Order is load-bearing — earlier layers claim narrower shapes
//! (specific Ident `process.on` / `Date.UTC` / Math static / etc.)
//! before generic layers (sibling-class dispatch / stdlib methods /
//! Closure-typed callees). The cascade falls through to
//! [`crate::ssa_lower_call_terminal::emit`] for the generic
//! direct-call path (M3 retarget + arg coercion + emit Call).
//!
//! Split into 4 fns to stay under the 200-LOC fn body hard limit:
//! - `try_dispatch_a` — first 15 layers (early ctors / coercion /
//!   namespace methods / vtable / number methods)
//! - `try_dispatch_b` — next 15 layers (Array.isArray / JSON /
//!   String char-code / bare globals / Date / Math / Number)
//! - `try_dispatch_c` — next 15 layers (BigInt asIntN / Array.of /
//!   Object.{define,getOwn,from,getPrototypeOf,assign,values,is,
//!   keys,hasOwn,entries,integrity} / Reflect.get / class_synth /
//!   Symbol.{registry} / promise_chain)
//! - `try_dispatch_d` — last 16 layers (Bun runtime / fs_promises /
//!   promise_static / console / Array mutators + dispatch / sibling
//!   class / weakref / Set / Map / Date / RegExp / str / arr_ho /
//!   arr_predicate / arr_flat_map / arr_iter_ctor / closure_local /
//!   fn_indirect)

use crate::ast::{Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Operand {
    // RFC 20260730-undeclared-ident — a call whose callee is a
    // marked unresolvable Ident (`ff()` with `ff` declared nowhere)
    // raises the ReferenceError at the callee read (§6.2.5.5
    // GetValue precedes the call). Claimed before every dispatcher
    // so no name-keyed lane can misroute it; lowering the callee
    // emits the throw (ssa_lower_ident::try_undeclared_read_throw).
    if matches!(ctx.ast.get_expr(callee), Expr::Ident(_))
        && ctx.ast.undeclared_reads.contains_key(&callee)
    {
        return ctx.lower_expr(callee);
    }
    // Any-method-call RFC 20260704 — an `any`-typed receiver claims
    // the call before every typed dispatcher (their name matches
    // assume concrete receiver types).
    if let Some(op) = crate::ssa_lower_any_method_call::try_lower(ctx, callee, args) {
        return op;
    }
    // RFC 20260728-gen-forof-yieldstar F0b — `recv[key](args…)` on an
    // any receiver is a METHOD call (§13.3.6.2 thisValue = base);
    // claims the Index callee before the bare any-call layer would
    // read it as a value and call it receiverless.
    if let Some(op) = crate::ssa_lower_index_any_method_call::try_lower(ctx, callee, args) {
        return op;
    }
    // RFC C4+ — bare call on an `any`-typed callee (`f(1)` where f
    // erased to any) routes to the runtime closure dispatch.
    if let Some(op) = crate::ssa_lower_any_call::try_lower(ctx, callee, args) {
        return op;
    }
    if let Some(op) = try_dispatch_a(ctx, eid, callee, args) {
        return op;
    }
    if let Some(op) = try_dispatch_b(ctx, callee, args) {
        return op;
    }
    if let Some(op) = try_dispatch_c(ctx, callee, args) {
        return op;
    }
    if let Some(op) = try_dispatch_d(ctx, eid, callee, args) {
        return op;
    }
    crate::ssa_lower_call_terminal::emit(ctx, eid, callee, args)
}

fn try_dispatch_a(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(v) = crate::ssa_lower_process_on::try_lower(ctx, callee, args) {
        return Some(v);
    }
    // S153 — `Date.UTC(y, m?, d?, h?, min?, s?, ms?)` arity 1-6 trailing-default padding.
    if let Some(op) = crate::ssa_lower_call_date_utc_pad::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // T-45 — synthetic `__torajs_in_op(key, obj)` from binary `in` rewrite.
    // §13.10 ergonomic brand check — `__torajs_priv_in_op` (`#x in o`).
    if let Some(op) = crate::ssa_lower_call_in_op::try_lower_priv(ctx, callee, args) {
        return Some(op);
    }
    if let Some(op) = crate::ssa_lower_call_in_op::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // V3-18 m1.h.8 — `Number(x)` / `String(x)` / `Boolean(x)` callable coercion (ES §7.1.{2,4,17}).
    if let Some(op) = crate::ssa_lower_call_coercion::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // RFC 20260716 刀 4 — `Object(x)` callable coercion (ES §20.1.1.1 + ToObject §7.1.18).
    if let Some(op) = crate::ssa_lower_call_object_coerce::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // V3-03 — `BigInt(value)` callable ctor (Type::BigInt → clone; Str → from_str; F64/I64 → from_number).
    if let Some(op) = crate::ssa_lower_call_bigint_ctor::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // T-13.a — `Symbol(desc?)` direct constructor call.
    if let Some(op) = crate::ssa_lower_call_symbol_ctor::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // V3-18 m2.b — Object.prototype subset on constructor-namespace objects.
    if let Some(op) = crate::ssa_lower_call_namespace_obj_methods::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // P3.struct-method-dispatch — `obj.method()` on Type::Obj(sid) struct with FnSig/Closure field.
    if let Some(op) =
        crate::ssa_lower_call_struct_method_dispatch::try_lower(ctx, eid, callee, args)
    {
        return Some(op);
    }
    // V3-18 m2.a / m2.d — Object.prototype methods on primitives (auto-box) + struct instances.
    if let Some(op) = crate::ssa_lower_call_universal_methods::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // T-24 virtual-dispatch via vtable — synthetic `__dispatch_<M>(obj, args)` + CallIndirect.
    if let Some(op) = crate::ssa_lower_call_vtable_dispatch::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `n.{toFixed|toString|toLocaleString|toExponential|toPrecision}` — primitive number wedges.
    if let Some(op) = crate::ssa_lower_call_number_methods::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // RFC 20260719-fn-tostring-source B4b — `f.toString()` on a top-level fn ident
    // folds to the type-erased source text (checker route_early wedge mirror).
    if let Some(op) = crate::ssa_lower_call_fn_tostring::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Array.isArray(value)` — compile-time static check (ES §23.1.2.2).
    if let Some(op) = crate::ssa_lower_call_array_is_array::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `JSON.stringify(value, replacer?, space?)` — recursive type-aware serializer.
    if let Some(op) = crate::ssa_lower_call_json_stringify::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `JSON.rawJSON(text)` / `JSON.isRawJSON(O)` — ES2026 json-parse-with-source kernels.
    if let Some(op) = crate::ssa_lower_call_json_raw::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `String.fromCharCode(...)` / `String.fromCodePoint(...)` variadic — pairwise str_concat chain.
    if let Some(op) = crate::ssa_lower_call_string_from_char_code::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `String.raw(template, ...substitutions)` — dispatch to
    // __torajs_string_raw kernel (walks template.raw + interleaved subs).
    if let Some(op) = crate::ssa_lower_call_string_raw::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Object.groupBy(items, cb)` — Array items lane, dispatch to
    // __torajs_object_group_by kernel (walks arr + cb via any_call).
    if let Some(op) = crate::ssa_lower_call_object_group_by::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Map.groupBy(items, cb)` — sister to Object.groupBy;
    // accumulator is a Map with SameValueZero keys.
    if let Some(op) = crate::ssa_lower_call_map_group_by::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Iterator.from(O)` — GetIteratorFlattenable + wrap-or-pass.
    if let Some(op) = crate::ssa_lower_call_iterator_from::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Iterator.concat(...items)` — eager iterability check + lazy
    // kind-CONCAT helper cell.
    if let Some(op) = crate::ssa_lower_call_iterator_concat::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Iterator.zip(iterables, options)` — eager opens + lazy
    // kind-ZIP helper cell.
    if let Some(op) = crate::ssa_lower_call_iterator_zip::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // Bare-name JS globals: parseInt / parseFloat / isNaN / isFinite / queueMicrotask.
    if let Some(op) = crate::ssa_lower_call_bare_globals::try_lower(ctx, callee, args) {
        return Some(op);
    }
    None
}

fn try_dispatch_b(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
    // S230 — `Date.parse(undefined)` static-fold (ES §21.4.3.2).
    if let Some(op) = crate::ssa_lower_call_date_parse_undef_fold::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Math.hypot(...args)` variadic — sqrt(sum of args²).
    if let Some(op) = crate::ssa_lower_call_math_hypot::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // S203 + S227 — `Math.<unary>(...)` 0-arg / single-undefined-arg fold.
    if let Some(op) = crate::ssa_lower_call_math_unary_undef_fold::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // S205 + S228 — `Math.{pow|atan2|imul}` <2-arg / undefined-arg fold.
    if let Some(op) = crate::ssa_lower_call_math_binary_undef_fold::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Math.min(...)` / `Math.max(...)` variadic pairwise reduction.
    if let Some(op) = crate::ssa_lower_call_math_min_max::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Number.<m>(args)` namespace dispatch — 6 methods (parseInt/parseFloat/isInteger/...).
    if let Some(op) = crate::ssa_lower_call_number_namespace::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // P12.4-B/C — `BigInt.{asIntN|asUintN}(bits, value)` per ES §21.2.2.{1,2}.
    if let Some(op) = crate::ssa_lower_call_bigint_as_int_n::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Array.of(...vals)` — no-spread array literal SSA shape (arr_alloc + len + slot stores).
    if let Some(op) = crate::ssa_lower_call_array_of::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // Object.defineProperty / defineProperties (RFC C1/C2) — property-descriptor trunk.
    if let Some(v) = crate::ssa_lower_object_define::try_lower(ctx, callee, args) {
        return Some(v);
    }
    // ES2025 §22.2.5.1 — RegExp.escape strict-String any shell.
    if let Some(op) = crate::ssa_lower_call_regexp_escape::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // P3.getOwnPropertyDescriptor — Object.getOwnPropertyDescriptor (W-M / RFC C5a + S315).
    if let Some(op) =
        crate::ssa_lower_call_object_get_property_descriptor::try_lower(ctx, callee, args)
    {
        return Some(op);
    }
    // `Array.from(iter, mapFn?)` — three iter shapes (string / typed Array<T> / Set).
    if let Some(op) = crate::ssa_lower_call_array_from::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // Class-synthesis register globals (__torajs_proto_register / class_register / ...).
    if let Some(op) = crate::ssa_lower_call_class_synth::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // P4.2 Phase B+C — `Object.getPrototypeOf(<arg>)`.
    if let Some(op) = crate::ssa_lower_call_object_get_prototype_of::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // S127-3 — `Object.assign(target, ...sources)` per ES §20.1.2.1.
    if let Some(op) = crate::ssa_lower_call_object_assign::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // V3-18 m1.h.5 — `Object.values(obj)` namespace static.
    if let Some(op) = crate::ssa_lower_call_object_values::try_lower(ctx, callee, args) {
        return Some(op);
    }
    None
}

fn try_dispatch_c(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
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

fn try_dispatch_d(
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
    if let Some(op) = crate::ssa_lower_call_sibling_class_dispatch::try_lower(ctx, callee, args) {
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
    if let Some(v) = crate::ssa_lower_str::try_lower_method_call(ctx, callee, args) {
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
