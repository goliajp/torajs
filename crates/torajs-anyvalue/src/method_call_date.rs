//! `Tag::Date` arm of `__torajs_any_method_call`
//! (Any-method-call RFC 20260704 C4-3a) — `d.getTime()` /
//! `d.toISOString()` / `d.setFullYear(y)` where the Date crossed
//! into the `any` world.
//!
//! Mirrors the typed tier's method table
//! (`ssa_lower_call_date_methods.rs`) one id per name onto the
//! `__torajs_date_*` kernels (RFC 20260713-date-invalid-time f64
//! ABI):
//!
//! - **f64 getters** (getTime / valueOf alias / getFullYear / UTC
//!   variants / getDay / getTimezoneOffset / annexB getYear) —
//!   receiver-only call, boxed f64 result (NaN when the receiver is
//!   an invalid date).
//! - **string renderings** (toISOString / toJSON alias /
//!   toUTCString / toGMTString alias / toDateString / toString /
//!   toLocale*) — the kernel returns a fresh +1 Str; the box
//!   transfers that ownership out. An invalid receiver makes
//!   toISOString/toJSON record a pending RangeError which the
//!   any-call site's `emit_throw_check` propagates.
//! - **setters** — arguments decode through `anyv_to_number` (ms
//!   values stay exact: f64 holds integers to 2^53); per-field
//!   setters (setFullYear 3 / setMonth 2 / setDate 1 / setHours 4 /
//!   setMinutes 3 / setSeconds 2 / setMilliseconds 1) carry a
//!   trailing present-bitmask exactly like the typed lowering —
//!   a supplied-but-undefined arg is NaN + present (ToNumber
//!   semantics, invalidates the date), a missing trailing arg keeps
//!   the current component, and the mandatory first arg is always
//!   present (missing ≡ undefined ≡ NaN). The boxed f64 return is
//!   the new time value or NaN (ES §21.4.4.20-27).

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_GET_DATE, ANY_METHOD_GET_DAY, ANY_METHOD_GET_FULL_YEAR, ANY_METHOD_GET_HOURS,
    ANY_METHOD_GET_MILLISECONDS, ANY_METHOD_GET_MINUTES, ANY_METHOD_GET_MONTH,
    ANY_METHOD_GET_SECONDS, ANY_METHOD_GET_TIME, ANY_METHOD_GET_TIMEZONE_OFFSET,
    ANY_METHOD_GET_UTC_DATE, ANY_METHOD_GET_UTC_DAY, ANY_METHOD_GET_UTC_FULL_YEAR,
    ANY_METHOD_GET_UTC_HOURS, ANY_METHOD_GET_UTC_MILLISECONDS, ANY_METHOD_GET_UTC_MINUTES,
    ANY_METHOD_GET_UTC_MONTH, ANY_METHOD_GET_UTC_SECONDS, ANY_METHOD_GET_YEAR, ANY_METHOD_SET_DATE,
    ANY_METHOD_SET_FULL_YEAR, ANY_METHOD_SET_HOURS, ANY_METHOD_SET_MILLISECONDS,
    ANY_METHOD_SET_MINUTES, ANY_METHOD_SET_MONTH, ANY_METHOD_SET_SECONDS, ANY_METHOD_SET_TIME,
    ANY_METHOD_SET_UTC_DATE, ANY_METHOD_SET_UTC_FULL_YEAR, ANY_METHOD_SET_UTC_HOURS,
    ANY_METHOD_SET_UTC_MILLISECONDS, ANY_METHOD_SET_UTC_MINUTES, ANY_METHOD_SET_UTC_MONTH,
    ANY_METHOD_SET_UTC_SECONDS, ANY_METHOD_SET_YEAR, ANY_METHOD_TO_DATE_STRING,
    ANY_METHOD_TO_GMT_STRING, ANY_METHOD_TO_ISO_STRING, ANY_METHOD_TO_JSON,
    ANY_METHOD_TO_LOCALE_DATE_STRING, ANY_METHOD_TO_LOCALE_STRING,
    ANY_METHOD_TO_LOCALE_TIME_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_TO_TIME_STRING,
    ANY_METHOD_TO_UTC_STRING, ANY_METHOD_VALUE_OF, AnySlotTag,
};

