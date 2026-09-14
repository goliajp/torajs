//! The `Tag::Obj` arm of `__torajs_any_index_get` — a class instance
//! or an inline object literal's anon struct read under an integer
//! key. Moved out of `index_any` verbatim at the 500-line cap
//! (rotation 646); the doc on the fn is the arm's contract.

use core::ffi::c_void;

use crate::index_any::{INDEX_ANY_ACCESSOR_TAG, i64_dec};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
}

/// `Tag::Obj` get arm — decimal-stringify the key and probe the class
/// layout through [`crate::member_get::struct_field_pair`]; the probe
/// is a borrow, the returned box owns its own reference (the
/// `dynobj_index_get` shape). A field miss probes the expando dict,
/// then the own named-accessor lane (`get 0() {…}` parses into a
/// `__getter_0` layout slot — the same spelling the named-getter
/// member lane resolves; ES §7.1.19 ToPropertyKey makes `o[0]` ≡
/// `o["0"]`), then the class prototype chain (rotation 441 — where
/// runtime-computed members reify), and a full miss answers
/// `undefined`.
pub(crate) unsafe fn struct_index_get(obj: *mut c_void, idx: i64) -> AnyValue {
    let mut buf = [0u8; 20];
    let (start, len) = i64_dec(&mut buf, idx);
    unsafe {
        let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
        let pair = crate::struct_probe::struct_field_pair(obj, key as *const c_void);
        let Some((ftag, fval)) = pair else {
            // Blade 2 (RFC 20260714-struct-dynamic-props) — a numeric
            // expando (`(c as any)[4] = v`) lives in the +24 dict
            // under its §7.1.19 decimal spelling; own face, so it
            // answers before the accessor fallback's undefined.
            let props = crate::member_get_layout::struct_props(obj);
            if !props.is_null() && __torajs_dynobj_has(props, key as *const c_void) != 0 {
                let etag = __torajs_dynobj_get_tag(props, key as *const c_void);
                let eval = __torajs_dynobj_get_value(props, key as *const c_void);
                __torajs_str_drop(key as *mut c_void);
                // An accessor expando (`Object.defineProperty(c, 0,
                // { get })`) invokes with the struct as receiver, the
                // `dynobj_index_entry` shape — the pair pointer boxed
                // as a value printed `null` (rotation 646).
                if etag == INDEX_ANY_ACCESSOR_TAG {
                    return __torajs_accessor_invoke_getter(
                        eval as *const c_void,
                        crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64),
                    );
                }
                crate::payload_rc_inc(etag as i64, eval as i64);
                return crate::nanbox_encode::__torajs_anyv_box_from_pair(etag as i64, eval as i64);
            }
            // Own named accessor (`get 0() {…}` parses into a
            // `__getter_0` layout slot) — presence-tested so a
            // genuine miss can keep walking instead of swallowing
            // the chain behind an unconditional undefined.
            if crate::struct_probe::struct_accessor_key(obj, key as *const c_void) {
                __torajs_str_drop(key as *mut c_void);
                return crate::struct_probe::__torajs_struct_accessor_get(
                    obj,
                    buf[start..].as_ptr(),
                    len as u32,
                );
            }
            // Rotation 441 — §10.1.8.1 step 3: the class prototype
            // chain under the decimal spelling. A runtime-computed
            // member (`get [1 + 1]() {…}`) reifies onto `__proto_<C>`
            // keyed "2"; the string/symbol lanes have walked that
            // chain since L3b ⑧ / RFC 20260802 刀 3a, so the
            // integer-key lane was the one read shape still blind to
            // it (`c[2]` answered undefined while `c["2"]` invoked
            // the getter). Accessor entries invoke with the STRUCT
            // as receiver; data pairs are borrows the box promotes.
            let (ptag, pval) = crate::struct_error_msg::struct_proto_chain_pair(
                obj as *const c_void,
                key as *const c_void,
            );
            __torajs_str_drop(key as *mut c_void);
            if ptag == INDEX_ANY_ACCESSOR_TAG {
                return __torajs_accessor_invoke_getter(
                    pval as *const c_void,
                    crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64),
                );
            }
            crate::payload_rc_inc(ptag as i64, pval as i64);
            return crate::nanbox_encode::__torajs_anyv_box_from_pair(ptag as i64, pval as i64);
        };
        __torajs_str_drop(key as *mut c_void);
        crate::payload_rc_inc(ftag as i64, fval as i64);
        crate::nanbox_encode::__torajs_anyv_box_from_pair(ftag as i64, fval as i64)
    }
}
