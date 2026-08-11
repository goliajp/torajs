//! Cross-staticlib kernel externs of the ns-static dispatch family
//! — split from `ns_static_table.rs` when the §19.2.6 UriKernel
//! rows pushed it over the 500-line cap (mechanical move; the
//! table re-exports the whole surface so sibling paths through
//! `super::ns_static_table::` stay unchanged).
//!
//! MAINTENANCE: every extern added here needs a matching no-op stub
//! in `lib.rs`'s `#[cfg(test)] mod tests` — the dispatch table is
//! test-reachable, so `-dead_strip` keeps this module and the test
//! binary link fails on any unstubbed symbol.

use core::ffi::c_void;

unsafe extern "C" {
    pub(super) fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — 1 when a pending throw is recorded (a poisoned
    /// valueOf during ToNumber aborts the remaining coercions).
    pub(super) fn __torajs_throw_check() -> i64;
    pub(super) fn __torajs_math_sqrt(x: f64) -> f64;
    pub(super) fn __torajs_math_abs(x: f64) -> f64;
    pub(super) fn __torajs_math_floor(x: f64) -> f64;
    pub(super) fn __torajs_math_ceil(x: f64) -> f64;
    pub(super) fn __torajs_math_log(x: f64) -> f64;
    pub(super) fn __torajs_math_exp(x: f64) -> f64;
    pub(super) fn __torajs_math_sign(x: f64) -> f64;
    pub(super) fn __torajs_math_round(x: f64) -> f64;
    pub(super) fn __torajs_math_trunc(x: f64) -> f64;
    pub(super) fn __torajs_math_sin(x: f64) -> f64;
    pub(super) fn __torajs_math_cos(x: f64) -> f64;
    pub(super) fn __torajs_math_tan(x: f64) -> f64;
    pub(super) fn __torajs_math_asin(x: f64) -> f64;
    pub(super) fn __torajs_math_acos(x: f64) -> f64;
    pub(super) fn __torajs_math_atan(x: f64) -> f64;
    pub(super) fn __torajs_math_log2(x: f64) -> f64;
    pub(super) fn __torajs_math_log10(x: f64) -> f64;
    pub(super) fn __torajs_math_cbrt(x: f64) -> f64;
    pub(super) fn __torajs_math_sinh(x: f64) -> f64;
    pub(super) fn __torajs_math_cosh(x: f64) -> f64;
    pub(super) fn __torajs_math_tanh(x: f64) -> f64;
    pub(super) fn __torajs_math_asinh(x: f64) -> f64;
    pub(super) fn __torajs_math_acosh(x: f64) -> f64;
    pub(super) fn __torajs_math_atanh(x: f64) -> f64;
    pub(super) fn __torajs_math_expm1(x: f64) -> f64;
    pub(super) fn __torajs_math_log1p(x: f64) -> f64;
    pub(super) fn __torajs_math_fround(x: f64) -> f64;
    pub(super) fn __torajs_math_f16round(x: f64) -> f64;
    pub(super) fn __torajs_math_pow(x: f64, y: f64) -> f64;
    pub(super) fn __torajs_math_min(x: f64, y: f64) -> f64;
    pub(super) fn __torajs_math_max(x: f64, y: f64) -> f64;
    pub(super) fn __torajs_math_atan2(y: f64, x: f64) -> f64;
    pub(super) fn __torajs_math_imul(a: i64, b: i64) -> i64;
    pub(super) fn __torajs_math_clz32(x: i64) -> i64;
    pub(super) fn __torajs_math_random() -> f64;
    /// torajs-num — the typed tier's §19.2.5/.4 parse kernels
    /// (Str cell in, auto-detect radix on 0).
    pub(super) fn __torajs_num_parse_int(s: *const u8, radix: i64) -> f64;
    pub(super) fn __torajs_num_parse_float(s: *const u8) -> f64;
    /// torajs-str — release the owned coercion temp.
    pub(super) fn __torajs_str_drop(s: *mut c_void);
    /// torajs-meta — §20.1.2.17/.23/.5 own-enumeration. Each answers
    /// a FRESH Arr cell (rc 1); `include_nonenum` picks the
    /// `getOwnPropertyNames` surface over the `keys` one.
    ///
    /// `own_keys` is the one of the three that hands back an
    /// UNSTAMPED block: `own_values` allocates a real `Array<Any>`
    /// (`FLAG_ARR_ANY`) and `own_entries` stamps its outer array
    /// itself, but the keys walk just pushes raw Str pointers. The
    /// typed tier never noticed — its static `Arr<Str>` result type
    /// drives the element drops — so the arm has to stamp the kind
    /// at the typed→Any boundary or every heap key leaks.
    pub(super) fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void;
    /// torajs-arr — stamp the element-kind field on a typed array
    /// crossing into the any world (the `exec` / `split` precedent).
    pub(super) fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    pub(super) fn __torajs_anyv_own_values(v: u64) -> *mut c_void;
    pub(super) fn __torajs_anyv_own_entries(v: u64) -> *mut c_void;
    /// torajs-meta — §20.1.2.1 single-source copy. Guards a
    /// null/undefined TARGET itself (the arm leans on that instead
    /// of re-deriving step 1); a null/undefined SOURCE is a no-op.
    pub(super) fn __torajs_anyv_assign(target: u64, source: u64);
    /// torajs-meta — §20.1.2.6. Returns the receiver bit pattern
    /// UNCHANGED and does NOT rc_inc: a borrow, so the arm owns it
    /// before handing it back.
    pub(super) fn __torajs_anyv_freeze(obj_any: u64) -> u64;
    /// torajs-rc — §20.1.2.13 NaN-box-aware probe (non-object reads
    /// `true` by definition). Answers a plain bool: no ownership.
    pub(super) fn __torajs_obj_is_frozen_any(v: i64) -> bool;
    /// torajs-meta — §20.1.2.12. Already OWNED on return (the
    /// builtin-prototype singletons and the dynobj slot read both
    /// rc_inc before answering), so the arm must NOT inc again.
    pub(super) fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
    /// torajs-meta — §20.1.2.21. Void; the static answers its
    /// receiver, so the arm owns the borrow before handing it back.
    pub(super) fn __torajs_anyv_set_prototype_of(obj: u64, proto: u64);
    /// torajs-meta — §20.1.2.7. Answers a FRESH dynobj (owned); the
    /// reject paths answer an immediate, so nothing leaks there.
    pub(super) fn __torajs_anyv_from_entries(entries: u64) -> u64;
    /// torajs-str — §20.4.2.2 registry lookup-or-create. SHARES the
    /// key on a miss (`symbol_alloc` incs the desc itself), so the
    /// arm still drops its minted coercion temp; the returned Symbol
    /// is rc'd for the caller (owned).
    pub(super) fn __torajs_symbol_for(key: *mut c_void) -> *mut c_void;
    /// torajs-str — §20.4.2.6. Answers the registered key Str (rc'd,
    /// owned) or NULL for an unregistered symbol — NULL maps to
    /// undefined, never to a raw null Str slot.
    pub(super) fn __torajs_symbol_key_for(sym: *mut c_void) -> *mut c_void;
    /// torajs-date — §21.4.3.1 Date.now (ms since epoch, no alloc).
    pub(super) fn __torajs_date_now_static() -> i64;
    /// torajs-date — §21.4.3.2 Date.parse (ISO 8601 → ms; NaN on
    /// parse failure). `s` is a live Str cell.
    pub(super) fn __torajs_date_parse_iso(s: *const c_void) -> f64;
    /// torajs-date — §21.4.3.4 Date.UTC MakeTime over 7 components
    /// (TimeClip'd; NaN when any component is NaN / out of range).
    pub(super) fn __torajs_date_utc_components(
        year: f64,
        month: f64,
        day: f64,
        hour: f64,
        minute: f64,
        second: f64,
        milli: f64,
    ) -> f64;
    /// torajs-str — §22.1.2.1 one-code-unit mint (truncates
    /// `n & 0xFFFF` itself, never throws).
    pub(super) fn __torajs_str_from_char_code(n: i64) -> *mut u8;
    /// torajs-str — §22.1.2.2 one-code-point mint; out-of-range
    /// records a catchable RangeError and answers an empty sentinel.
    pub(super) fn __torajs_str_from_code_point(n: i64) -> *mut u8;
    /// torajs-meta — §20.1.2.16/.13/.20/.15 integrity family (RFC
    /// C5b). The setters answer the receiver as a BORROW (the arm
    /// owns it before handing it back); the readers answer plain
    /// bools.
    pub(super) fn __torajs_anyv_prevent_extensions(obj_any: u64) -> u64;
    pub(super) fn __torajs_anyv_is_extensible(obj_any: u64) -> bool;
    pub(super) fn __torajs_anyv_seal(obj_any: u64) -> u64;
    pub(super) fn __torajs_anyv_is_sealed(obj_any: u64) -> bool;
    /// torajs-throw — catchable RangeError for the §7.1.22 ToIndex
    /// rejects in the BigInt.asN arm.
    pub(super) fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    /// torajs-bigint — §21.2.2.{1,2} fixed-width views (arbitrary
    /// bits, 刀 5a). Fresh owned BigInt out; bits < 0 and the
    /// asUintN negative-input size cap record a RangeError and
    /// answer a `0n` sentinel.
    pub(super) fn __torajs_bigint_as_int_n(bits: i64, value: *const c_void) -> *mut u8;
    pub(super) fn __torajs_bigint_as_uint_n(bits: i64, value: *const c_void) -> *mut u8;
    /// torajs-bigint — release an owned BigInt stake (coercion temp
    /// / kernel result on the throw-unwind path).
    pub(super) fn __torajs_bigint_drop_rc(p: *mut c_void);
    /// torajs-meta — §20.1.2.10 W-N-c truth: tr has no symbol-keyed
    /// props, so the kernel answers a FRESH empty `Arr<Str>` (owned)
    /// for every object; a nullish receiver records its ToObject
    /// TypeError and still answers the well-formed empty Arr.
    pub(super) fn __torajs_anyv_own_symbols(obj_any: u64) -> *mut c_void;
}