use crate::method_call::method_no_such;
use crate::nanbox::AnyValue;
use crate::nanbox_encode::{__torajs_anyv_box_f64, __torajs_anyv_box_from_pair};
use crate::nanbox_ffi::__torajs_anyv_to_number;

unsafe extern "C" {
    fn __torajs_date_get_time(d: *const c_void) -> f64;
    fn __torajs_date_get_year(d: *const c_void) -> f64;
    fn __torajs_date_get_full_year(d: *const c_void) -> f64;
    fn __torajs_date_get_month(d: *const c_void) -> f64;
    fn __torajs_date_get_date(d: *const c_void) -> f64;
    fn __torajs_date_get_hours(d: *const c_void) -> f64;
    fn __torajs_date_get_minutes(d: *const c_void) -> f64;
    fn __torajs_date_get_seconds(d: *const c_void) -> f64;
    fn __torajs_date_get_milliseconds(d: *const c_void) -> f64;
    fn __torajs_date_get_day(d: *const c_void) -> f64;
    fn __torajs_date_get_timezone_offset(d: *const c_void) -> f64;
    fn __torajs_date_get_utc_full_year(d: *const c_void) -> f64;
    fn __torajs_date_get_utc_month(d: *const c_void) -> f64;
    fn __torajs_date_get_utc_date(d: *const c_void) -> f64;
    fn __torajs_date_get_utc_hours(d: *const c_void) -> f64;
    fn __torajs_date_get_utc_minutes(d: *const c_void) -> f64;
    fn __torajs_date_get_utc_seconds(d: *const c_void) -> f64;
    fn __torajs_date_get_utc_milliseconds(d: *const c_void) -> f64;
    fn __torajs_date_get_utc_day(d: *const c_void) -> f64;
    fn __torajs_date_to_iso_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_gmt_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_date_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_time_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_locale_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_locale_date_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_locale_time_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_set_time(d: *mut c_void, ms: f64) -> f64;
    fn __torajs_date_set_year(d: *mut c_void, y: f64) -> f64;
    fn __torajs_date_set_full_year(d: *mut c_void, y: f64, mo: f64, dd: f64, present: i64) -> f64;
    fn __torajs_date_set_month(d: *mut c_void, mo: f64, dd: f64, present: i64) -> f64;
    fn __torajs_date_set_date(d: *mut c_void, dd: f64, present: i64) -> f64;
    fn __torajs_date_set_hours(
        d: *mut c_void,
        h: f64,
        mi: f64,
        s: f64,
        ms: f64,
        present: i64,
    ) -> f64;
    fn __torajs_date_set_minutes(d: *mut c_void, mi: f64, s: f64, ms: f64, present: i64) -> f64;
    fn __torajs_date_set_seconds(d: *mut c_void, s: f64, ms: f64, present: i64) -> f64;
    fn __torajs_date_set_milliseconds(d: *mut c_void, ms: f64, present: i64) -> f64;
    fn __torajs_date_set_utc_full_year(
        d: *mut c_void,
        y: f64,
        mo: f64,
        dd: f64,
        present: i64,
    ) -> f64;
    fn __torajs_date_set_utc_month(d: *mut c_void, mo: f64, dd: f64, present: i64) -> f64;
    fn __torajs_date_set_utc_date(d: *mut c_void, dd: f64, present: i64) -> f64;
    fn __torajs_date_set_utc_hours(
        d: *mut c_void,
        h: f64,
        mi: f64,
        s: f64,
        ms: f64,
        present: i64,
    ) -> f64;
    fn __torajs_date_set_utc_minutes(d: *mut c_void, mi: f64, s: f64, ms: f64, present: i64)
    -> f64;
    fn __torajs_date_set_utc_seconds(d: *mut c_void, s: f64, ms: f64, present: i64) -> f64;
    fn __torajs_date_set_utc_milliseconds(d: *mut c_void, ms: f64, present: i64) -> f64;
}

