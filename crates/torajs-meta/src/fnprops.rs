//! Function-instance property side table — port of
//! `runtime_str.c` L697-752.
//!
//! For non-Closure functions (FnSig form) the per-instance property
//! bag (`fn.x = v`) cannot live in the function's own layout (there
//! is no layout — fn pointers are bare globals). Instead a side
//! table maps `fn_ptr → dynobj`, and the dynobj holds the props.
//! Lazy: a function that never gains a property never gets an entry.
//!
//! Closure-form functions use the in-layout `CLOSURE_PROPS_OFF`
//! path; this table is only for FnSig fns.
//!
//! Single-threaded JS execution: a plain `Mutex<HashMap>` suffices.
//! The C version used unsynchronized `static` linked-list buckets;
//! switching to HashMap removes the per-bucket scan and matches
//! "正统" Rust runtime style. Same per-fn lookup complexity (O(1)
//! amortized) and zero behavior change.

use core::ffi::c_void;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
}

/// Per-fn-pointer `dynobj` mapping. Both keys and values are
/// pointer-sized integers; we store as `usize` so the `HashMap`
/// (and the surrounding `Mutex` / `OnceLock`) is `Sync`-clean
/// without raw-pointer Send violations.
type FnPropsTable = HashMap<usize, usize>;
static FNPROPS: OnceLock<Mutex<FnPropsTable>> = OnceLock::new();

#[inline]
fn table() -> &'static Mutex<FnPropsTable> {
    FNPROPS.get_or_init(|| Mutex::new(HashMap::new()))
}

const ANY_UNDEF_TAG: u64 = 5;

#[inline]
fn intern(fn_ptr: *mut c_void) -> *mut c_void {
    let mut t = table().lock().expect("torajs-meta fnprops mutex poisoned");
    let key = fn_ptr as usize;
    if let Some(&p) = t.get(&key) {
        return p as *mut c_void;
    }
    let new_dynobj = unsafe { __torajs_dynobj_alloc() };
    t.insert(key, new_dynobj as usize);
    new_dynobj
}

#[inline]
fn lookup(fn_ptr: *mut c_void) -> *mut c_void {
    let t = table().lock().expect("torajs-meta fnprops mutex poisoned");
    t.get(&(fn_ptr as usize))
        .copied()
        .map(|v| v as *mut c_void)
        .unwrap_or(core::ptr::null_mut())
}

/// `fn.x = value` — intern the per-fn dynobj if needed, then
/// `__torajs_dynobj_set` the (key, tag, value) triple onto it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fnprops_set(
    fn_ptr: *mut c_void,
    key: *const c_void,
    tag: i64,
    value: i64,
) {
    let mut dynobj = intern(fn_ptr);
    // dynobj_set may grow the dynobj and reassign the pointer; we
    // need to write the new pointer back into the table.
    unsafe { __torajs_dynobj_set(&mut dynobj, key as *const u8, tag as u64, value as u64) };
    // Write back the (possibly-reallocated) dynobj pointer.
    let mut t = table().lock().expect("torajs-meta fnprops mutex poisoned");
    t.insert(fn_ptr as usize, dynobj as usize);
}

/// `fn.x` — return the slot's tag, or `ANY_UNDEF` if no fnprops
/// entry / no key.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fnprops_get_tag(fn_ptr: *mut c_void, key: *const c_void) -> u64 {
    let dynobj = lookup(fn_ptr);
    if dynobj.is_null() {
        return ANY_UNDEF_TAG;
    }
    unsafe { __torajs_dynobj_get_tag(dynobj, key as *const u8) }
}

/// `fn.x` — return the slot's value half (i64 bits / heap ptr).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fnprops_get_value(
    fn_ptr: *mut c_void,
    key: *const c_void,
) -> u64 {
    let dynobj = lookup(fn_ptr);
    if dynobj.is_null() {
        return 0;
    }
    unsafe { __torajs_dynobj_get_value(dynobj, key as *const u8) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lookup_empty_returns_undef_tag() {
        // ANY_UNDEF_TAG (5) is the unregistered-fn fallback.
        let result = unsafe {
            __torajs_fnprops_get_tag(
                0x1234 as *mut c_void,
                b"missing\0".as_ptr() as *const c_void,
            )
        };
        assert_eq!(result, ANY_UNDEF_TAG);
    }

    #[test]
    fn table_lookup_empty_returns_zero_value() {
        let result = unsafe {
            __torajs_fnprops_get_value(0x5678 as *mut c_void, b"x\0".as_ptr() as *const c_void)
        };
        assert_eq!(result, 0);
    }
}
