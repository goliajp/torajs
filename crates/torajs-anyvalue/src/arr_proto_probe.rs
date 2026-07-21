//! `__torajs_object_proto_index_get` — %Object.prototype% digit-key
//! probe, the chain tail of an Arr element read that found no own
//! property (RFC 20260721-array-proto-cluster 刀 5 G3).
//!
//! §10.1.8.1 OrdinaryGet step 3: a hole (elision / `delete` /
//! length-grow) or an out-of-bounds index is not an own property, so
//! [[Get]] continues along the chain. torajs-arr's
//! `__torajs_arr_proto_index_get` owns the Array.prototype leg (the
//! tag-2 singleton is an Arr cell — its digit-key data lives in ITS
//! element storage) and falls through here for the final
//! %Object.prototype% hop, which is a plain dynobj host. An accessor
//! entry runs its getter with the ORIGINAL receiver.
//!
//! Cold by construction: only reached from the hole / OOB exits, and
//! an unpatched %Object.prototype% answers in one entry probe.

use core::ffi::c_void;

use crate::index_any::{dynobj_index_entry, i64_dec};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    /// torajs-str — heap Str constructor (rc=1) for the decimal key.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release the probe key.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — hole-tombstone probe (a deleted proto index
    /// is absent, never a resurrected undefined).
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
}

/// See module doc. Answers an OWNED AnyValue (`undefined` when
/// %Object.prototype% misses too) — same +1-for-cells contract as
/// `__torajs_arr_index_get`, whose cold exits delegate here through
/// the Array.prototype leg.
///
/// # Safety
/// `recv` is a live `Tag::Arr` heap pointer (the original receiver —
/// accessor getters run against it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_object_proto_index_get(recv: *mut c_void, idx: i64) -> AnyValue {
    let op = unsafe {
        torajs_rc::builtin_proto::__torajs_get_builtin_prototype(
            torajs_rc::builtin_proto::OBJECT_PROTO_TAG as i64,
        )
    };
    if op.is_null() {
        return VALUE_UNDEFINED;
    }
    let mut buf = [0u8; 20];
    let (start, len) = i64_dec(&mut buf, idx);
    unsafe {
        let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
        let r = if __torajs_dynobj_entry_is_hole(op as *const c_void, key as *const c_void) != 0 {
            None
        } else {
            dynobj_index_entry(op as *const c_void, key as *const c_void, recv)
        };
        __torajs_str_drop(key as *mut c_void);
        r.unwrap_or(VALUE_UNDEFINED)
    }
}
