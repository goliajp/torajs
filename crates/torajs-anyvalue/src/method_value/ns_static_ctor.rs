//! Ctor-static dispatch arms (RFC 20260720-ctor-static-reflection
//! 刀 1) — the Date / String / Object.hasOwn family appended behind
//! the RFC-20260719 namespaces. Split from `ns_static.rs` under the
//! 500-line file rule; the parent's `dispatch` match delegates here.

use core::ffi::c_void;

use crate::nanbox::{VALUE_UNDEFINED, box_double, box_void_ptr, is_null, is_undefined};

use super::ns_static::{arg_at, arg_num};
use super::ns_static_table::{
    __torajs_bigint_as_int_n, __torajs_bigint_as_uint_n, __torajs_bigint_drop_rc,
    __torajs_date_parse_iso, __torajs_date_utc_components, __torajs_str_from_char_code,
    __torajs_str_from_code_point, __torajs_throw_check, __torajs_throw_range_error,
    __torajs_throw_type_error,
};

/// §21.4.3.2 — ToString(arg0) into the ISO parse kernel (NaN on
/// failure). The coercion temp stays ours to release.
pub(super) unsafe fn date_parse(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let n = __torajs_date_parse_iso(s as *const c_void);
        crate::__torajs_str_drop(s as *mut c_void);
        box_double(n)
    }
}

/// §21.4.3.4 — ToNumber every PRESENT component in source order;
/// absent trailing args take the spec defaults (year NaN — MakeTime
/// propagates it to a NaN time value — month 0, day 1, rest 0).
pub(super) unsafe fn date_utc(argv: *const u64, argc: i64) -> u64 {
    let mut c: [f64; 7] = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for (i, slot) in c.iter_mut().enumerate() {
        if (i as i64) < argc {
            let Ok(x) = (unsafe { arg_num(argv, argc, i as i64) }) else {
                return VALUE_UNDEFINED;
            };
            *slot = x;
        }
    }
    box_double(unsafe { __torajs_date_utc_components(c[0], c[1], c[2], c[3], c[4], c[5], c[6]) })
}

/// §22.1.2.1/.2 — per-code mint + pairwise concat, the same chain
/// the typed lowering builds (`ssa_lower_call_string_from_char_code`).
/// fromCharCode's kernel truncates ToUint16 itself (never throws);
/// fromCodePoint rejects non-integral codes here (the kernel's i64
/// ABI can't see the fraction) and range-errors inside the kernel —
/// checked per arg before the concat consumes the piece.
pub(super) unsafe fn str_from_codes(code_point: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let release = |p: *mut u8| {
            if !p.is_null() {
                crate::__torajs_str_drop(p as *mut c_void);
            }
        };
        let mut acc: *mut u8 = core::ptr::null_mut();
        for i in 0..argc {
            let Ok(x) = arg_num(argv, argc, i) else {
                release(acc);
                return VALUE_UNDEFINED;
            };
            let piece = if code_point {
                // Non-integral / non-finite → the kernel's OOR
                // RangeError (spec step 5.b) via the -1 sentinel.
                let n = if x.is_finite() && x.trunc() == x {
                    x as i64
                } else {
                    -1
                };
                let p = __torajs_str_from_code_point(n);
                if __torajs_throw_check() != 0 {
                    release(p);
                    release(acc);
                    return VALUE_UNDEFINED;
                }
                p
            } else {
                __torajs_str_from_char_code(super::ns_static::to_i64_mod32(x))
            };
            acc = if acc.is_null() {
                piece
            } else {
                let joined =
                    crate::__torajs_str_concat(acc as *const u8, piece as *const u8) as *mut u8;
                release(acc);
                release(piece);
                joined
            };
        }
        if acc.is_null() {
            acc = crate::__torajs_str_alloc_pooled(0);
        }
        acc as u64
    }
}

/// The ns-static namespace name a builtin proto tag keys —
/// `torajs-rc builtin_proto` order; families with no table statics
/// answer through `ns_static_id`'s natural miss.
fn ctor_ns_name(proto_tag: i64) -> &'static str {
    match proto_tag {
        0 => "Number",
        1 => "Object",
        2 => "Array",
        3 => "String",
        4 => "Boolean",
        5 => "Symbol",
        6 => "BigInt",
        7 => "RegExp",
        8 => "Date",
        9 => "Error",
        10 => "Promise",
        11 => "Map",
        12 => "Set",
        13 => "Function",
        _ => "",
    }
}

