//! §10.1.8.1 step 4 over the builtin prototypes — the walk that
//! `method_support_proto::proto_level_value` is one level of.
//!
//! Every consumer of the prototype face used to ask ONE singleton and
//! stop, which was right only while every builtin prototype hung
//! straight off %Object.prototype%. Two things break that:
//!
//! - `Object.prototype.foo = 5` is unreachable from a receiver whose
//!   family is anything else — `const a: any = []` read it (517-07
//!   closed the any lane) while a typed array did not, because the
//!   typed lane's kernel asked `Array.prototype` alone.
//! - §23.1.5.2 puts %ArrayIteratorPrototype% under
//!   %Iterator.prototype%, so a two-level "family, else root" walk
//!   skips the middle link entirely.
//!
//! `proto_parent_tag` is the one place that knows how many links
//! there are, and this walk is built on it, so neither shape can
//! drift again.

use core::ffi::c_void;

use crate::method_support_proto::{proto_dict, proto_is_arr, proto_level_value};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-arr — per-index attribute word (hole tombstone bit).
    fn __torajs_arr_index_flags(arr: *const c_void, i: u64) -> u64;
}

/// torajs-arr `layout::ARR_LEN_OFF` mirror.
const ARR_LEN_OFF: usize = 8;

/// The value the chain starting at `tag` answers for `key` —
/// undefined when no link claims it.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn proto_chain_value(tag: i64, key: *const c_void) -> AnyValue {
    let mut t = tag;
    while t >= 0 {
        if let Some(v) = unsafe { proto_level_value(t, key) } {
            return v;
        }
        t = torajs_rc::builtin_proto::proto_parent_tag(t);
    }
    VALUE_UNDEFINED
}

/// Whether any link of the chain starting at `tag` owns `key` in its
/// singleton's expando — the membership twin, for the `in` face.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn proto_chain_expando_owns(tag: i64, key: *const c_void) -> bool {
    let mut t = tag;
    while t >= 0 {
        if unsafe { level_expando_owns(t, key) } {
            return true;
        }
        t = torajs_rc::builtin_proto::proto_parent_tag(t);
    }
    false
}

/// ONE level's singleton expando own-key probe — the interned
/// method tables only cover mid-named entries; anything else a user
/// installed on `<Ctor>.prototype` lives in the singleton's expando
/// storage (the side-props table for the Arr-backed
/// `Array.prototype`, the dynobj itself everywhere else).
unsafe fn level_expando_owns(family_tag: i64, key: *const c_void) -> bool {
    let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(family_tag) };
    if proto.is_null() {
        return false;
    }
    if unsafe { proto_is_arr(proto) } {
        // 刀 5 G3 — the index domain is owned by the singleton's
        // element storage (`Array.prototype[1] = v` grows it): a
        // canonical in-bounds index is present unless its slot is a
        // hole tombstone. The side-props table only holds SHADOW
        // entries for indices (attributes / tombstones), so a
        // canonical key never consults `arrprops_has` — a deleted
        // index's tombstone entry would read as present.
        if let Some(i) = unsafe { crate::prop_has::canonical_index(key) } {
            let len = unsafe { proto.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
            return i < len
                && unsafe { __torajs_arr_index_flags(proto as *const c_void, i) }
                    & crate::prop_has::ARR_F_HOLE
                    == 0;
        }
        return unsafe { __torajs_arrprops_has(proto, key) } != 0;
    }
    let dict = unsafe { proto_dict(proto) };
    !dict.is_null() && unsafe { __torajs_dynobj_has(dict, key) != 0 }
}
