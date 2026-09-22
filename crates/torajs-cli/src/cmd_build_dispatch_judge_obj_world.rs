//! The kernels that re-enter the dispatcher with obj-world receivers
//! from INSIDE the runtime — coercion (OrdinaryToPrimitive, ToPropertyKey)
//! and object probes — moved out of [`crate::cmd_build_dispatch_judge`]
//! when that file reached the 500-line limit. The scan only sees what the
//! user `.o` names, so each entry is a kernel whose inner seam is
//! invisible to it.

use torajs_rc::any_method_family as fam;

/// Any→primitive coercion surface: these kernels run the spec
/// OrdinaryToPrimitive machinery (toString / valueOf mids) against
/// arbitrary heap receivers, `await` resolves thenables, and
/// JSON.stringify consults toJSON. The mids they re-dispatch enter
/// a family ARM only for: user objects' own valueOf/toString
/// (dynobj / struct / closure expandos), Array.prototype.toString
/// (= join, in the arr arm), and the per-family exotic coercions
/// (Date valueOf, RegExp toString, TypedArray join, Symbol/BigInt
/// throw faces) — the latter group is covered by the PREFIX map,
/// because a value of those families cannot exist without its
/// construction/usage symbols being emitted. So a coercion hit
/// keeps the OBJ-world four, not everything. (The prologue's error
/// constructors emit `anyv_to_str_pair` in every program — the
/// spec ToString(message) — which is why this must not be a
/// fallback trigger: it would kill the judgment for the empty
/// program too.)
pub(crate) const COERCION_PREFIXES: [&str; 30] = [
    // The array join kernels run the coercion per ELEMENT rather
    // than on themselves, so the scan never saw `anyv_to_str` in
    // the user `.o` and judged the obj world unreachable. Every
    // face of that: `[{x:1}].join("|")` answered `undefined`, and
    // so did `String([{x:1}])` — but the one that names it is
    // `[{toString(){return "T";}}].join(",")`, where a user method
    // the spec requires to run simply did not. (No sibling entry is
    // needed: `arr_join_exotic` is a runtime-internal delegate to
    // this kernel and is never a lowering's callee, and the locale
    // lanes already worked because they reach their element's own
    // toLocaleString through a mid the scan does see.)
    "__torajs_arr_join_any",
    "__torajs_anyv_to_number",
    // `x++` / `x--` run ToNumeric in the kernel (`eval++` asks for valueOf).
    "__torajs_anyv_incr_",
    "__torajs_anyv_number_ctor",
    "__torajs_anyv_to_str",
    "__torajs_anyv_to_display_str",
    "__torajs_any_to_bigint",
    "__torajs_any_to_object",
    "__torajs_anyv_add_pair",
    "__torajs_anyv_arith_pair",
    "__torajs_anyv_loose_eq",
    "__torajs_anyv_json_stringify",
    // §7.1.19 ToPropertyKey is ToPrimitive first, so a keyed read or
    // write whose KEY is a heap value runs the same OrdinaryToPrimitive
    // machinery the entries above do — `o[k]` where `k` is an object
    // or a function asks `k` for its `toString`. Rotation 608: with a
    // FUNCTION key admitted, nothing else in such a program emits a
    // `__torajs_closure_*` symbol, so the closure arm was stubbed and
    // `o[() => {}]` reached the loud reject instead of the key "() =>
    // {}". The object-key half (rotation 438) was reachable the same
    // way and only ever worked because a program with an object key
    // also had dynobj symbols. The four keyed kernels are here for
    // the same reason as the seam itself: they run ToPropertyKey on
    // the key INSIDE the runtime, so the seam symbol never appears in
    // the user `.o` and only the kernel names are visible to the scan
    // (`__torajs_any_index_method_call` stays a FALLBACK trigger — it
    // also interns a mid, which is strictly more than a coercion).
    "__torajs_anyv_to_property_key",
    "__torajs_any_index_get",
    "__torajs_any_index_get_keyed",
    "__torajs_any_index_set",
    "__torajs_any_index_set_keyed",
    "__torajs_anyv_await",
    // The string / regex kernels that take an `any` pattern, separator,
    // replacement or flags and run ToString on it INSIDE the runtime
    // (§22.1.3.19 step 3, §22.1.3.23 step 4, §22.2.3.1 RegExpInitialize):
    // `s.replace(a, "Z")` with `a` an array or a `{toString}` object
    // reached the stubbed arr / dynobj arm, because the only symbol the
    // user `.o` names is the kernel, whose prefix the family table maps
    // to REGEXP alone.
    "__torajs_str_replace_any_",
    "__torajs_str_regex_replace_any_repl",
    "__torajs_regexp_from_any",
    "__torajs_regex_compile_any",
    "__torajs_str_split_any_sep",
    // §20.4.2.2 Symbol.for step 1 — ToString(key) inside the kernel:
    // `Symbol.for(q)` with `q` a `{toString}` object reached the
    // stubbed dynobj / struct arm.
    "__torajs_symbol_for_any",
    // The any-lane relational / bitwise / unary kernels run ToNumeric
    // on each operand inside the runtime, like the add / arith / loose
    // eq pair kernels above; `q < 1` with `q` a `{ valueOf }` object
    // reached the stubbed dynobj arm.
    "__torajs_anyv_compare_pair",
    "__torajs_anyv_bitwise_pair",
    "__torajs_anyv_bitnot_pair",
    "__torajs_anyv_unary_neg_pair",
    // §25.1.4.1 ToIndex(length) and §22.1.2.4 String.raw's
    // ToLength(raw.length): `new ArrayBuffer(q)` / `String.raw({ raw:
    // { length: q } })`.
    "__torajs_arraybuffer_create",
    "__torajs_string_raw",
];

/// Kernels that PROBE user objects internally — spec machinery that
/// reads a constructor / species / descriptor off an arbitrary
/// object and so re-enters the dispatcher with obj-world receivers
/// (defineProperty's ToNumber(value) runs a user valueOf; the
/// array species guard reads `.constructor` through the expando
/// probe). Emitting one keeps the obj-world four, same as a
/// coercion hit. (The `class_*_define` registration faces are NOT
/// here — their descriptors are compiler-built and the prologue
/// emits them in every program.)
pub(crate) const KERNEL_PROBE_PREFIXES: [&str; 4] = [
    "__torajs_arr_species_guard",
    "__torajs_dynobj_define",
    "__torajs_arr_define",
    "__torajs_anyv_define_props_source_gate",
];

/// What a coercion hit keeps — see [`COERCION_PREFIXES`]. Shared
/// truth with the per-static table (`torajs_rc::ns_static_judge`).
pub(crate) const COERCION_KEEP: u16 = fam::FAM_OBJ_WORLD;
