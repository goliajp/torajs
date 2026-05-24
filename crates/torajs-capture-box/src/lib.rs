//! Refcounted heap box for escape-captured `let` slots — port of
//! `runtime_capture_box.c` (75 LOC, 3 fns; P6.5, 2026-05-24).
//!
//! Standalone Rust crate; distinct from [`torajs-rc`]'s universal
//! heap header (which is `refcount u32 + type_tag u16 + flags u16`
//! = 8 B). The capture box uses a wider u64 refcount + i64 value =
//! 16 B layout because the box is value-typed (no tag dispatch
//! needed — codegen knows the type at the alloc site).
//!
//! ## Layout (16 bytes)
//!
//! ```text
//!   base+0  : refcount u64 (rc starts at 0; each closure-construction
//!             that captures inc's, each env_drop dec's)
//!   base+8  : the actual i64 value (Number / Bool widened / ...)
//! ```
//!
//! Crucially, the pointer ssa_lower threads around (`info.slot`)
//! points at the VALUE slot (= `base + 8`). All `Load` / `Store`
//! sites in the body remain `slot+0` reads/writes; ARC
//! bookkeeping steps back 8 bytes inside the helpers. This keeps
//! the substrate footprint small — no Load/Store offset sweep.
//!
//! ## Why rc=0 initial state
//!
//! A let that gets heap-promoted but never captured at runtime
//! (the escape_captured_lets pre-pass collects all captures
//! statically — but the runtime check is conservative) still
//! wouldn't leak: the box would never be `inc`'d nor `drop`'d and
//! would reclaim at process exit. Captured paths `inc` per
//! construction (rc = N for N closures) and `drop` per env-drop,
//! with exact free at last-drop. The drop fn includes a defensive
//! at-zero-observation free for the never-captured edge case.

use core::ffi::c_void;

const BOX_SIZE: usize = 16;

fn box_layout() -> std::alloc::Layout {
    // 16 bytes, 8-byte aligned (u64 + i64).
    std::alloc::Layout::from_size_align(BOX_SIZE, 8).unwrap()
}

/// Step back from a value-slot pointer (`base + 8`) to the
/// refcount word (`base + 0`).
///
/// # Safety
///
/// `slot_ptr` must have been returned by
/// [`__torajs_capture_box_alloc`] (or `null`).
unsafe fn rc_word(slot_ptr: *mut c_void) -> *mut u64 {
    unsafe { (slot_ptr as *mut u64).offset(-1) }
}

/// Allocate a 16-byte capture box, write `init_value` at base+8,
/// return the value-slot pointer (= base + 8). Refcount starts at
/// 0; the caller's closure-construction site inc's per use.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_capture_box_alloc(init_value: i64) -> *mut c_void {
    let base = unsafe { std::alloc::alloc(box_layout()) } as *mut u64;
    if base.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        *base = 0;
        *(base.add(1) as *mut i64) = init_value;
    }
    unsafe { base.add(1) as *mut c_void }
}

/// Inc the refcount of a capture box. `slot_ptr` is the value-slot
/// pointer (= base + 8).
///
/// # Safety
///
/// `slot_ptr` is null or a value-slot pointer from
/// [`__torajs_capture_box_alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_capture_box_inc(slot_ptr: *mut c_void) {
    if slot_ptr.is_null() {
        return;
    }
    unsafe {
        let rc = rc_word(slot_ptr);
        *rc += 1;
    }
}

/// Dec the refcount; free the underlying allocation when it hits
/// zero. Defensive at-zero-observation free covers the unused-but-
/// promoted edge case (see crate docs).
///
/// # Safety
///
/// `slot_ptr` is null or a value-slot pointer from
/// [`__torajs_capture_box_alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_capture_box_drop(slot_ptr: *mut c_void) {
    if slot_ptr.is_null() {
        return;
    }
    unsafe {
        let rc = rc_word(slot_ptr);
        if *rc == 0 {
            // Never inc'd — heap-promoted let that wasn't actually
            // captured at runtime, or rc bookkeeping bug. Free here
            // to avoid leaking.
            std::alloc::dealloc(rc as *mut u8, box_layout());
            return;
        }
        *rc -= 1;
        if *rc == 0 {
            std::alloc::dealloc(rc as *mut u8, box_layout());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_inc_drop_round_trip() {
        let slot = __torajs_capture_box_alloc(42);
        assert!(!slot.is_null());
        // Read the value through the slot pointer (mirrors what
        // ssa_lower emits: Load i64 at slot+0).
        let v = unsafe { *(slot as *const i64) };
        assert_eq!(v, 42);
        // inc x 2.
        unsafe {
            __torajs_capture_box_inc(slot);
            __torajs_capture_box_inc(slot);
        }
        let rc = unsafe { *rc_word(slot) };
        assert_eq!(rc, 2);
        // drop x 2 → frees.
        unsafe {
            __torajs_capture_box_drop(slot);
            __torajs_capture_box_drop(slot);
        }
        // Slot is freed; we can't safely dereference. Test passes
        // if no panic / leak (also asan-friendly).
    }

    #[test]
    fn never_inced_drop_frees_immediately() {
        let slot = __torajs_capture_box_alloc(0);
        assert!(!slot.is_null());
        unsafe { __torajs_capture_box_drop(slot) };
    }

    #[test]
    fn null_inputs_no_op() {
        unsafe {
            __torajs_capture_box_inc(core::ptr::null_mut());
            __torajs_capture_box_drop(core::ptr::null_mut());
        }
    }

    #[test]
    fn value_slot_is_base_plus_8() {
        let slot = __torajs_capture_box_alloc(0x12_3456_789a);
        let value_offset = (slot as usize) % 8;
        assert_eq!(value_offset, 0, "value slot must be 8-aligned");
        let rc_ptr = unsafe { rc_word(slot) } as usize;
        assert_eq!((slot as usize) - rc_ptr, 8);
        unsafe { __torajs_capture_box_drop(slot) };
    }
}
