//! Receiver host-face probes for the ES generic array-like family —
//! `ToLength(Get(O, "length"))` / per-index `Get` / `HasProperty`
//! over whatever cell hosts the scan (dynobj / anon struct /
//! primitive-wrapper expando). Split out of
//! [`crate::method_call_arraylike`] at 刀 9 when the wrapper arm
//! pushed the shared file over the 500-line limit; bodies moved
//! verbatim.

use core::ffi::c_void;

use crate::index_any::__torajs_any_index_get;
use crate::nanbox::AnyValue;
use crate::nanbox_encode::{__torajs_anyv_box_from_pair, __torajs_anyv_box_pointer};
use crate::nanbox_ffi::__torajs_anyv_to_number;

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of `method_call_dynobj.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

unsafe extern "C" {
    /// torajs-str — fresh Str from raw bytes (probe keys).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — property probe by Str key.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — run an accessor entry's getter.
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-throw — pending-throw flag.
    fn __torajs_throw_check() -> i64;
    /// Universal NaN-box-safe heap dropper.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `ToLength(Get(O, "length"))` — the accessor getter runs
/// (observable per §23.1.3 step 2 tests); `None` when the getter
/// left a pending throw. Object lengths answer NaN → 0 (recorded
/// no-valueOf boundary).
pub(crate) unsafe fn arraylike_len(obj: *mut c_void) -> Option<i64> {
    unsafe {
        let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
        // 刀 2 (RFC 20260714-t262-top-clusters) — a `Tag::Obj` anon
        // struct receiver (`{0: v, length: 2}` lowers static) reads
        // its `length` field through the class-layouts probe; absent
        // field answers the undefined pair (→ ToLength 0).
        let obj_tag = (obj.cast::<u8>().add(4) as *const u16).read();
        let (dtag, dval) = if obj_tag == torajs_rc::Tag::TypedArray as u16 {
            // §23.2.3.21 — a TypedArray receiver's `length` is the
            // prototype getter (0 for a detached / out-of-bounds
            // view, never a throw); the any member-get face carries
            // exactly that, own-expando shadow included.
            let recv = __torajs_anyv_box_pointer(obj);
            (
                crate::member_get::__torajs_any_member_get_tag(recv, key as *const c_void),
                crate::member_get_value::__torajs_any_member_get_value(recv, key as *const c_void),
            )
        } else if obj_tag == torajs_rc::Tag::Obj as u16 {
            crate::struct_probe::struct_field_pair(obj, key as *const c_void).unwrap_or((5, 0))
        } else if crate::member_get::is_wrapper_tag(obj_tag) {
            // 刀 9 G2c — a primitive-wrapper receiver's own `length`
            // lives in its lazy `+16` expando dynobj (`obj.length =
            // 2` on `new Boolean(false)`); NULL expando / absent key
            // answers the undefined pair (→ ToLength 0).
            let props = crate::member_get::wrapper_props(obj);
            if props.is_null() {
                (5, 0)
            } else {
                (
                    __torajs_dynobj_get_tag(props, key as *const c_void),
                    __torajs_dynobj_get_value(props, key as *const c_void),
                )
            }
        } else {
            (
                __torajs_dynobj_get_tag(obj, key as *const c_void),
                __torajs_dynobj_get_value(obj, key as *const c_void),
            )
        };
        __torajs_str_drop(key as *mut c_void);
        let av = if dtag == ANY_ACCESSOR_TAG {
            let got = __torajs_accessor_invoke_getter(
                dval as *const c_void,
                crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64),
            );
            if __torajs_throw_check() != 0 {
                __torajs_value_drop_heap(got as *mut c_void);
                return None;
            }
            got
        } else {
            // Borrow pair — ToNumber below only reads, no stake.
            __torajs_anyv_box_from_pair(dtag as i64, dval as i64)
        };
        let n = __torajs_anyv_to_number(av);
        if dtag == ANY_ACCESSOR_TAG {
            __torajs_value_drop_heap(av as *mut c_void);
        }
        // §7.1.20 ToLength runs ToNumber — an object length's valueOf
        // may throw (15.4.4.14-5-30: that abrupt completion must
        // precede the fromIndex ToInteger side effects).
        if __torajs_throw_check() != 0 {
            return None;
        }
        // §7.1.20 ToLength — NaN/negative clamp 0, cap 2^53-1.
        let len = if n.is_nan() || n <= 0.0 {
            0
        } else if n >= 9007199254740991.0 {
            9007199254740991
        } else {
            n as i64
        };
        Some(len)
    }
}

/// `Get(O, ToString(k))` — owned answer (accessor getters run).
pub(crate) unsafe fn arraylike_get(obj: *mut c_void, k: i64) -> AnyValue {
    unsafe { __torajs_any_index_get(__torajs_anyv_box_pointer(obj), k) }
}

/// `HasProperty(O, ToString(k))` — the hole gate for the has-gated
/// families (§23.1.3.17 step 9.a etc.).
pub(crate) unsafe fn arraylike_has(obj: *mut c_void, k: i64) -> bool {
    unsafe {
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        let mut m = k.max(0) as u64;
        loop {
            i -= 1;
            buf[i] = b'0' + (m % 10) as u8;
            m /= 10;
            if m == 0 {
                break;
            }
        }
        let key = __torajs_str_alloc(buf[i..].as_ptr(), (buf.len() - i) as i64);
        // §7.3.11 HasProperty — the chain-walking kernel (an
        // inherited index prop is present; RFC 20260721 G2d), not the
        // own-only probe.
        let hit = crate::prop_has::__torajs_any_has_property(
            __torajs_anyv_box_pointer(obj),
            key as *const c_void,
        );
        __torajs_str_drop(key as *mut c_void);
        hit != 0
    }
}
