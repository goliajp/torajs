//! §24.2.1.2 GetSetRecord — the set-like argument protocol behind
//! the ES2025 Set methods (§24.2.4). A Set-method argument is not
//! required to be a Set: anything carrying a numeric `size`, a
//! callable `has`, and a callable `keys` qualifies (a Map, a user
//! class with a `get size()` accessor, an object literal), and the
//! methods interact with it ONLY through those three faces — the
//! observable calls the test262 set-like suites pin.
//!
//! This module builds and releases the record; the seven walks that
//! consume it live in [`crate::set_like_ops`]. The Set×Set fast
//! kernels in torajs-collections `query.rs` stay untouched — the
//! dispatch arms route a real-Set argument there and everything
//! else here.
//!
//! Protocol notes (each is a recorded test262 face):
//! - step 1: a primitive argument refuses (TypeError) before any
//!   property read;
//! - steps 2-7: `size` is Get + ToNumber + ToIntegerOrInfinity —
//!   NaN refuses (TypeError; an Array argument lands here because
//!   its `size` is absent → undefined → NaN), a negative refuses
//!   (RangeError), `-0` and `+Infinity` are legal;
//! - steps 8-11: `has` / `keys` are Get + IsCallable — `undefined`
//!   refuses too (this is NOT GetMethod's nullish amnesty).
//!
//! Ownership: the record holds one owned stake on each method value
//! (the [`CallTarget`]s borrow through them) — [`release_record`]
//! returns both. `other` rides borrowed; the caller's argv stake
//! outlives the walk.

use core::ffi::c_void;

use crate::iter_zip_shared::{av_is_object, member_pair_cell};
use crate::method_call_closure::{CallTarget, call_target, dispatch};
use crate::nanbox::{AnyValue, as_double, as_void_ptr, box_int32, is_cell, is_double};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_bool, __torajs_anyv_to_number};
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-collections — live entry count (the builtin `size`).
    fn __torajs_map_size(p: *const c_void) -> i64;
    /// torajs-str — fresh rc-1 Str cell / its release.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — pending-throw records + the in-flight probe.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
}

/// §24.2.1.2's Set Record — see module doc for the field contracts.
pub(crate) struct SetRecord {
    /// The set-like argument (borrowed — the caller's stake).
    pub(crate) other: AnyValue,
    /// [[Size]] — ToIntegerOrInfinity(size); `≥ 0`, may be `+inf`.
    pub(crate) size: f64,
    has_v: AnyValue,
    pub(crate) has: CallTarget,
    keys_v: AnyValue,
    pub(crate) keys: CallTarget,
}

/// Owned `Get(obj, name)` — the member pair probe with the accessor
/// sentinel resolved (a struct `get size()` / dynobj accessor runs
/// its getter; §9.1.8 observable). A pending throw answers
/// `undefined` with the throw left for the caller's check.
unsafe fn get_prop(obj: AnyValue, name: &[u8]) -> AnyValue {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64) as *const c_void;
        let (t, v) = member_pair_cell(obj, key);
        let out = if t == crate::struct_probe::ANY_ACCESSOR_TAG {
            crate::struct_probe::__torajs_any_accessor_get(obj, key, v)
        } else {
            crate::payload_rc_inc(t as i64, v as i64);
            __torajs_anyv_box_from_pair(t as i64, v as i64)
        };
        __torajs_str_drop(key as *mut c_void);
        out
    }
}

/// Get + IsCallable (§24.2.1.2 steps 8-11) — an absent / undefined
/// property is a refusal here, unlike GetMethod. Answers the owned
/// method value plus its classified call target.
unsafe fn get_callable(
    obj: AnyValue,
    name: &[u8],
    err: &core::ffi::CStr,
) -> Option<(AnyValue, CallTarget)> {
    unsafe {
        let v = get_prop(obj, name);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(v);
            return None;
        }
        let target = if is_cell(v) {
            call_target(as_void_ptr(v))
        } else {
            None
        };
        match target {
            Some(t) => Some((v, t)),
            None => {
                __torajs_anyv_rc_dec(v);
                __torajs_throw_type_error(err.as_ptr());
                None
            }
        }
    }
}

