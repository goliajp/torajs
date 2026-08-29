//! The builtin-prototype leg of the own-key walk — split out of
//! `obj_own_keys.rs`, which the addition pushed past the 500-line
//! limit.
//!
//! A builtin prototype's methods are own properties that live in no
//! dict: `method_support_proto` answers them one key at a time for a
//! read or a gOPD, so the enumeration walk in the parent saw an empty
//! object and disagreed with `hasOwnProperty` about the same fact.

use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    /// torajs-anyvalue — the synthesized own string keys of a builtin
    /// prototype singleton (immortal interned Str cells, borrowed).
    /// A `cap == 0` probe answers the count.
    fn __torajs_builtin_proto_own_names(proto: *const c_void, out: *mut u64, cap: u64) -> u64;
    /// torajs-dynobj / torajs-arr — own-entry membership, for the
    /// dedupe below (`Array.prototype` keeps its entries in the Arr
    /// side props, every other prototype in a dynobj).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const u8) -> bool;
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
}

/// `torajs_rc::Tag::Closure` mirror — the cell shape §20.2.3 gives
/// `Function.prototype`.
const TAG_CLOSURE_CELL: u16 = 3;

/// Closure expando slot — `member_get_layout::CLOSURE_PROPS_OFF`
/// mirror.
const CLOSURE_PROPS_OFF: usize = 24;

/// Append a builtin prototype's synthesized own method / accessor /
/// `constructor` names to the key array being built.
///
/// A name the dict ALREADY carries is skipped: a monkey-patched
/// `Map.prototype.get = f` writes a real entry, and the synthesized
/// table still claims the name, so emitting both would list it twice.
/// The dict's copy wins because it is the one that answers the read.
///
/// All these names are non-enumerable, which is why every caller
/// gates on the gOPN / ownKeys surface — `Object.keys(Map.prototype)`
/// stays empty.
///
/// # Safety
/// `obj` is a live DynObj (or the `Array.prototype` Arr cell, with
/// `is_arr`); `arr` is a live `Arr<Str>` being built.
pub(crate) unsafe fn push_synthesized_proto_names(
    obj: *const c_void,
    mut arr: *mut u8,
    is_arr: bool,
) -> *mut u8 {
    let n = unsafe { __torajs_builtin_proto_own_names(obj, core::ptr::null_mut(), 0) };
    if n == 0 {
        return arr;
    }
    let mut names = vec![0u64; n as usize];
    unsafe { __torajs_builtin_proto_own_names(obj, names.as_mut_ptr(), n) };
    // §20.2.3 makes `Function.prototype` a built-in FUNCTION object,
    // so its own entries live in the closure expando rather than in
    // the cell (`torajs_anyvalue::method_support_proto::proto_dict`
    // mirror — torajs-meta keeps zero Cargo deps). Probing the
    // closure cell walked `fn_addr` as an entry count.
    let dict = if is_arr {
        obj
    } else if unsafe { obj.cast::<u8>().add(4).cast::<u16>().read() } == TAG_CLOSURE_CELL {
        unsafe { *(obj.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const *const c_void) }
    } else {
        obj
    };
    for cell in names {
        let present = if is_arr {
            unsafe { __torajs_arrprops_has(obj as *mut c_void, cell as *const c_void) != 0 }
        } else {
            !dict.is_null() && unsafe { __torajs_dynobj_has(dict, cell as *const u8) }
        };
        if present {
            continue;
        }
        // The cells are immortal interned statics — rc traffic no-ops
        // on the static flag, so the array slot borrows rather than
        // adopting a stake (unlike the `alloc_str_key` mints in the
        // parent).
        arr = unsafe { __torajs_arr_push(arr, cell as i64) };
    }
    arr
}
