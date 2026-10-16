//! The kernels that re-enter the dispatcher with obj-world receivers
//! from INSIDE the runtime — moved out of
//! [`crate::cmd_build_dispatch_judge`] when that file reached the
//! 500-line limit. The scan only sees what the user `.o` names, so
//! each entry is a kernel whose inner re-dispatch is invisible to it.
//!
//! **What puts a kernel here** (rotation 693, after RFC
//! `20260922-to-primitive-get-call`): its body reaches a dispatcher
//! entry — `any_method_call_inner` / `any_method_redispatch` /
//! `__torajs_any_method_dispatch` — through private helpers, as
//! measured by [`crate::cmd_build_dispatch_judge_census`]. The table
//! is that census's output, not a hand-written list.
//!
//! Until that RFC the seam was the private OrdinaryToPrimitive
//! machinery, and the coercion kernels reached a user `toString` /
//! `valueOf` by re-dispatching a mid. They now reach it by Get +
//! Call (§7.1.1.1), which removed the seam edge for 41 kernels. What
//! survives for the rest is the **Call** step: the property may hold
//! a borrowed BUILTIN method unit (`({toString: Array.prototype
//! .join})`), and calling one still enters the dispatcher — the
//! five-row direct table covers root props only. So the ToString /
//! ToNumber / ToPropertyKey families stay; what left is every kernel
//! whose only seam edge WAS OrdinaryToPrimitive itself (the regex
//! any-argument lane, the builtin-subclass `super(v)` lane,
//! `Symbol.for`, `String.raw`, `new ArrayBuffer(q)`, `xs.length = q`
//! and the rest).

use torajs_rc::any_method_family as fam;

