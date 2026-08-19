//! Pass 0 `declare_intrinsic` group: fn/arr-as-object side tables +
//! Array<Any> drop + AnyValue ops + proto/class registry +
//! any-unbox/box-drop.
//!
//! chunk 122 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-121). 26 declarations covering the contiguous source
//! block from `fnprops_set` through `any_box_drop` (i.e. the
//! `__torajs_fnprops_*` / `__torajs_arrprops_*` / `__torajs_arr_drop_any`
//! / canonical `__torajs_anyv_*` NaN-box AnyValue family / proto +
//! class registry / `__torajs_anyv_unbox_*` + `_rc_dec` block).
//!
//! Subgroups (source order):
//! - **fn-as-object** (T-27.b): `fnprops_set`, `fnprops_get_tag`,
//!   `fnprops_get_value` — hashmap keyed by fn pointer; lazy dynobj
//!   alloc on first prop write.
//! - **arr-as-object** (T-29): `arrprops_set` / `_get_tag` /
//!   `_get_value`.
//! - **Array<Any> drop**: `arr_drop_any`.
//! - **AnyValue ops** (Step 7f-B canonical `__torajs_anyv_*`): `typeof`,
//!   `to_bool`, `to_number`, `add_pair`, `arith_pair`, `compare_pair`,
//!   `strict_eq_imm_pair` (i.e. one operand still typed Any, the
//!   other split into i64-pair), `strict_eq` (both Any), `box_from_pair`,
//!   `payload_rc_inc_pair`.
//! - **Proto/class registry**: `proto_register`, `register_native_error`
//!   (P7.4-a-2 — slot enum: 0=Error 1=TypeError 2=RangeError; factory
//!   = codegen'd `__new_<C>` address), `proto_get`, `class_register`,
//!   `class_get`, `get_proto_of_any`.
//! - **Any unbox / drop**: `anyv_unbox_tag`, `anyv_unbox_value`,
//!   `anyv_rc_dec` (= legacy `any_box_drop`).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct AnySubstrateIds {
    pub fnprops_set: FuncId,
    pub fnprops_get_tag: FuncId,
    pub fnprops_get_value: FuncId,
    pub arrprops_set: FuncId,
    pub arrprops_get_tag: FuncId,
    pub arrprops_get_value: FuncId,
    pub arr_member_value: FuncId,
    /// RFC 20260712-arr-exotic-define chunk B — Array
    /// DefineOwnProperty kernel (§10.4.2.1 index/length/expando
    /// dispatch + §10.1.6.3 validation).
    pub arr_define: FuncId,
    pub arr_drop_any: FuncId,
    pub any_typeof: FuncId,
    pub any_to_bool: FuncId,
    pub any_to_number: FuncId,
    /// RFC 20260720 刀 5b-2 — ToBigInt (§7.1.13) for the Any→BigInt
    /// call-boundary coerce (owned BigInt out; NULL = pending throw).
    pub any_to_bigint: FuncId,
    /// RFC 20260716 刀 4 — `Object(v)` callable coercion (ES §20.1.1.1
    /// + ToObject §7.1.18). Primitives mint a fresh wrapper; heap
    /// cells identity (rc_inc + return recv); null/undef fresh {}.
    pub any_to_object: FuncId,
    pub any_add: FuncId,
    pub any_arith: FuncId,
    /// RFC 20260716 刀 7 — `&` / `|` / `^` / `<<` / `>>` / `>>>`
    /// on Any operands per ES §13.12. Both operands `ToNumber` →
    /// `ToInt32` (`ToUint32` on `>>>`'s LHS); result boxed as I32
    /// or F64 (`>>>` results in `[2^31, 2^32)`).
    pub any_bitwise: FuncId,
    /// RFC 20260716 刀 7 — unary `~` on an Any operand per ES §13.5.6.
    /// Operand `ToNumber` → `ToInt32` → `xor -1`.
    pub any_bitnot: FuncId,
    /// `x++` / `x--` on an `any` slot per ES §13.4.4 / §13.4.5. Takes
    /// the slot pointer rather than the loaded value: §13.4.4.1 puts a
    /// ToNumeric between the load and the add that must run exactly
    /// once (a second one would call `valueOf` twice) and that picks
    /// the numeric domain — BigInt or Number — for the step. The whole
    /// read-modify-write therefore stays on the runtime side, which
    /// also keeps the replaced value's release next to the store. The
    /// result is the COERCED old value (`s = "5"; s++` answers 5).
    pub any_incr_slot: FuncId,
    pub any_compare: FuncId,
    pub any_strict_eq: FuncId,
    /// SameValueZero pair variant (§7.2.9) — `includes` with an
    /// `any` needle over a typed-element receiver (NaN equals NaN).
    pub any_svz: FuncId,
    pub any_any_strict_eq: FuncId,
    /// RFC 20260713-loose-eq-substrate blade 1 — IsLooselyEqual
    /// (§7.2.14) full coercion ladder over two AnyValues.
    pub any_any_loose_eq: FuncId,
    pub any_box: FuncId,
    pub anyv_box_str_slot: FuncId,
    /// Rotation 185 — Substr mirror of `anyv_box_str_slot`: the
    /// undefined sentinel VIEW decodes to VALUE_UNDEFINED at the
    /// any boundary; heap views stay a pure tag-4 encode.
    pub anyv_box_substr_slot: FuncId,
    pub anyv_str_slot_tag: FuncId,
    pub anyv_str_slot_value: FuncId,
    /// Rotation 185 — Substr mirrors of the Str-slot pair decode
    /// (the (tag, value) boundary twin of `anyv_box_substr_slot`).
    pub anyv_substr_slot_tag: FuncId,
    pub anyv_substr_slot_value: FuncId,
    pub any_payload_rc_inc: FuncId,
    /// RFC 20260708-closure-argv-face — whole-box heap-payload
    /// retain (`__torajs_anyv_retain`); immediates no-op.
    pub anyv_retain: FuncId,
    pub proto_register: FuncId,
    pub register_native_error: FuncId,
    pub proto_get: FuncId,
    /// G2 (rotation 178) — generator-factory fncell mint installs
    /// the `__Gen_<name>` class proto as the cell's `.prototype`.
    pub closure_install_gen_proto: FuncId,
    /// L3b ④ — boxed builtin-constructor value for the bare
    /// namespace ident read (`Object` as a VALUE).
    pub builtin_ctor_value: FuncId,
    /// RFC 20260719-ns-static-value-reify — the interned dispatcher
    /// cell for a namespace static read as a VALUE (`Math.max`);
    /// Closure-repr borrow of an immortal cell.
    pub ns_static_cell: FuncId,
    /// RFC 20260719-ns-static-value-reify B3b — `JSON.stringify` over
    /// an any-lane value (the typed tier unfolds a per-shape walk;
    /// this is the runtime twin). NULL answer = `undefined` result.
    pub anyv_json_stringify: FuncId,
    pub anyv_json_stringify_gap: FuncId,
    pub anyv_json_gap_str: FuncId,
    pub class_register: FuncId,
    /// RFC 20260718-builtin-error-ctor-first-class 刀 1 — installs
    /// the §20.5.6.3/6.4 own `name` / `message` data properties on
    /// an injected error class's `__proto_<C>` (`tag, name Str`).
    pub error_proto_install: FuncId,
    /// RFC 20260718 刀 3 — §20.5.2.1 [[ErrorData]] probe
    /// (`Error.isError`'s injected static-method body).
    pub error_is_error: FuncId,
    pub class_get: FuncId,
    pub get_proto_of_any: FuncId,
    pub proto_member_get: FuncId,
    /// `Error.prototype.toString` (§20.5.3.4) over a FLAG_ERROR OBJ
    /// instance pointer — `name + ": " + message` with empty-side
    /// special cases. Returns a fresh (owned) Str.
    pub error_to_string: FuncId,
    /// rotation 141 — `<error>.toString()` typed-tier dispatch:
    /// probes the class prototype chain for a monkey-patched
    /// `toString` before the fixed-offset formatter; NULL answer =
    /// pending throw recorded.
    pub error_tostring_dispatch: FuncId,
    /// 刀 3 — derived-ctor no-super ReferenceError raiser (§9.2.2
    /// this-TDZ; records the pending throw, message baked in).
    pub ctor_no_super_throw: FuncId,
    /// RFC 20260730-undeclared-ident — §6.2.5.5 GetValue on an
    /// unresolvable Reference: raises `<name> is not defined` as a
    /// catchable ReferenceError. `name` is the identifier Str.
    pub throw_reference_error_name: FuncId,
    /// RFC 20260713 blade 5 cut 4 — %GeneratorFunction.prototype% /
    /// %AsyncGeneratorFunction.prototype% singleton (kind 0/1) and
    /// the per-generator-proto → %GeneratorPrototype% chain writer.
    pub genfn_proto: FuncId,
    pub genfn_chain: FuncId,
    /// Knife B cut 2 (RFC 20260717-class-first-class-value) — static
    /// method reification: define one reified adapter cell onto the
    /// class object (`tag, name Str, adapter vaddr`).
    pub static_method_define: FuncId,
    pub static_field_define: FuncId,
    pub class_cell_raw: FuncId,
    pub proto_cell_raw: FuncId,
    /// RFC 20260718-accessor-reify 刀 2 — class-accessor
    /// reification: define one AccessorPair own entry onto the class
    /// prototype (`tag, name Str, get vaddr, set vaddr`).
    pub class_accessor_define: FuncId,
    /// 刀 3 static twin — AccessorPair own entry onto the class object.
    pub class_static_accessor_define: FuncId,
    pub any_index_get: FuncId,
    /// Cluster #1 blade 3 — `recv[key]` where both sides are `any`:
    /// runtime ToPropertyKey dispatch (numeric lane / Str / Symbol
    /// probe / ToString fallback).
    pub any_index_get_keyed: FuncId,
    /// §7.1.19 ToPropertyKey as a key cell (owned) + its releaser —
    /// the define family's `any`-key arm.
    pub anyv_to_property_key: FuncId,
    pub anyv_property_key_drop: FuncId,
    /// Write mirror of the keyed read.
    pub any_index_set_keyed: FuncId,
    pub any_index_set: FuncId,
    pub any_length_get: FuncId,
    pub any_name_get: FuncId,
    pub any_size_get: FuncId,
    pub any_regexp_prop: FuncId,
    pub any_member_get_tag: FuncId,
    pub any_member_get_value: FuncId,
    pub any_accessor_get: FuncId,
    pub any_member_set: FuncId,
    pub any_iter_next: FuncId,
    /// `for await (v of <any>)` — same cascade with the §14.7.5.6
    /// step-await / §27.1.4.4 value-await taps.
    pub any_iter_next_await: FuncId,
    /// `Array.from`'s entry to the same walk — §23.1.2.1 step 3 takes
    /// the array-like branch where every other consumer throws.
    pub any_iter_next_array_like: FuncId,
    pub any_iter_close: FuncId,
    pub iter_close_value: FuncId,
    pub any_call: FuncId,
    /// §6.2.6.5 IsCallable unbox for an Any-typed accessor face.
    pub accessor_face_from_any: FuncId,
    pub closure_call_variadic: FuncId,
    pub any_method_call: FuncId,
    pub any_method_call_opt: FuncId,
    /// §13.3.6.2 `recv[key](args…)` with a runtime key — RFC
    /// 20260728-gen-forof-yieldstar F0b.
    pub any_index_method_call: FuncId,
    pub any_method_probe: FuncId,
    pub any_prop_delete: FuncId,
    pub any_prop_has: FuncId,
    /// §7.3.11 HasProperty — own probe + user [[Prototype]] chain
    /// (RFC 20260721 刀 5 R-F; the for-in mid-loop guard's probe).
    pub any_has_property: FuncId,
    /// For-in typed-Arr liveness re-check (RFC 20260721 刀 12 G16).
    pub arr_forin_key_live: FuncId,
    /// RFC 20260711-closure-reflection chunk A — static
    /// `<Ctor>.prototype.<m>` method-value read (monkey-patch probe
    /// + interned method cell + undefined).
    pub builtin_proto_method_value: FuncId,
    /// RFC 20260725-str-method-value-reify — the interned cell for a
    /// compile-time-known (family, mid) pair; a typed-receiver
    /// method VALUE read (`const m = s.slice`) mints through this.
    pub builtin_method_cell_tagged: FuncId,
    pub any_unbox_tag: FuncId,
    pub any_unbox_value: FuncId,
    pub any_cell_ptr: FuncId,
    pub any_unbox_value_owned: FuncId,
    pub any_unbox_settle: FuncId,
    pub any_box_drop: FuncId,
    pub any_box_rc_inc: FuncId,
    /// S-NEW 刀 2 — record a class object's boxed factory adapter, so
    /// `new <expr>()` can find it once the callee has been evaluated.
    pub ctor_register: FuncId,
    /// S-NEW 刀 2 — `new <runtime value>(args…)`: IsConstructor, then
    /// the factory adapter.
    pub construct: FuncId,
    /// S-NEW 刀 3 — §7.2.4 IsConstructor as a value-level predicate,
    /// which is the whole of test262's `isConstructor.js`.
    pub is_constructor: FuncId,
}

