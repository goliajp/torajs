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

mod dispatch_c;
use dispatch_c::try_dispatch_c;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Operand {
    crate::ssa_lower_iface::refuse_reflection(ctx, callee, args);
    // §13.3.9 — a call the desugar marked as an optional chain runs
    // under its base's nullish guard. It sits OUTSIDE the cascade
    // rather than at the top of it because its hit branch re-enters
    // the cascade for the very same call: that is what keeps the
    // arguments from evaluating when the chain short-circuits, and
    // re-entering here instead would never terminate.
    if let Some(op) = crate::ssa_lower_call_optchain::try_lower(ctx, eid, callee, args) {
        return crate::ssa_lower_call_closure_local::settle_slot_ret(ctx, eid, op);
    }
    // RFC 20260920-struct-user-proto K4 — same outside-the-cascade
    // shape: its static branch re-enters `dispatch` for this call.
    if let Some(op) = crate::ssa_lower_call_reparent_guard::try_lower(ctx, eid, callee, args) {
        return op;
    }
    // §6.2.5.5 GetValue on `r.join` is `ToObject(r)` when `r` holds
    // the cell — a TypeError, and one that precedes the argument
    // list. Armed here, ahead of the cascade, because every typed
    // arm below reads the receiver as a live value of its own type.
    crate::ssa_lower_call_undef_recv::arm(ctx, callee, args);
    let op = dispatch(ctx, eid, callee, args);
    crate::ssa_lower_call_closure_local::settle_slot_ret(ctx, eid, op)
}

/// The dispatcher cascade proper — every lane from the undeclared-read
/// claim down. Reached directly by the optional-chain guard, whose hit
/// branch IS an ordinary call.
pub(crate) fn dispatch(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Operand {
    // RFC 20260730-undeclared-ident — a call whose callee is a
    // marked unresolvable Ident (`ff()` with `ff` declared nowhere)
    // reads the callee first (§6.2.5.5 GetValue precedes the call):
    // the global object's answer, or the recorded ReferenceError
    // (ssa_lower_ident::try_undeclared_read_throw). Claimed before
    // every dispatcher so no name-keyed lane can misroute it; the
    // value the object answers is then CALLED (RFC 20260911: `this.f
    // = …` in global code puts `f` there, and `f(4)` must call it —
    // returning the read alone handed back the function).
    if matches!(ctx.ast.get_expr(callee), Expr::Ident(_))
        && ctx.ast.undeclared_reads.contains_key(&callee)
    {
        return crate::ssa_lower_any_call::lower(ctx, eid, callee, args);
    }
    // Rotation 371 — `__superbuiltin__<m>` is the desugared
    // `super.m()` of a builtin-heritage subclass method; claim the
    // synthetic name before any dispatcher reads it as a program
    // identifier.
    if let Some(op) = crate::ssa_lower_call_super_builtin::try_lower(ctx, callee, args) {
        return op;
    }
    // Rotation 372 — a call whose argument list still carries a
    // dynamic `...spread` needs a RUNTIME argc; claimed before every
    // fixed-argc lane (all of them assume a compile-time list).
    if let Some(op) = crate::ssa_lower_call_spread::try_lower(ctx, callee, args) {
        return op;
    }
    // RFC 20260805 blade 1 — the async driver's intrinsic PromiseResolve.
    // Ahead of the any-call layer: its callee is a synthetic name no
    // declaration binds, which that layer would otherwise read as an
    // unknown global and call (`ReferenceError` at runtime).
    if let Some(op) = crate::ssa_lower_call_await_resolve::try_lower(ctx, callee, args) {
        return op;
    }
    // RFC 20260914 — a self-hosted static's startup registration;
    // same reason it sits ahead of the any-call layer.
    if let Some(op) = crate::ssa_lower_call_ns_static_bind::try_lower(ctx, callee, args) {
        return op;
    }
    // RFC 20260915 — injected source's spec abstract ops (IsConstructor,
    // SpeciesConstructor); same reason.
    if let Some(op) = crate::ssa_lower_call_injected_spec_ops::try_lower(ctx, callee, args) {
        return op;
    }
    // Any-method-call RFC 20260704 — an `any`-typed receiver claims the call before every typed
    // dispatcher. Undefined default pads stay off its argv: the callee reads absent args itself.
    let passed = &args[..crate::ast::source_argc(ctx.ast, eid, args)];
    if let Some(op) = crate::ssa_lower_any_method_call::try_lower(ctx, callee, passed) {
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
    if let Some(op) = crate::ssa_lower_any_call::try_lower(ctx, eid, callee, args) {
        return op;
    }
    // A callee statically KNOWN uncallable (`true()` / `f.length()`)
    // — §13.3.6.2 runtime TypeError via the any-call kernel's
    // non-closure arm; checker mirror in check_type_of_call::general.
    if let Some(op) = crate::ssa_lower_call_uncallable::try_lower(ctx, callee, args) {
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
    if let Some(op) = crate::ssa_lower_call_stage_d::try_dispatch_d(ctx, eid, callee, args) {
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
    if let Some(op) = crate::ssa_lower_call_super_value::try_lower(ctx, callee, args) {
        return Some(op);
    }
    if let Some(op) = crate::ssa_lower_call_script_decl::try_lower(ctx, callee, args) {
        return Some(op);
    }
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
    if let Some(op) = crate::ssa_lower_call_iface_dispatch::try_lower(ctx, eid, callee, args) {
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
    if let Some(op) = crate::ssa_lower_call_vtable_dispatch::try_lower(ctx, eid, callee, args) {
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
    // `Array.from(iter, mapFn?)` — three iter shapes (string / typed Array<T> / Set).
    if let Some(op) = crate::ssa_lower_call_array_from::try_lower(ctx, eid, callee, args) {
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
    // Expression-position `JSON.parse(text)` — any-lane runtime parse kernel.
    if let Some(op) = crate::ssa_lower_call_json_parse::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `String.fromCharCode(...)` / `String.fromCodePoint(...)` variadic — pairwise str_concat chain.
    if let Some(op) = crate::ssa_lower_call_string_from_char_code::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `String.raw(template, ...subs)` — __torajs_string_raw kernel (template.raw + subs).
    if let Some(op) = crate::ssa_lower_call_string_raw::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Object.groupBy(items, cb)` — __torajs_object_group_by kernel (arr + cb via any_call).
    if let Some(op) = crate::ssa_lower_call_object_group_by::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // `Map.groupBy(items, cb)` — sister to Object.groupBy; a SameValueZero-keyed Map.
    if let Some(op) = crate::ssa_lower_call_map_group_by::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // §25.1.5.1 `ArrayBuffer.isView(arg)` — a question about the argument's heap tag.
    if let Some(op) = crate::ssa_lower_call_arraybuffer_static::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // §23.2.2.1-2 `Uint8Array.fromBase64` / `fromHex`.
    if let Some(op) = crate::ssa_lower_call_uint8array_static::try_lower(ctx, callee, args) {
        return Some(op);
    }
    // §23.2.2.1 / .2 `<TypedArray ctor>.from` / `.of` — the boxed ctor
    // cell rides the any-method call into the %TypedArray% kernels.
    if let Some(op) = crate::ssa_lower_call_typedarray_static::try_lower(ctx, callee, args) {
        return Some(op);
    }
    if let Some(op) = crate::ssa_lower_call_proxy_revocable::try_lower(ctx, callee, args) {
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