/// Decoded per-field setter arguments: 4 f64 value slots + the
/// present bitmask (bit k = argv[k] supplied). The mandatory first
/// argument is always marked present — a missing first arg decodes
/// as undefined → NaN per ToNumber, invalidating the date.
struct Fields {
    v: [f64; 4],
    present: i64,
}

/// `None` = a `? ToNumber(arg)` step threw (a user valueOf/toString
/// recorded a pending throw): the setter must NOT run — §21.4.4.20
/// step 2 et al. abort BEFORE the kernel writes [[DateValue]], and
/// the t262 Date `-err` family asserts both faces (the throw
/// propagates AND getTime() is unchanged). The caller's throw check
/// fires on the pending record; the returned box is discarded.
fn decode_fields(argv: *const u64, argc: i64, arity: usize) -> Option<Fields> {
    let mut v = [0f64; 4];
    let mut present = 1i64; // first arg mandatory (missing ≡ undefined ≡ NaN)
    v[0] = f64::NAN;
    for (k, slot) in v.iter_mut().enumerate().take(arity) {
        if (k as i64) < argc {
            *slot = unsafe { __torajs_anyv_to_number(*argv.add(k)) };
            if unsafe { __torajs_throw_check() } != 0 {
                return None;
            }
            present |= 1 << k;
        }
    }
    Some(Fields { v, present })
}

