//! §7.2.2 IsArray — one answer, for every asker.
//!
//! The predicate is not "is this cell an array". It walks a Proxy to
//! its `[[ProxyTarget]]`, however deep, and it THROWS on a revoked
//! one (step 3.a) rather than answering. It also says no to an
//! arguments object, which tr mints as an `Arr` cell but which is an
//! ordinary object carrying a [[ParameterMap]], not an Array exotic
//! one.
//!
//! Two callers ask it, and before this module they were two separate
//! implementations that disagreed:
//! `Object.prototype.toString`'s step 3 (§20.1.3.6) and
//! `Array.isArray` (§22.1.2.2). The same `arguments` object was "not
//! an array" to one and "an array" to the other.

use core::ffi::c_void;

use crate::FLAG_ARR_ARGUMENTS;

#[cfg(not(test))]
unsafe extern "C" {
    /// torajs-anyvalue — NaN-box accessors (the same pair
    /// `in_op_any` externs).
    fn __torajs_anyv_unbox_tag(v: i64) -> i64;
    fn __torajs_anyv_unbox_value(v: i64) -> i64;
    fn __torajs_throw_type_error(msg: *const u8);
}

/// Tag ssa_lower emits for NaN-boxed heap pointers (mirrors
/// `ANY_TAG_HEAP = 4`).
const ANY_TAG_HEAP: i64 = 4;
/// `VALUE_NULL` — the whole bit pattern, per `nanbox`'s table.
const ANY_VALUE_NULL: i64 = 0x2;
/// `crate::Tag` numeric values (stable ABI per the assert_eq! suite
/// in `lib.rs`).
const TAG_ARR: u16 = 2;
const TAG_PROXY: u16 = 26;
/// Proxy cell: `{ header:8 | target:8 | handler:8 }`.
const PROXY_TARGET_OFF: usize = 8;
const PROXY_HANDLER_OFF: usize = 16;
/// Universal heap header: type tag u16 @ +4, flags half-word @ +6.
const HDR_TYPE_TAG_OFF: usize = 4;
const HDR_FLAGS_OFF: usize = 6;

/// A proxy-of-a-proxy chain is finite (ProxyCreate rejects a revoked
/// target), but the walk is bounded anyway — a stack overflow is a
/// poor way to learn that invariant was wrong.
const MAX_PROXY_DEPTH: usize = 1000;

/// §7.2.2. `Ok(true)` / `Ok(false)`, or `Err(())` with the step 3.a
/// TypeError already in flight.
///
/// # Safety
/// `v` is an unconstrained i64; every step is defensive.
pub unsafe fn is_array_spec(v: i64) -> Result<bool, ()> {
    let mut cur = v;
    for _ in 0..MAX_PROXY_DEPTH {
        if unsafe { __torajs_anyv_unbox_tag(cur) } != ANY_TAG_HEAP {
            return Ok(false);
        }
        let ptr = unsafe { __torajs_anyv_unbox_value(cur) } as *const c_void;
        if ptr.is_null() {
            return Ok(false);
        }
        let type_tag = unsafe { *((ptr as *const u8).add(HDR_TYPE_TAG_OFF) as *const u16) };
        if type_tag != TAG_PROXY {
            if type_tag != TAG_ARR {
                return Ok(false);
            }
            // The arguments object is the one Arr cell that is not an
            // Array exotic object.
            let flags = unsafe { *((ptr as *const u8).add(HDR_FLAGS_OFF) as *const u16) };
            return Ok(flags & FLAG_ARR_ARGUMENTS == 0);
        }
        let handler = unsafe { *((ptr as *const u8).add(PROXY_HANDLER_OFF) as *const i64) };
        if handler == ANY_VALUE_NULL {
            unsafe {
                __torajs_throw_type_error(
                    b"Cannot perform 'IsArray' on a proxy that has been revoked\0".as_ptr(),
                )
            };
            return Err(());
        }
        cur = unsafe { *((ptr as *const u8).add(PROXY_TARGET_OFF) as *const i64) };
    }
    Ok(false)
}

/// The C face: `1` array, `0` not, `-1` the revoked-proxy TypeError is
/// pending (the caller's throw check forwards it).
///
/// # Safety
/// `v` is an unconstrained i64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_is_array_spec(v: i64) -> i64 {
    match unsafe { is_array_spec(v) } {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(()) => -1,
    }
}

