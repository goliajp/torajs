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

unsafe extern "C" {
    /// Cross-tier universal heap drop (tag-dispatched, rc-aware) —
    /// releases the boxed non-Copy content when the last stake on a
    /// promoted mutable capture box drops (RFC 20260710).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// NaN-box-aware release — cell-shaped AnyValues dec their heap
    /// cell, immediates are no-ops (RFC 20260710 C4, Any-typed
    /// promoted captures).
    fn __torajs_anyv_rc_dec(v: u64);
    /// Cycle-candidate registration. Cheap: gated on the header's
    /// walkable-children tag and a BUFFERED flag, so a repeat or a
    /// leaf costs one call and two loads.
    fn __torajs_cycle_buffer(p: *mut c_void);
}

const BOX_SIZE: usize = 16;

fn box_layout() -> std::alloc::Layout {
    // 16 bytes, 8-byte aligned (u64 + i64). Const inputs satisfy
    // Layout invariants; unchecked ctor avoids pulling Rust's panic
    // formatting path (polish A3).
    unsafe { std::alloc::Layout::from_size_align_unchecked(BOX_SIZE, 8) }
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
/// 1 — the promoting function's own stake, released by its fn-exit
/// drop walk (RFC 20260705 chunk 550 fix-up: the pre-fix rc=0
/// protocol counted only capturing closures, so an env released
/// while the outer frame was still live freed the box under the
/// outer's feet). Each closure-construction site inc's per use.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_capture_box_alloc(init_value: i64) -> *mut c_void {
    let base = unsafe { std::alloc::alloc(box_layout()) } as *mut u64;
    if base.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        *base = 1;
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
/// zero. Under the outer-stake protocol (alloc = rc 1) a drop at
/// rc 0 is unreachable when bookkeeping balances; the defensive
/// at-zero free is kept as a leak backstop.
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

/// [`__torajs_capture_box_drop`] for a box holding a NON-Copy value
/// (RFC 20260710 — promoted mutable captured binding): when the last
/// stake drops, release the boxed heap content through the universal
/// tag-dispatched drop BEFORE freeing the box. A zero payload (never
/// initialized / nulled slot) skips the content release.
///
/// # Safety
///
/// `slot_ptr` is null or a value-slot pointer from
/// [`__torajs_capture_box_alloc`]; a non-zero payload is a valid
/// universal-header heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_capture_box_drop_heap(slot_ptr: *mut c_void) {
    if slot_ptr.is_null() {
        return;
    }
    unsafe {
        let rc = rc_word(slot_ptr);
        if *rc > 1 {
            *rc -= 1;
            // The box survives, so its content did not lose a
            // reference — but a holder of the box went away, and the
            // box is a TRANSPARENT hop: `__env_trace_*` reports a byref
            // capture as an edge straight to the payload, with this
            // slot as the address to break. So the payload stands in
            // for the box under Bacon-Rajan's "decremented but still
            // positive" rule, and this is the one place that can say
            // so — the collector never sees boxes, which carry no
            // universal header.
            //
            // Without it a self-referential closure (`const f = n =>
            // … f(n - 1) …`) is unreachable garbage no pass can find:
            // env → box → env, and the env's own refcount is never
            // decremented by anyone, so nothing ever nominates it.
            let content = *(slot_ptr as *const i64);
            if content != 0 {
                __torajs_cycle_buffer(content as *mut c_void);
            }
            return;
        }
        // Last stake (rc 1, or the defensive never-inc'd rc 0 edge):
        // release the content, then the box.
        let content = *(slot_ptr as *const i64);
        if content != 0 {
            __torajs_value_drop_heap(content as *mut c_void);
        }
        std::alloc::dealloc(rc as *mut u8, box_layout());
    }
}

/// [`__torajs_capture_box_drop_heap`]'s Any-typed sibling (RFC
/// 20260710 C4): the box payload is a NaN-box AnyValue — the last
/// stake releases it through the NaN-box-aware dec (cell values dec
/// their heap cell, immediates are no-ops).
///
/// # Safety
///
/// `slot_ptr` is null or a value-slot pointer from
/// [`__torajs_capture_box_alloc`]; the payload carries a valid
/// AnyValue bit pattern (zero decodes as an immediate — no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_capture_box_drop_any(slot_ptr: *mut c_void) {
    if slot_ptr.is_null() {
        return;
    }
    unsafe {
        let rc = rc_word(slot_ptr);
        if *rc > 1 {
            *rc -= 1;
            return;
        }
        let content = *(slot_ptr as *const u64);
        if content != 0 {
            __torajs_anyv_rc_dec(content);
        }
        std::alloc::dealloc(rc as *mut u8, box_layout());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-binary stand-in for the runtime's universal heap drop —
    /// the real provider lives in the torajs-value-drop staticlib,
    /// which unit-test binaries don't link (torajs-arr stub
    /// convention). Records nothing; the drop-heap tests only
    /// exercise box rc mechanics with a zero payload.
    #[unsafe(no_mangle)]
    extern "C" fn __torajs_value_drop_heap(_p: *mut c_void) {}

    /// Same stub convention for the NaN-box-aware dec (real provider
    /// is the torajs-anyvalue staticlib).
    #[unsafe(no_mangle)]
    extern "C" fn __torajs_anyv_rc_dec(_v: u64) {}

    /// Same stub convention for the cycle-candidate registration
    /// (real provider is the torajs-cycle staticlib). The rc-mechanics
    /// tests all use a zero payload, which never reaches it.
    #[unsafe(no_mangle)]
    extern "C" fn __torajs_cycle_buffer(_p: *mut c_void) {}

    #[test]
    fn drop_heap_zero_payload_round_trip() {
        let slot = __torajs_capture_box_alloc(0);
        assert!(!slot.is_null());
        unsafe { __torajs_capture_box_inc(slot) };
        // First drop: rc 2 → 1, box stays live.
        unsafe { __torajs_capture_box_drop_heap(slot) };
        let v = unsafe { *(slot as *const i64) };
        assert_eq!(v, 0);
        // Last drop: zero payload skips content release, frees box.
        unsafe { __torajs_capture_box_drop_heap(slot) };
    }

    #[test]
    fn alloc_inc_drop_round_trip() {
        let slot = __torajs_capture_box_alloc(42);
        assert!(!slot.is_null());
        // Read the value through the slot pointer (mirrors what
        // ssa_lower emits: Load i64 at slot+0).
        let v = unsafe { *(slot as *const i64) };
        assert_eq!(v, 42);
        // alloc = 1 (outer stake); inc x 2 (two capturing envs).
        unsafe {
            __torajs_capture_box_inc(slot);
            __torajs_capture_box_inc(slot);
        }
        let rc = unsafe { *rc_word(slot) };
        assert_eq!(rc, 3);
        // env drops x 2 leave the outer stake alive — the value
        // stays readable (the chunk-550 regression shape).
        unsafe {
            __torajs_capture_box_drop(slot);
            __torajs_capture_box_drop(slot);
        }
        let v = unsafe { *(slot as *const i64) };
        assert_eq!(v, 42);
        // outer fn-exit drop → frees.
        unsafe { __torajs_capture_box_drop(slot) };
        // Slot is freed; we can't safely dereference. Test passes
        // if no panic / leak (also asan-friendly).
    }

    #[test]
    fn uncaptured_promoted_box_frees_on_outer_drop() {
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