/// `Tag::Date` arm — id-switch onto the torajs-date kernels (see
/// module doc).
pub(crate) unsafe fn date_method(
    d: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let fields = |arity: usize| decode_fields(argv, argc, arity);
    unsafe {
        let n = |x: f64| __torajs_anyv_box_f64(x);
        let s = |p: *mut u8| __torajs_anyv_box_from_pair(4, p as i64);
        match mid {
            m if m == ANY_METHOD_GET_TIME || m == ANY_METHOD_VALUE_OF => {
                n(__torajs_date_get_time(d))
            }
            m if m == ANY_METHOD_TO_ISO_STRING => s(__torajs_date_to_iso_string(d)),
            // §21.4.4.37 steps 2-3 — an invalid date answers null
            // (toISOString would record the RangeError; toJSON's
            // non-finite gate fires first).
            m if m == ANY_METHOD_TO_JSON => {
                if __torajs_date_get_time(d).is_finite() {
                    s(__torajs_date_to_iso_string(d))
                } else {
                    crate::nanbox::VALUE_NULL
                }
            }
            m if m == ANY_METHOD_GET_FULL_YEAR => n(__torajs_date_get_full_year(d)),
            m if m == ANY_METHOD_GET_UTC_FULL_YEAR => n(__torajs_date_get_utc_full_year(d)),
            m if m == ANY_METHOD_GET_MONTH => n(__torajs_date_get_month(d)),
            m if m == ANY_METHOD_GET_UTC_MONTH => n(__torajs_date_get_utc_month(d)),
            m if m == ANY_METHOD_GET_DATE => n(__torajs_date_get_date(d)),
            m if m == ANY_METHOD_GET_UTC_DATE => n(__torajs_date_get_utc_date(d)),
            m if m == ANY_METHOD_GET_HOURS => n(__torajs_date_get_hours(d)),
            m if m == ANY_METHOD_GET_UTC_HOURS => n(__torajs_date_get_utc_hours(d)),
            m if m == ANY_METHOD_GET_MINUTES => n(__torajs_date_get_minutes(d)),
            m if m == ANY_METHOD_GET_UTC_MINUTES => n(__torajs_date_get_utc_minutes(d)),
            m if m == ANY_METHOD_GET_SECONDS => n(__torajs_date_get_seconds(d)),
            m if m == ANY_METHOD_GET_UTC_SECONDS => n(__torajs_date_get_utc_seconds(d)),
            m if m == ANY_METHOD_GET_MILLISECONDS => n(__torajs_date_get_milliseconds(d)),
            m if m == ANY_METHOD_GET_UTC_MILLISECONDS => n(__torajs_date_get_utc_milliseconds(d)),
            m if m == ANY_METHOD_GET_DAY => n(__torajs_date_get_day(d)),
            m if m == ANY_METHOD_GET_UTC_DAY => n(__torajs_date_get_utc_day(d)),
            m if m == ANY_METHOD_GET_TIMEZONE_OFFSET => n(__torajs_date_get_timezone_offset(d)),
            m if m == ANY_METHOD_GET_YEAR => n(__torajs_date_get_year(d)),
            m if m == ANY_METHOD_TO_GMT_STRING || m == ANY_METHOD_TO_UTC_STRING => {
                s(__torajs_date_to_gmt_string(d))
            }
            m if m == ANY_METHOD_TO_DATE_STRING => s(__torajs_date_to_date_string(d)),
            m if m == ANY_METHOD_TO_TIME_STRING => s(__torajs_date_to_time_string(d)),
            m if m == ANY_METHOD_TO_STRING => s(__torajs_date_to_string(d)),
            m if m == ANY_METHOD_TO_LOCALE_STRING => s(__torajs_date_to_locale_string(d)),
            m if m == ANY_METHOD_TO_LOCALE_DATE_STRING => s(__torajs_date_to_locale_date_string(d)),
            m if m == ANY_METHOD_TO_LOCALE_TIME_STRING => s(__torajs_date_to_locale_time_string(d)),
            m if m == ANY_METHOD_SET_TIME => {
                let Some(f) = fields(1) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_time(d, f.v[0]))
            }
            m if m == ANY_METHOD_SET_YEAR => {
                let Some(f) = fields(1) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_year(d, f.v[0]))
            }
            m if m == ANY_METHOD_SET_FULL_YEAR => {
                let Some(f) = fields(3) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_full_year(
                    d, f.v[0], f.v[1], f.v[2], f.present,
                ))
            }
            m if m == ANY_METHOD_SET_MONTH => {
                let Some(f) = fields(2) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_month(d, f.v[0], f.v[1], f.present))
            }
            m if m == ANY_METHOD_SET_DATE => {
                let Some(f) = fields(1) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_date(d, f.v[0], f.present))
            }
            m if m == ANY_METHOD_SET_HOURS => {
                let Some(f) = fields(4) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_hours(
                    d, f.v[0], f.v[1], f.v[2], f.v[3], f.present,
                ))
            }
            m if m == ANY_METHOD_SET_MINUTES => {
                let Some(f) = fields(3) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_minutes(
                    d, f.v[0], f.v[1], f.v[2], f.present,
                ))
            }
            m if m == ANY_METHOD_SET_SECONDS => {
                let Some(f) = fields(2) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_seconds(d, f.v[0], f.v[1], f.present))
            }
            m if m == ANY_METHOD_SET_MILLISECONDS => {
                let Some(f) = fields(1) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_milliseconds(d, f.v[0], f.present))
            }
            m if m == ANY_METHOD_SET_UTC_FULL_YEAR => {
                let Some(f) = fields(3) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_utc_full_year(
                    d, f.v[0], f.v[1], f.v[2], f.present,
                ))
            }
            m if m == ANY_METHOD_SET_UTC_MONTH => {
                let Some(f) = fields(2) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_utc_month(d, f.v[0], f.v[1], f.present))
            }
            m if m == ANY_METHOD_SET_UTC_DATE => {
                let Some(f) = fields(1) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_utc_date(d, f.v[0], f.present))
            }
            m if m == ANY_METHOD_SET_UTC_HOURS => {
                let Some(f) = fields(4) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_utc_hours(
                    d, f.v[0], f.v[1], f.v[2], f.v[3], f.present,
                ))
            }
            m if m == ANY_METHOD_SET_UTC_MINUTES => {
                let Some(f) = fields(3) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_utc_minutes(
                    d, f.v[0], f.v[1], f.v[2], f.present,
                ))
            }
            m if m == ANY_METHOD_SET_UTC_SECONDS => {
                let Some(f) = fields(2) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_utc_seconds(d, f.v[0], f.v[1], f.present))
            }
            m if m == ANY_METHOD_SET_UTC_MILLISECONDS => {
                let Some(f) = fields(1) else {
                    return crate::nanbox::VALUE_UNDEFINED;
                };
                n(__torajs_date_set_utc_milliseconds(d, f.v[0], f.present))
            }
            _ => method_no_such(),
        }
    }
}