/// gOPD's ctor-static probe (RFC 20260720 刀 2) — when `cell` is an
/// interned builtin ctor cell and `key` names one of that ctor's
/// table statics, answers the interned ns-static value cell (the
/// SAME identity a value read mints, so
/// `gOPD(Date, "parse").value === Date.parse` holds). NULL on every
/// miss; torajs-meta's Closure descriptor arm consumes it.
///
/// # Safety
/// `cell` is null or a live `Tag::Closure` cell; `key` is null or a
/// live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_static_value_cell(
    cell: *const c_void,
    key: *const c_void,
) -> *mut u8 {
    let Some(tag) = super::ctor::ctor_tag_of_cell(cell) else {
        return core::ptr::null_mut();
    };
    if key.is_null() {
        return core::ptr::null_mut();
    }
    let name = unsafe {
        let len = (key.cast::<u8>().add(super::STR_LEN_OFF) as *const u32).read() as usize;
        let bytes = core::slice::from_raw_parts(key.cast::<u8>().add(super::STR_DATA_OFF), len);
        match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return core::ptr::null_mut(),
        }
    };
    let id = torajs_rc::ns_static_id(ctor_ns_name(tag), name);
    if id == torajs_rc::NS_STATIC_UNKNOWN {
        return core::ptr::null_mut();
    }
    super::ns_static::ns_static_cell(id)
}

/// §21.2.2.1/.2 BigInt.asIntN / asUintN — ToIndex(bits) per §7.1.22
/// (NaN → 0, negative / past 2^53-1 → RangeError, messages match
/// bun/JSC), ToBigInt(value) per §7.1.13, then the 刀-5a fixed-width
/// kernel. Every temp is owned here and released before boxing.
pub(super) unsafe fn bigint_as_n(signed: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let Ok(x) = arg_num(argv, argc, 0) else {
            return VALUE_UNDEFINED;
        };
        let x = if x.is_nan() { 0.0 } else { x.trunc() };
        if x < 0.0 {
            __torajs_throw_range_error(c"number of bits cannot be negative".as_ptr());
            return VALUE_UNDEFINED;
        }
        if x > 9007199254740991.0 {
            __torajs_throw_range_error(c"number of bits larger than (2 ** 53) - 1".as_ptr());
            return VALUE_UNDEFINED;
        }
        let bits = x as i64;
        let Some(b) = crate::to_bigint::any_to_bigint(arg_at(argv, argc, 1)) else {
            return VALUE_UNDEFINED;
        };
        let r = if signed {
            __torajs_bigint_as_int_n(bits, b as *const c_void)
        } else {
            __torajs_bigint_as_uint_n(bits, b as *const c_void)
        };
        __torajs_bigint_drop_rc(b as *mut c_void);
        if __torajs_throw_check() != 0 {
            __torajs_bigint_drop_rc(r as *mut c_void);
            return VALUE_UNDEFINED;
        }
        box_void_ptr(r as *mut c_void)
    }
}

/// §27.2.4.7/.6 Promise.resolve / reject (RFC 20260720 刀 6) — both
/// statics read |this| as the species constructor (step 1) and a
/// bare reified-cell call has an undefined this (the ns-static
/// dispatcher is receiver-less by design), so the call ALWAYS raises
/// the catchable TypeError bun/JSC does for `const r =
/// Promise.resolve; r(42)`. The cell still serves the reflection
/// surface (name / length / print / gOPD identity); a this-aware
/// cell entry is the recorded follow-up face (RFC 刀 6 note).
pub(super) unsafe fn promise_settle() -> u64 {
    unsafe { __torajs_throw_type_error(c"|this| is not an object".as_ptr()) };
    VALUE_UNDEFINED
}

/// §20.1.2.11 Object.hasOwn — step 1 ToObject throws on a nullish
/// target; every other receiver routes through the same prop_has
/// probe the `hasOwnProperty` dispatcher arm uses (single source).
pub(super) unsafe fn object_has_own(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let o = arg_at(argv, argc, 0);
        if is_undefined(o) || is_null(o) {
            __torajs_throw_type_error(c"Object.hasOwn called on null or undefined".as_ptr());
            return VALUE_UNDEFINED;
        }
        let (key_argv, key_argc) = if argc >= 2 {
            (argv.add(1), argc - 1)
        } else {
            (core::ptr::null(), 0)
        };
        crate::method_call_object_proto::own_prop_probe(
            o,
            torajs_rc::ANY_METHOD_HAS_OWN_PROPERTY,
            key_argv,
            key_argc,
        )
    }
}
