//! `<key> in <obj>` for `Type::Any` rhs operands.
//!
//! ssa_lower's static `in`-op path (ssa_lower.rs T-45) dispatches on
//! the rhs operand's SSA type:
//!   Type::Arr(_) → inline numeric bounds check
//!   Type::Any   → previously: `__torajs_dynobj_has(any_unbox_value, key)`
//!                 — assumed every Any-tagged heap cell was a DynObj,
//!                 so an `arr: any = [1,2,3]; 0 in arr` SIGSEGV'd
//!                 (Array heap layout ≠ DynObj layout).
//!
//! This helper closes that gap. ssa_lower now routes the Type::Any
//! arm to one of two helpers, picked by the **key's** static SSA type:
//!
//!   `__torajs_in_op_any_num(v, key_i64)` — key is Number-typed
//!     • Tag::Arr   → bounds check  (0 ≤ key < len@+8)
//!     • else       → false (DynObj numeric-key + str ToString is
//!                    deferred — narrow ship; L3b watch)
//!
//!   `__torajs_in_op_any_str(v, key_str_ptr)` — key is String-typed
//!     • Tag::DynObj → `__torajs_dynobj_has(ptr, key)`
//!     • else        → false (Array str→numeric coercion deferred)
//!
//! Both helpers mirror `instanceof_any` discipline: NaN-box unbox →
//! tag-gate (ANY_HEAP) → NULL-gate → `HeapHeader::type_tag@+4` read.

use core::ffi::c_void;

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_anyv_unbox_tag(v: i64) -> i64;
    fn __torajs_anyv_unbox_value(v: i64) -> i64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const u8) -> i32;
}

// Offset of the i64 `len` slot inside the Array heap block — matches
// `ARR_LEN_OFF = 8` in ssa_lower.rs (8 bytes after the universal
// HeapHeader).
const ARR_LEN_OFF: usize = 8;

// Tag value ssa_lower emits for NaN-boxed heap pointers (mirrors
// `ANY_TAG_HEAP = 4` from torajs-anyvalue / ssa_lower).
const ANY_TAG_HEAP: i64 = 4;

// `torajs_rc::Tag` numeric values (stable ABI per assert_eq! suite
// in `lib.rs`):
const TAG_ARR: u16 = 2;
const TAG_DYNOBJ: u16 = 14;

/// `<key:number> in <v:any>` — Number-keyed dispatch.
///
/// Returns `true` iff `v` is a NaN-boxed heap pointer to a live Array
/// cell whose `len` covers `key` (`0 ≤ key < len`). Every other
/// runtime layout (non-heap tag, NULL ptr, non-Array tag) collapses
/// to `false` rather than UB.
///
/// # Safety
/// `v` is an unconstrained i64; helper is defensive on every step.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_in_op_any_num(v: i64, key: i64) -> bool {
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return false;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return false;
    }
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    if type_tag != TAG_ARR {
        return false;
    }
    let len = unsafe { *((ptr as *const u8).add(ARR_LEN_OFF) as *const i64) };
    key >= 0 && key < len
}

/// `<key:string> in <v:any>` — String-keyed dispatch.
///
/// Returns `true` iff `v` is a NaN-boxed heap pointer to a live
/// DynObj cell that owns the property named by `key`. Defensive on
/// every step.
///
/// # Safety
/// `v` is an unconstrained i64; `key` must be a torajs-str heap block
/// pointer (the same shape ssa_lower already passes to
/// `__torajs_dynobj_has`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_in_op_any_str(v: i64, key: *const u8) -> bool {
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return false;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return false;
    }
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    if type_tag != TAG_DYNOBJ {
        return false;
    }
    let r = unsafe { __torajs_dynobj_has(ptr, key) };
    r != 0
}

// Cargo-test stubs for the NaN-box / dynobj externs. Real symbols
// live in torajs-anyvalue / torajs-dynobj; unit tests in this crate
// hand-roll a NaN-box + dynobj_has stub so dispatch logic verifies
// in isolation.
#[cfg(test)]
unsafe fn __torajs_anyv_unbox_tag(v: i64) -> i64 {
    ((v as u64 >> 48) & 0xFFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_anyv_unbox_value(v: i64) -> i64 {
    (v as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_dynobj_has(_obj: *const c_void, _key: *const u8) -> i32 {
    // Tests that exercise the DynObj path set this thread-local
    // ahead of the call.
    DYNOBJ_HAS_RESULT.with(|r| r.get())
}

#[cfg(test)]
thread_local! {
    static DYNOBJ_HAS_RESULT: core::cell::Cell<i32> = const { core::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_arr_heap_block(len: i64) -> Vec<u8> {
        // 16 bytes: 4B refcount + 2B type_tag + 2B flags + 8B len.
        let mut block = vec![0u8; 16];
        block[4..6].copy_from_slice(&TAG_ARR.to_ne_bytes());
        block[ARR_LEN_OFF..ARR_LEN_OFF + 8].copy_from_slice(&len.to_ne_bytes());
        block
    }

    fn make_dynobj_heap_block() -> Vec<u8> {
        // 16 bytes: 4B refcount + 2B type_tag + 2B flags + 8B payload.
        let mut block = vec![0u8; 16];
        block[4..6].copy_from_slice(&TAG_DYNOBJ.to_ne_bytes());
        block
    }

    fn make_other_heap_block(type_tag: u16) -> Vec<u8> {
        let mut block = vec![0u8; 16];
        block[4..6].copy_from_slice(&type_tag.to_ne_bytes());
        block
    }

    fn nan_box(tag: i64, value: i64) -> i64 {
        (tag << 48) | (value & 0x0000_FFFF_FFFF_FFFF)
    }

    #[test]
    fn num_arr_key_in_bounds_returns_true() {
        let block = make_arr_heap_block(3);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(unsafe { __torajs_in_op_any_num(boxed, 0) });
        assert!(unsafe { __torajs_in_op_any_num(boxed, 2) });
    }

    #[test]
    fn num_arr_key_out_of_bounds_returns_false() {
        let block = make_arr_heap_block(3);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 3) });
        assert!(!unsafe { __torajs_in_op_any_num(boxed, -1) });
    }

    #[test]
    fn num_non_arr_tag_returns_false() {
        let block = make_other_heap_block(TAG_DYNOBJ);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 0) });
    }

    #[test]
    fn num_non_heap_tag_returns_false() {
        let boxed = nan_box(2, 0x1234_5678);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 0) });
    }

    #[test]
    fn num_null_ptr_returns_false() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 0) });
    }

    #[test]
    fn str_dynobj_dynobj_has_true_returns_true() {
        DYNOBJ_HAS_RESULT.with(|r| r.set(1));
        let block = make_dynobj_heap_block();
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        let key = b"foo\0".as_ptr();
        assert!(unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn str_dynobj_dynobj_has_false_returns_false() {
        DYNOBJ_HAS_RESULT.with(|r| r.set(0));
        let block = make_dynobj_heap_block();
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        let key = b"foo\0".as_ptr();
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn str_arr_tag_returns_false() {
        let block = make_arr_heap_block(3);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        let key = b"0\0".as_ptr();
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn str_non_heap_tag_returns_false() {
        let boxed = nan_box(2, 0x1234_5678);
        let key = b"foo\0".as_ptr();
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn str_null_ptr_returns_false() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        let key = b"foo\0".as_ptr();
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key) });
    }
}