unsafe extern "C" {
    /// torajs-throw — pending-throw probe for the ToPrimitive leg
    /// of the generic toJSON body below.
    fn __torajs_throw_check() -> i64;
    /// torajs-dynobj — own-property probe for the Invoke(O,
    /// "toISOString") leg (per-module extern convention, mirrors
    /// `method_call_object_proto`'s pair).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
}

/// §21.4.4.37 Date.prototype.toJSON through a `.call`-re-dispatched
/// reified cell — receiver-generic: `tv = ToPrimitive(O, number)`;
/// a non-finite Number tv answers null WITHOUT touching
/// `toISOString` (the t262 non-finite family plants a throwing
/// getter there); otherwise Invoke(O, "toISOString") re-enters the
/// dispatcher by mid. Recorded edge: the re-entry passes no name
/// Str, so a non-Date receiver's OWN `toISOString` property is not
/// consulted — mid-routed builtin arms only (the t262 non-finite
/// cases never reach the invoke leg).
pub(crate) unsafe fn date_to_json_generic(recv: AnyValue) -> AnyValue {
    unsafe {
        let is_obj = crate::nanbox::is_cell(recv) && crate::to_primitive::is_object_value(recv);
        let (tv, owned) = if is_obj {
            match crate::to_primitive::heap_to_primitive(crate::nanbox::as_void_ptr(recv), false) {
                Some(v) => (v, true),
                // Both ToPrimitive methods answered objects — the
                // TypeError is already recorded.
                None => return crate::nanbox::VALUE_UNDEFINED,
            }
        } else {
            (recv, false)
        };
        if __torajs_throw_check() != 0 {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(tv);
            }
            return crate::nanbox::VALUE_UNDEFINED;
        }
        let non_finite = crate::nanbox::is_double(tv) && !crate::nanbox::as_double(tv).is_finite();
        if owned {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(tv);
        }
        if non_finite {
            return crate::nanbox::VALUE_NULL;
        }
        // §21.4.4.37 step 3 is Invoke(O, "toISOString") — an ordinary
        // Get-then-Call, so a plain object's OWN `toISOString` wins
        // over the builtin (t262 invoke-result hands `{ toISOString:
        // function () { … } }` to the reified cell). Same hand as
        // `arr_to_string_borrowed`'s `Get(this, "join")` leg; a
        // non-closure or missing entry falls through to the
        // redispatch, whose not-a-function TypeError is the same
        // answer §7.3.21 gives an uncallable Get result.
        if let Some((ptr, tag)) = crate::member_get::recv_cell(recv)
            && tag == torajs_rc::Tag::DynObj as u16
        {
            let key = {
                let bytes = b"toISOString";
                let s = crate::__torajs_str_alloc_pooled(bytes.len() as u64);
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), s.add(16), bytes.len());
                s
            };
            let jtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
            let jval = __torajs_dynobj_get_value(ptr, key as *const c_void);
            crate::__torajs_str_drop(key as *mut c_void);
            // Heap-tagged or nothing — see the twin in
            // `arr_to_string_borrowed`: any other tag's payload is a
            // value, not an address, and
            // `Date.prototype.toJSON.call({ toISOString: true })`
            // dereferenced it.
            if jtag == AnySlotTag::Heap as u64
                && let Some((env, entry)) =
                    crate::method_call::closure_cell_entry(jval as *mut c_void)
            {
                return crate::method_call::invoke_with_this(
                    env,
                    entry,
                    recv,
                    core::ptr::null(),
                    0,
                );
            }
        }
        crate::method_call::any_method_redispatch(
            recv,
            ANY_METHOD_TO_ISO_STRING,
            core::ptr::null(),
            0,
        )
    }
}