/// Kernels whose body reaches a dispatcher entry (see the module
/// doc). The mids they re-dispatch enter a family ARM only for: user
/// objects' own valueOf/toString (dynobj / struct / closure
/// expandos), Array.prototype.toString (= join, in the arr arm), and
/// the per-family exotic coercions (Date valueOf, RegExp toString,
/// TypedArray join, Symbol/BigInt throw faces) — the latter group is
/// covered by the PREFIX map, because a value of those families
/// cannot exist without its construction/usage symbols being
/// emitted. So a hit keeps the OBJ-world four, not everything. (The
/// prologue's error constructors emit `anyv_to_str_pair` in every
/// program — the spec ToString(message) — which is why this must not
/// be a fallback trigger: it would kill the judgment for the empty
/// program too. `SYNTH_ERROR_FNS` in the judge is what keeps that
/// per-program tax off; it cannot retire while ToString is here.)
pub(crate) const COERCION_PREFIXES: [&str; 57] = [
    "__torajs_anyv_to_number",
    // `x++` / `x--` run ToNumeric in the kernel (`eval++` asks for valueOf).
    "__torajs_anyv_incr_",
    "__torajs_anyv_to_str",
    "__torajs_any_to_bigint",
    "__torajs_anyv_add_pair",
    "__torajs_anyv_arith_pair",
    "__torajs_anyv_loose_eq",
    "__torajs_anyv_json_stringify",
    // §7.1.19 ToPropertyKey is ToPrimitive first, so a keyed read or
    // write whose KEY is a heap value runs the same machinery the
    // entries above do — `o[k]` where `k` is an object or a function
    // asks `k` for its `toString`. Rotation 608: with a FUNCTION key
    // admitted, nothing else in such a program emits a
    // `__torajs_closure_*` symbol, so the closure arm was stubbed and
    // `o[() => {}]` reached the loud reject instead of the key "() =>
    // {}". The object-key half (rotation 438) was reachable the same
    // way and only ever worked because a program with an object key
    // also had dynobj symbols. (`__torajs_any_index_method_call`
    // stays a FALLBACK trigger — it also interns a mid, which is
    // strictly more than a coercion.)
    "__torajs_anyv_to_property_key",
    "__torajs_any_index_get",
    "__torajs_any_index_get_keyed",
    "__torajs_any_index_set",
    "__torajs_any_index_set_keyed",
    // The any-lane relational / bitwise / unary kernels run ToNumeric
    // on each operand inside the runtime, like the add / arith / loose
    // eq pair kernels above; `q < 1` with `q` a `{ valueOf }` object
    // reached the stubbed dynobj arm.
    "__torajs_anyv_compare_pair",
    "__torajs_anyv_bitwise_pair",
    "__torajs_anyv_bitnot_pair",
    "__torajs_anyv_unary_neg_pair",
    // §13.3.7.1 `super[k]`: ToPropertyKey(k) in the kernel, every
    // face (get / set / call / optional call / update).
    "__torajs_anyv_super_prop_",
    // `super(…)` against a Proxy or bound parent constructs through the
    // runtime, which calls the trap / target with the receiver.
    "__torajs_anyv_super_call",
    // Rotation 691's census — kernels whose bodies reach the seam, or
    // a kernel above, through private helpers. Probed live
    // loud-rejects: `a.length = obj` on an any array (ArraySetLength
    // inside `any_member_set`), `JSON.parse(obj)` with a reviver. The
    // rest answered right in the probe only because another symbol of
    // the same program happened to keep the arms.
    "__torajs_any_member_set",
    // ToPropertyKey(key) inside `k in o`, `delete o[k]` and the
    // any-lane accessor / length / name reads.
    "__torajs_any_accessor_get",
    "__torajs_any_has_property",
    "__torajs_any_length_get",
    "__torajs_any_name_get",
    "__torajs_any_prop_delete",
    // Rotation 693: an accessor's getter is called with the receiver,
    // and the pair may hold a borrowed builtin method unit
    // (`Object.defineProperty(o, "x", { get: Array.prototype.join })`),
    // so the call enters the dispatcher. New under this seam — the
    // old one only ever saw coercion.
    "__torajs_accessor_invoke_getter",
    "__torajs_array_from_dyn",
    "__torajs_json_parse_reviver",
    // Rotation 693, second wave: wiring the proxy `get` trap into the
    // symbol-key read (§10.5.1 applies to every key kind) gave the
    // symbol lane a dispatcher edge it did not have before, and these
    // four reach it. `__torajs_iterator_from` had left the table
    // earlier the same rotation, on the measurement before that edge
    // existed — which is the census doing its job in both directions
    // rather than a list being edited by hand.
    "__torajs_any_str_symbol_try",
    "__torajs_anyv_reject_regexp_needle",
    "__torajs_iterator_concat",
    "__torajs_iterator_from",
    // a builtin collection subclass's `super(iterable)` walks it.
    "__torajs_map_subclass_super",
    "__torajs_set_subclass_super",
    "__torajs_weakmap_subclass_super",
    "__torajs_weakset_subclass_super",
    "__torajs_arr_typed_to_locale_string",
    "__torajs_bigint_ctor_any",
    "__torajs_collection_add_static",
    "__torajs_collection_init_from_iterable",
    "__torajs_date_from_value",
    "__torajs_error_tostring_dispatch",
    "__torajs_fnsig_to_primitive_str",
    "__torajs_instanceof_dynamic",
    "__torajs_iterator_zip",
    // the combinators walk an arbitrary iterable (§27.2.4.1.1): its
    // `next` is dispatched on whatever `@@iterator` answered.
    "__torajs_promise_all_",
    "__torajs_promise_allsettled_",
    "__torajs_promise_any_",
    "__torajs_promise_race_",
    "__torajs_reflect_apply",
    "__torajs_reflect_construct",
    "__torajs_set_relation_setlike",
    "__torajs_set_setop_setlike",
    "__torajs_str_locale_case_",
    // the toLocaleString gate walks a locales list (ToString of each
    // object entry) and reads options through [[Get]] (getters run).
    "__torajs_locale_args_check",
    // the object cast reads each field of the layout with [[Get]] on the
    // value (§7.3.2), so an accessor's getter runs with that object as
    // the receiver — and the getter may be a borrowed builtin method
    // unit, which the call dispatches. Both entries (plain and the
    // optional-member one) share the prefix.
    "__torajs_anyv_cast_obj",
    // the typed read of a deleted object-literal accessor asks the
    // prototype with [[Get]] (§10.1.8.1 step 3), whose getter runs on
    // the instance and may be a borrowed builtin method unit.
    "__torajs_obj_deleted_member_get",
];

/// What a hit keeps — see [`COERCION_PREFIXES`]. Shared truth with
/// the per-static table (`torajs_rc::ns_static_judge`).
pub(crate) const COERCION_KEEP: u16 = fam::FAM_OBJ_WORLD;