/// `Array.isArray(v)` for a `Type::Any` argument — §22.1.2.2, which
/// is `IsArray` and nothing else.
///
/// Compile-time `Array.isArray(x)` resolves statically when `x` has a
/// concrete SSA type; only the `any` lane reaches here. A revoked
/// proxy leaves the TypeError pending and answers `false`, which the
/// lowering's throw check turns into the throw before the value is
/// ever read.
///
/// # Safety
/// `v` is an unconstrained i64; helper is defensive on every step.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_is_arr(v: i64) -> bool {
    matches!(unsafe { is_array_spec(v) }, Ok(true))
}

// Cargo-test stubs for the NaN-box / throw externs — the real
// symbols live in torajs-anyvalue and torajs-throw. Same arrangement
// `in_op_any` uses for its own dispatch tests.
#[cfg(test)]
unsafe fn __torajs_anyv_unbox_tag(v: i64) -> i64 {
    ((v as u64 >> 48) & 0xFFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_anyv_unbox_value(v: i64) -> i64 {
    (v as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_throw_type_error(_msg: *const u8) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn nan_box(tag: i64, value: i64) -> i64 {
        (tag << 48) | (value & 0x0000_FFFF_FFFF_FFFF)
    }

    /// 32 bytes: 4B refcount + 2B type_tag + 2B flags + payload —
    /// wide enough for a Proxy's target/handler at +8 / +16.
    fn block(type_tag: u16, flags: u16) -> Vec<u8> {
        let mut b = vec![0u8; 32];
        b[4..6].copy_from_slice(&type_tag.to_ne_bytes());
        b[6..8].copy_from_slice(&flags.to_ne_bytes());
        b
    }

    fn boxed(b: &[u8]) -> i64 {
        nan_box(ANY_TAG_HEAP, b.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF)
    }

    fn proxy_block(target: i64, handler: i64) -> Vec<u8> {
        let mut b = block(TAG_PROXY, 0);
        b[PROXY_TARGET_OFF..PROXY_TARGET_OFF + 8].copy_from_slice(&target.to_ne_bytes());
        b[PROXY_HANDLER_OFF..PROXY_HANDLER_OFF + 8].copy_from_slice(&handler.to_ne_bytes());
        b
    }

    #[test]
    fn plain_array_is_an_array() {
        let a = block(TAG_ARR, 0);
        assert_eq!(unsafe { is_array_spec(boxed(&a)) }, Ok(true));
    }

    #[test]
    fn dynobj_is_not() {
        let o = block(1, 0);
        assert_eq!(unsafe { is_array_spec(boxed(&o)) }, Ok(false));
    }

    #[test]
    fn non_heap_and_null_pointer_are_not() {
        assert_eq!(unsafe { is_array_spec(nan_box(2, 0x42)) }, Ok(false));
        assert_eq!(
            unsafe { is_array_spec(nan_box(ANY_TAG_HEAP, 0)) },
            Ok(false)
        );
    }

    #[test]
    fn arguments_object_is_not_an_array_exotic_object() {
        let a = block(TAG_ARR, FLAG_ARR_ARGUMENTS);
        assert_eq!(unsafe { is_array_spec(boxed(&a)) }, Ok(false));
    }

    #[test]
    fn proxy_answers_for_its_target() {
        let arr = block(TAG_ARR, 0);
        let obj = block(1, 0);
        let handler = block(1, 0);
        let over_arr = proxy_block(boxed(&arr), boxed(&handler));
        let over_obj = proxy_block(boxed(&obj), boxed(&handler));
        assert_eq!(unsafe { is_array_spec(boxed(&over_arr)) }, Ok(true));
        assert_eq!(unsafe { is_array_spec(boxed(&over_obj)) }, Ok(false));
    }

    #[test]
    fn proxy_walk_recurses() {
        let arr = block(TAG_ARR, 0);
        let handler = block(1, 0);
        let inner = proxy_block(boxed(&arr), boxed(&handler));
        let outer = proxy_block(boxed(&inner), boxed(&handler));
        assert_eq!(unsafe { is_array_spec(boxed(&outer)) }, Ok(true));
    }

    #[test]
    fn revoked_proxy_throws_rather_than_answering() {
        let arr = block(TAG_ARR, 0);
        let revoked = proxy_block(boxed(&arr), ANY_VALUE_NULL);
        assert_eq!(unsafe { is_array_spec(boxed(&revoked)) }, Err(()));
    }

    #[test]
    fn a_proxy_over_arguments_is_still_not_an_array() {
        let args = block(TAG_ARR, FLAG_ARR_ARGUMENTS);
        let handler = block(1, 0);
        let p = proxy_block(boxed(&args), boxed(&handler));
        assert_eq!(unsafe { is_array_spec(boxed(&p)) }, Ok(false));
    }
}