/// §24.2.1.2 GetSetRecord. `None` = a pending throw is recorded.
pub(crate) unsafe fn get_set_record(other: AnyValue) -> Option<SetRecord> {
    unsafe {
        if !av_is_object(other) {
            __torajs_throw_type_error(c"Set method argument must be a set-like object".as_ptr());
            return None;
        }
        // Steps 2-7 — size. A Map / Set argument reads its builtin
        // entry count directly: the member pair probe only reifies
        // METHODS off builtin receivers, so `size` (an accessor)
        // would answer absent there.
        let cell = as_void_ptr(other);
        let cell_tag = (cell.cast::<u8>().add(4) as *const u16).read();
        let num = if cell_tag == Tag::Map as u16 || cell_tag == Tag::Set as u16 {
            __torajs_map_size(cell) as f64
        } else {
            let raw = get_prop(other, b"size");
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(raw);
                return None;
            }
            let n = __torajs_anyv_to_number(raw);
            __torajs_anyv_rc_dec(raw);
            if __torajs_throw_check() != 0 {
                return None;
            }
            n
        };
        if num.is_nan() {
            __torajs_throw_type_error(c"set-like object size is not a number".as_ptr());
            return None;
        }
        if num < 0.0 {
            __torajs_throw_range_error(c"set-like object size is negative".as_ptr());
            return None;
        }
        let size = num.trunc();
        let (has_v, has) = get_callable(other, b"has", c"set-like object has is not callable")?;
        let Some((keys_v, keys)) =
            get_callable(other, b"keys", c"set-like object keys is not callable")
        else {
            __torajs_anyv_rc_dec(has_v);
            return None;
        };
        Some(SetRecord {
            other,
            size,
            has_v,
            has,
            keys_v,
            keys,
        })
    }
}

/// Return the record's two owned method stakes.
pub(crate) unsafe fn release_record(rec: SetRecord) {
    unsafe {
        __torajs_anyv_rc_dec(rec.has_v);
        __torajs_anyv_rc_dec(rec.keys_v);
    }
}

/// `ToBoolean(? Call(rec.[[Has]], rec.[[SetObject]], «e»))` — the
/// element rides borrowed (the caller holds a frame stake across the
/// call; the callee can delete the entry out of the live Set).
/// `None` = the call threw.
pub(crate) unsafe fn call_has(rec: &SetRecord, e_box: AnyValue) -> Option<bool> {
    unsafe {
        let argv = [e_box];
        let r = dispatch(&rec.has, rec.other, argv.as_ptr(), 1);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(r);
            return None;
        }
        let b = __torajs_anyv_to_bool(r);
        __torajs_anyv_rc_dec(r);
        Some(b)
    }
}

/// `? Call(rec.[[Keys]], rec.[[SetObject]])` + the
/// GetIteratorFromMethod object gate (§7.4.3 step 2). Answers the
/// OWNED iterator; `None` = a pending throw.
pub(crate) unsafe fn keys_iterator(rec: &SetRecord) -> Option<AnyValue> {
    unsafe {
        let argv: [u64; 0] = [];
        let it = dispatch(&rec.keys, rec.other, argv.as_ptr(), 0);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(it);
            return None;
        }
        if !is_cell(it) {
            __torajs_anyv_rc_dec(it);
            __torajs_throw_type_error(c"set-like object keys() did not return an object".as_ptr());
            return None;
        }
        Some(it)
    }
}

/// §24.2.1.4 CanonicalizeKeyedCollectionKey — `-0` folds to `+0`
/// before a keys-iterator element is stored or probed. Immediates
/// carry no refcount, so the swap is stake-neutral.
pub(crate) fn canonical(v: AnyValue) -> AnyValue {
    if is_double(v) && as_double(v) == 0.0 {
        return box_int32(0);
    }
    v
}