// CARVE-OUT: dispatch table — the body is one `decl!` line per
// runtime helper filling a struct literal, whose field order the
// callers read by name; same shape as the intrinsics_table /
// intrinsics_map_set / intrinsics_print_freeze declarations already
// carved out, and it grows by one line per helper the runtime gains.
pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> AnySubstrateIds {
    // defined inside the fn so `module` / `fn_table` resolve at the
    // macro definition site (macro_rules locals are hygienic) — the
    // chunk 457 intrinsics_object convergence pattern
    macro_rules! decl {
        ($name:literal, [$($param:ident),*], $ret:ident) => {
            declare_intrinsic(module, fn_table, $name, &[$(Type::$param),*], Type::$ret)
        };
    }
    AnySubstrateIds {
        fnprops_set: decl!("__torajs_fnprops_set", [Ptr, Ptr, I64, I64], Void),
        fnprops_get_tag: decl!("__torajs_fnprops_get_tag", [Ptr, Ptr], I64),
        fnprops_get_value: decl!("__torajs_fnprops_get_value", [Ptr, Ptr], I64),
        arrprops_set: decl!("__torajs_arrprops_set", [Ptr, Ptr, I64, I64], Void),
        arr_define: decl!("__torajs_arr_define", [Ptr, Ptr, I64, I64, I64], Void),
        arrprops_get_tag: decl!("__torajs_arrprops_get_tag", [Ptr, Ptr], I64),
        arrprops_get_value: decl!("__torajs_arrprops_get_value", [Ptr, Ptr], I64),
        arr_member_value: decl!("__torajs_arr_member_value", [Ptr, Ptr], Any),
        arr_drop_any: decl!("__torajs_arr_drop_any", [Ptr], Void),
        any_typeof: decl!("__torajs_anyv_typeof", [Any], Str),
        any_to_bool: decl!("__torajs_anyv_to_bool", [Any], Bool),
        any_to_number: decl!("__torajs_anyv_to_number", [Any], F64),
        any_to_bigint: decl!("__torajs_any_to_bigint", [Any], BigInt),
        any_to_object: decl!("__torajs_any_to_object", [Any], Any),
        any_add: decl!("__torajs_anyv_add_pair", [I64, I64, I64, I64], Any),
        any_arith: decl!("__torajs_anyv_arith_pair", [I64, I64, I64, I64, I64], Any),
        any_bitwise: decl!("__torajs_anyv_bitwise_pair", [I64, I64, I64, I64, I64], Any),
        any_bitnot: decl!("__torajs_anyv_bitnot_pair", [I64, I64], Any),
        any_incr_slot: decl!("__torajs_anyv_incr_slot", [Ptr, I64], Any),
        any_compare: decl!(
            "__torajs_anyv_compare_pair",
            [I64, I64, I64, I64, I64],
            Bool
        ),
        any_strict_eq: decl!("__torajs_anyv_strict_eq_imm_pair", [Any, I64, I64], Bool),
        any_svz: decl!("__torajs_anyv_svz_imm_pair", [Any, I64, I64], Bool),
        any_any_strict_eq: decl!("__torajs_anyv_strict_eq", [Any, Any], Bool),
        any_any_loose_eq: decl!("__torajs_anyv_loose_eq", [Any, Any], Bool),
        any_box: decl!("__torajs_anyv_box_from_pair", [I64, I64], Any),
        anyv_box_str_slot: decl!("__torajs_anyv_box_str_slot", [Str], Any),
        anyv_box_substr_slot: decl!("__torajs_anyv_box_substr_slot", [Substr], Any),
        anyv_str_slot_tag: decl!("__torajs_anyv_str_slot_tag", [Str], I64),
        anyv_str_slot_value: decl!("__torajs_anyv_str_slot_value", [Str], I64),
        anyv_substr_slot_tag: decl!("__torajs_anyv_substr_slot_tag", [Substr], I64),
        anyv_substr_slot_value: decl!("__torajs_anyv_substr_slot_value", [Substr], I64),
        any_payload_rc_inc: decl!("__torajs_anyv_payload_rc_inc_pair", [I64, I64], Void),
        anyv_retain: decl!("__torajs_anyv_retain", [Any], Any),
        proto_register: decl!("__torajs_anyv_proto_register", [I64, Any], Void),
        register_native_error: decl!("__torajs_register_native_error", [I64, Ptr], Void),
        proto_get: decl!("__torajs_anyv_proto_get", [I64], Any),
        closure_install_gen_proto: decl!("__torajs_closure_install_gen_proto", [Ptr, Any], Void),
        builtin_ctor_value: decl!("__torajs_builtin_ctor_value", [I64], Any),
        ns_static_cell: decl!("__torajs_ns_static_cell", [I64], Ptr),
        anyv_json_stringify: decl!("__torajs_anyv_json_stringify", [Any], Str),
        anyv_json_stringify_gap: decl!("__torajs_anyv_json_stringify_gap", [Any, Str, I64], Str),
        anyv_json_gap_str: decl!("__torajs_anyv_json_gap_str", [Any], Str),
        class_register: decl!(
            "__torajs_anyv_class_register",
            [I64, Any, I64, I64, I64],
            Void
        ),
        error_proto_install: decl!("__torajs_error_proto_install", [I64, Str], Void),
        error_is_error: decl!("__torajs_error_is_error", [Any], Bool),
        static_method_define: decl!(
            "__torajs_class_static_method_define",
            [I64, Str, I64, I64, I64],
            Void
        ),
        static_field_define: decl!(
            "__torajs_class_static_field_define",
            [I64, Str, I64, I64],
            Void
        ),
        class_cell_raw: decl!("__torajs_class_cell_raw", [I64], I64),
        proto_cell_raw: decl!("__torajs_proto_cell_raw", [I64], I64),
        class_accessor_define: decl!("__torajs_class_accessor_define", [I64, Str, I64, I64], Void),
        class_static_accessor_define: decl!(
            "__torajs_class_static_accessor_define",
            [I64, Str, I64, I64],
            Void
        ),
        class_get: decl!("__torajs_anyv_class_get", [I64], Any),
        get_proto_of_any: decl!("__torajs_anyv_get_proto_of_any", [Any], Any),
        proto_member_get: decl!("__torajs_anyv_proto_member_get", [Any], Any),
        error_to_string: decl!("__torajs_error_to_string", [Ptr], Str),
        error_tostring_dispatch: decl!("__torajs_error_tostring_dispatch", [Ptr], Str),
        ctor_no_super_throw: decl!("__torajs_ctor_no_super_throw", [], Void),
        throw_reference_error_name: decl!("__torajs_throw_reference_error_name", [Str], Void),
        genfn_proto: decl!("__torajs_genfn_proto", [I64], Any),
        genfn_chain: decl!("__torajs_genfn_chain", [Any, I64], I64),
        // RFC 20260704 S3 — recv[idx] on an `any` receiver (Arr
        // kind-aware / Str / primitive dispatch); S3-set = the
        // (tag, value) pair write mirror.
        any_index_get: decl!("__torajs_any_index_get", [Any, I64], Any),
        any_index_get_keyed: decl!("__torajs_any_index_get_keyed", [Any, Any], Any),
        anyv_to_property_key: decl!("__torajs_anyv_to_property_key", [Any], Ptr),
        anyv_property_key_drop: decl!("__torajs_anyv_property_key_drop", [Ptr], Void),
        any_index_set_keyed: decl!(
            "__torajs_any_index_set_keyed",
            [Any, Any, I64, I64, Ptr],
            Void
        ),
        any_index_set: decl!("__torajs_any_index_set", [Any, I64, I64, I64, Ptr], Void),
        // RFC 20260704 S4 / chunk 716 / C4-2 — recv.length / .name /
        // .size runtime dispatches (dynobj probe included).
        any_length_get: decl!("__torajs_any_length_get", [Any], Any),
        any_name_get: decl!("__torajs_any_name_get", [Any], Any),
        any_size_get: decl!("__torajs_any_size_get", [Any], Any),
        // RFC 20260704 C4-3c-2 — RegExp accessor surface (source /
        // flags / lastIndex / flag booleans) + dynobj probe.
        any_regexp_prop: decl!("__torajs_any_regexp_prop", [Any, I64, Ptr], Any),
        // RFC 20260704 C4+ — tag-gated member read pair: DynObj
        // own-property probe (accessor sentinel included), Arr
        // expando probe, definite (ANY_UNDEF, 0) for every other
        // receiver; null/undefined receivers record a catchable
        // TypeError on the tag call.
        any_member_get_tag: decl!("__torajs_any_member_get_tag", [Any, Ptr], I64),
        any_member_get_value: decl!("__torajs_any_member_get_value", [Any, Ptr], I64),
        // RFC 20260714-objlit-accessor blade 5 — the single [[Get]]
        // behind an accessor member read. A non-zero value channel is
        // a dynobj AccessorPair; a ZERO one is a struct accessor,
        // invoked WITH the receiver so its `this` is bound.
        any_accessor_get: decl!("__torajs_any_accessor_get", [Any, Ptr, I64], Any),
        // RFC 20260704 C4+ — tag-gated member write (recv AnyValue
        // slot, key Str, payload (tag, value) pair, name hint):
        // DynObj set with relocation write-back, RegExp lastIndex,
        // Arr expando; everything else a catchable TypeError.
        any_member_set: decl!("__torajs_any_member_set", [Ptr, Ptr, I64, I64, I64], Void),
        // RFC 20260704 S5+ — unified for-of iteration protocol
        // (indexed strings/arrays + stateful MapIter/ArrIter cells +
        // class instances stepped through `[Symbol.iterator]()` /
        // `next()`; throws on non-iterables). (recv, idx-cursor slot,
        // caller-owned iterator park slot, owned-AnyValue out slot)
        // → live flag.
        any_iter_next: decl!("__torajs_any_iter_next", [Any, Ptr, Ptr, Ptr], I64),
        any_iter_next_await: decl!("__torajs_any_iter_next_await", [Any, Ptr, Ptr, Ptr], I64),
        any_iter_next_array_like: decl!(
            "__torajs_any_iter_next_array_like",
            [Any, Ptr, Ptr, Ptr],
            I64
        ),
        // ES §7.4.9 IteratorClose — owed to an iterator a consumer
        // stops stepping before it reports done (a destructuring
        // pattern shorter than its source). Runs a generator's
        // `finally`; a no-op for the lanes whose iterators have no
        // `return` method. (recv, iterator park slot) → void.
        any_iter_close: decl!("__torajs_any_iter_close", [Any, Ptr], Void),
        iter_close_value: decl!("__torajs_iter_close_value", [Any], Void),
        // RFC C4+ — bare any-call `f(args…)`: (callee, argv, argc)
        // → Any; non-closures raise a catchable TypeError.
        any_call: decl!("__torajs_any_call", [Any, Ptr, I64], Any),
        accessor_face_from_any: decl!("__torajs_accessor_face_from_any", [Any], Ptr),
        // RFC 20260708-variadic — closure-slot variadic call through
        // a `(...args: E[]) => R`-typed binding: (env cell, argv,
        // argc) → Any via the boxed dual entry; a missing adapter
        // raises the same catchable TypeError.
        closure_call_variadic: decl!("__torajs_closure_call_variadic", [Ptr, Ptr, I64], Any),
        // RFC 20260704 C1 — recv.name(args…) runtime dispatch:
        // (recv, method-id, name ptr/len for TypeError messages,
        // receiver write-back slot, argv, argc) → Any. Chunk 709
        // added the `o.m?.(…)` optional flavor (no-such method
        // answers undefined) and the GetV-existence probe deciding
        // whether the optional call's args evaluate.
        any_method_call: decl!(
            "__torajs_any_method_call",
            [Any, I64, Ptr, I64, Ptr, Ptr, I64],
            Any
        ),
        any_method_call_opt: decl!(
            "__torajs_any_method_call_opt",
            [Any, I64, Ptr, Ptr, Ptr, I64],
            Any
        ),
        // §13.3.6.2 `recv[key](args…)` — runtime ToPropertyKey then
        // the by-name/symbol method dispatch with recv as thisValue
        // (RFC 20260728-gen-forof-yieldstar F0b).
        any_index_method_call: decl!(
            "__torajs_any_index_method_call",
            [Any, Any, Ptr, Ptr, I64],
            Any
        ),
        any_method_probe: decl!("__torajs_any_method_probe", [Any, I64, Ptr], I64),
        any_prop_delete: decl!("__torajs_any_prop_delete", [Any, Ptr], I64),
        any_prop_has: decl!("__torajs_any_prop_has", [Any, Ptr], I64),
        any_has_property: decl!("__torajs_any_has_property", [Any, Ptr], I64),
        arr_forin_key_live: decl!("__torajs_arr_forin_key_live", [Ptr, Ptr], I64),
        // RFC 20260711-closure-reflection chunk A — static
        // `<Ctor>.prototype.<m>` read (builtin-proto tag + key Str).
        builtin_proto_method_value: decl!("__torajs_builtin_proto_method_value", [I64, Ptr], Any),
        // RFC 20260725-str-method-value-reify — (family, mid) →
        // interned cell ptr (immortal; rc traffic no-ops).
        builtin_method_cell_tagged: decl!("__torajs_builtin_method_cell_tagged", [I64, I64], Ptr),
        any_unbox_tag: decl!("__torajs_anyv_unbox_tag", [Any], I64),
        any_unbox_value: decl!("__torajs_anyv_unbox_value", [Any], I64),
        // chunk 712 — borrow-shaped cell-pointer read: heap cell →
        // pointer bits, every immediate (ShortStr included) → 0
        // (the materializing unbox_value leaked an owned Str per
        // read on a ShortStr receiver).
        any_cell_ptr: decl!("__torajs_anyv_cell_ptr", [Any], I64),
        // Owned-pair unbox: heap cell → rc_inc + ptr, ShortStr →
        // materialized rc=1 Str (the materialization IS the
        // caller's stake — no separate payload_rc_inc).
        any_unbox_value_owned: decl!("__torajs_anyv_unbox_value_owned", [Any], I64),
        // Reclaims the materialized temp a ShortStr unbox_value
        // left behind — emitted after every borrow-shaped pair
        // consumer (args: original AnyValue, raw unboxed value).
        any_unbox_settle: decl!("__torajs_anyv_unbox_settle", [Any, I64], Void),
        any_box_drop: decl!("__torajs_anyv_rc_dec", [Any], Void),
        any_box_rc_inc: decl!("__torajs_anyv_rc_inc", [Any], Void),
        ctor_register: decl!("__torajs_anyv_ctor_register", [Any, Ptr], Void),
        construct: decl!("__torajs_anyv_construct", [Any, Ptr, I64], Any),
        is_constructor: decl!("__torajs_is_constructor", [Any], Bool),
    }
}
