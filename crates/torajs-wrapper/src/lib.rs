//! Primitive-wrapper heap objects — `new Number(x)` / `new String(x)` /
//! `new Boolean(x)` (RFC 20260716-primitive-wrapper-substrate 刀 1).
//!
//! Three fixed-size (16B) heap blocks, each carrying the universal
//! [`torajs_rc::HeapHeader`] plus one payload word:
//!
//! ```text
//! NumberWrapper  = [header:8][num:8]          [props_slot:8]  (f64)
//! StringWrapper  = [header:8][str_cell:8]     [props_slot:8]  (*mut u8 borrow of Tag::Str cell, rc_inc'd)
//! BooleanWrapper = [header:8][val:1 + pad:7]  [props_slot:8]  (0 = false, 1 = true)
//! ```
//!
//! **Lazy expando props slot** (RFC 20260716 刀 5, rotation 121) — the
//! `+16` word carries a `*mut DynObj` that starts NULL. The first
//! `wrapper.foo = X` allocates the props table (mirror of the closure
//! cell's `+24` slot at `member_set.rs`); subsequent gets/sets read it
//! back. Drop releases the props dynobj through the universal drop
//! dispatcher, so a whole own-property tree tears down uniformly.
//!
//! # Contract with the universal drop dispatcher
//!
//! Each `__torajs_*_wrapper_drop` performs the "release one reference"
//! duty for its own inner refs and frees the outer block. It is meant
//! to be called by [`torajs_value_drop::__torajs_value_drop_heap`]'s
//! per-tag arm AFTER `__torajs_rc_dec` has returned `1` on the wrapper
//! itself (mirrors the [`__torajs_bigint_drop`] shape). The rc-aware
//! sibling helper `__torajs_*_wrapper_drop_rc` combines the dec + free
//! for ssa_lower's `emit_drop_value` binding-drop sites.
//!
//! # ABI note
//!
//! Wrappers alloc via mmalloc's libc-compat shim (same channel as
//! BigInt / Str / DynObj) — no libc / no crates.io dep, per the
//! `docs/design-principles.md` self-hosted pillar. The `rc_inc` /
//! `rc_dec` calls resolve at link time against `libtorajs_rc.a`.

#![no_std]

use core::ffi::c_void;

use torajs_rc::{HeapHeader, Tag};

// ============================================================
// Alloc / free primitives — mmalloc libc-compat
// ============================================================

unsafe extern "C" {
    /// mmalloc's libc-compat `malloc(size)`. Same shim BigInt / DynObj /
    /// Symbol / etc. call into.
    #[link_name = "__torajs_libc_malloc"]
    fn libc_malloc(size: usize) -> *mut c_void;

    /// mmalloc's libc-compat `free(ptr)` (no size arg — mmalloc's
    /// header tracks the class internally).
    #[link_name = "__torajs_libc_free"]
    fn libc_free(ptr: *mut c_void);

    /// Universal drop dispatcher (resolves against libtorajs_value_drop.a
    /// at link time). Rc-decs `child` and, on hit-zero, routes to the
    /// per-tag drop arm (which frees the block).
    fn __torajs_value_drop_heap(child: *mut c_void);

    /// Rc decrement (torajs-rc). Returns `1` on hit-zero; `0` otherwise.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;

    /// torajs-cycle — register a possible cycle root when the rc
    /// stayed positive (RFC 20260717 blade 3: wrappers joined the
    /// trial-deletion walk through their +16 expando props dict).
    fn __torajs_cycle_buffer(p: *mut c_void);
    /// torajs-cycle — scrub a normal-dropped block from the root
    /// buffer before its memory is freed.
    fn __torajs_cycle_unbuffer(p: *mut c_void);
}

/// Byte layout constants — mirror-referenced by ssa_lower emit sites
/// (`__torajs_wrapper::*_OFF`). Every offset is `usize` so LLVM constant-
/// folds them into the generated `getelementptr` sequences.

pub const WRAPPER_HDR_SIZE: usize = 8;

/// Shared lazy props-dynobj slot (RFC 20260716 刀 5) — every wrapper
/// carries `[header:8][value:8][props_slot:8]`. NULL until the first
/// `wrapper.foo = X`; then holds a `*mut DynObj` that own-property
/// reads/writes route through. Mirror of `MEMBER_SET_CLOSURE_PROPS_OFF`
/// at `torajs-anyvalue/src/member_set.rs`.
pub const WRAPPER_PROPS_OFF: usize = 16;

/// NumberWrapper — `[header:8][num:8][props_slot:8]`.
pub const NUMBER_WRAPPER_SIZE: usize = 24;
pub const NUMBER_WRAPPER_VALUE_OFF: usize = 8;

/// StringWrapper — `[header:8][str_cell_ptr:8][props_slot:8]`.
pub const STRING_WRAPPER_SIZE: usize = 24;
pub const STRING_WRAPPER_CELL_OFF: usize = 8;

/// BooleanWrapper — `[header:8][val:1 + pad:7][props_slot:8]`.
pub const BOOLEAN_WRAPPER_SIZE: usize = 24;
pub const BOOLEAN_WRAPPER_VALUE_OFF: usize = 8;

// ============================================================
// Header init
// ============================================================

#[inline]
unsafe fn init_wrapper_header(ptr: *mut u8, tag: Tag) {
    let hdr = ptr as *mut HeapHeader;
    unsafe {
        (*hdr).refcount = 1;
        (*hdr).type_tag = tag as u16;
        (*hdr).flags = 0;
        // Lazy expando props slot starts NULL — no dynobj alloc until
        // the first `wrapper.foo = X`. Mirror of the closure cell's
        // +24 slot at member_set.rs.
        (ptr.add(WRAPPER_PROPS_OFF) as *mut *mut c_void).write(core::ptr::null_mut());
    }
}

/// Release the wrapper's lazy props dynobj through the universal drop
/// dispatcher. NULL-safe. Called by each wrapper's `_drop` arm BEFORE
/// freeing the outer block, so the drop dispatcher walks the props
/// tree uniformly (mirror of `string_wrapper_drop`'s inner-cell
/// release).
///
/// # Safety
/// `ptr` must point to a live wrapper cell with a valid +16 props slot
/// (any wrapper allocated via one of the three `*_wrapper_new` fns).
#[inline]
unsafe fn release_wrapper_props(ptr: *mut u8) {
    unsafe {
        let props = (ptr.add(WRAPPER_PROPS_OFF) as *const *mut c_void).read();
        if !props.is_null() {
            __torajs_value_drop_heap(props);
        }
    }
}

// ============================================================
// NumberWrapper — new Number(x), [[NumberData]] = x
// ============================================================

/// Allocate a fresh Number wrapper with refcount=1 and
/// `[[NumberData]] = val` (spec §21.1.1.1). Never null in practice —
/// mmalloc panics on OOM.
///
/// # Safety
/// The returned pointer owns a fresh refcount=1 block; the caller is
/// responsible for eventually dropping it via the universal dispatcher
/// or `__torajs_number_wrapper_drop_rc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_number_wrapper_new(val: f64) -> *mut u8 {
    let ptr = unsafe { libc_malloc(NUMBER_WRAPPER_SIZE) } as *mut u8;
    unsafe {
        init_wrapper_header(ptr, Tag::NumberWrapper);
        (ptr.add(NUMBER_WRAPPER_VALUE_OFF) as *mut f64).write(val);
    }
    ptr
}

/// Read `[[NumberData]]`. Caller asserts `ptr` is a live NumberWrapper.
///
/// # Safety
/// `ptr` must point to a live NumberWrapper cell.
#[inline]
pub unsafe fn number_wrapper_value(ptr: *mut u8) -> f64 {
    unsafe { (ptr.add(NUMBER_WRAPPER_VALUE_OFF) as *const f64).read() }
}

/// Direct free of a NumberWrapper block. Called by the universal
/// drop dispatcher AFTER `__torajs_rc_dec` returned 1. NULL-safe.
///
/// # Safety
/// `p` must be either NULL or a valid NumberWrapper block whose
/// refcount just reached zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_number_wrapper_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    unsafe {
        __torajs_cycle_unbuffer(p);
        release_wrapper_props(p as *mut u8);
        libc_free(p);
    };
}

/// Rc-aware drop. Decrements; frees iff this was the last owner.
///
/// # Safety
/// `p` must be either NULL or a valid pointer to a NumberWrapper
/// block with a live refcount header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_number_wrapper_drop_rc(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } != 0 {
        unsafe { __torajs_number_wrapper_drop(p) };
    } else {
        unsafe { __torajs_cycle_buffer(p) };
    }
}

// ============================================================
// StringWrapper — new String(s), [[StringData]] = str cell (borrow +1)
// ============================================================

/// Allocate a fresh String wrapper adopting an existing owning
/// reference on `cell` (a Tag::Str block). **Transfer semantics —
/// the caller's `+1` is consumed** (no rc_inc here), matching the
/// convention `arr_alloc_any_filled` / `__torajs_new_*` args use.
/// Caller must not `rc_dec` `cell` after the call; the wrapper's
/// drop path releases it. `cell` may be NULL (interpreted as an
/// empty-string wrapper by downstream consumers).
///
/// # Safety
/// `cell` is NULL or a live Tag::Str block with the universal
/// HeapHeader layout whose refcount the caller intends to transfer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_string_wrapper_new(cell: *mut u8) -> *mut u8 {
    let ptr = unsafe { libc_malloc(STRING_WRAPPER_SIZE) } as *mut u8;
    unsafe {
        init_wrapper_header(ptr, Tag::StringWrapper);
        (ptr.add(STRING_WRAPPER_CELL_OFF) as *mut *mut u8).write(cell);
    }
    ptr
}

/// Read the inner Str cell pointer. Borrow — caller does not own.
///
/// # Safety
/// `ptr` must be a live StringWrapper cell.
#[inline]
pub unsafe fn string_wrapper_cell(ptr: *mut u8) -> *mut u8 {
    unsafe { (ptr.add(STRING_WRAPPER_CELL_OFF) as *const *mut u8).read() }
}

/// Direct free of a StringWrapper block. Releases the wrapper's own
/// reference on the inner Str cell (routed through the universal
/// dispatcher so rc + tag walk happens uniformly), then frees the
/// wrapper block itself. Called by the dispatcher AFTER
/// `__torajs_rc_dec` returned 1 on the wrapper.
///
/// # Safety
/// `p` must be either NULL or a valid StringWrapper block whose
/// refcount just reached zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_string_wrapper_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    unsafe { __torajs_cycle_unbuffer(p) };
    let cell = unsafe { string_wrapper_cell(p as *mut u8) };
    if !cell.is_null() {
        unsafe { __torajs_value_drop_heap(cell as *mut c_void) };
    }
    unsafe {
        release_wrapper_props(p as *mut u8);
        libc_free(p);
    };
}

/// Rc-aware drop. Decrements; frees + inner-cell-releases iff this
/// was the last owner.
///
/// # Safety
/// `p` must be either NULL or a valid pointer to a StringWrapper
/// block with a live refcount header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_string_wrapper_drop_rc(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } != 0 {
        unsafe { __torajs_string_wrapper_drop(p) };
    } else {
        unsafe { __torajs_cycle_buffer(p) };
    }
}

// ============================================================
// BooleanWrapper — new Boolean(b), [[BooleanData]] = b
// ============================================================

/// Allocate a fresh Boolean wrapper with refcount=1 and
/// `[[BooleanData]] = val` (spec §20.3.1.1). `val` is 0 (false) or
/// 1 (true); other bit patterns are canonicalized to 0/1 at read.
///
/// # Safety
/// The returned pointer owns a fresh refcount=1 block; the caller is
/// responsible for eventually dropping it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_boolean_wrapper_new(val: u8) -> *mut u8 {
    let ptr = unsafe { libc_malloc(BOOLEAN_WRAPPER_SIZE) } as *mut u8;
    unsafe {
        init_wrapper_header(ptr, Tag::BooleanWrapper);
        (ptr.add(BOOLEAN_WRAPPER_VALUE_OFF) as *mut u8).write(if val != 0 { 1 } else { 0 });
        // Zero the remaining 7 pad bytes so tag inspectors reading
        // wider than a byte see stable content.
        for i in 1..8 {
            (ptr.add(BOOLEAN_WRAPPER_VALUE_OFF + i)).write(0);
        }
    }
    ptr
}

/// Read `[[BooleanData]]` (0 or 1).
///
/// # Safety
/// `ptr` must point to a live BooleanWrapper cell.
#[inline]
pub unsafe fn boolean_wrapper_value(ptr: *mut u8) -> u8 {
    unsafe { (ptr.add(BOOLEAN_WRAPPER_VALUE_OFF) as *const u8).read() }
}

/// Direct free of a BooleanWrapper block. NULL-safe. Called by the
/// dispatcher AFTER `__torajs_rc_dec` returned 1.
///
/// # Safety
/// `p` must be either NULL or a valid BooleanWrapper block whose
/// refcount just reached zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_boolean_wrapper_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    unsafe {
        __torajs_cycle_unbuffer(p);
        release_wrapper_props(p as *mut u8);
        libc_free(p);
    };
}

/// Rc-aware drop.
///
/// # Safety
/// `p` must be either NULL or a valid pointer to a BooleanWrapper
/// block with a live refcount header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_boolean_wrapper_drop_rc(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } != 0 {
        unsafe { __torajs_boolean_wrapper_drop(p) };
    } else {
        unsafe { __torajs_cycle_buffer(p) };
    }
}

// ============================================================
// Unit tests — verify header init + read helpers + rc balance
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Stubs so the test binary links without staticlib injection.
    // Layer-1 dep sites (mmalloc / rc / value-drop) resolve against
    // sibling rlibs in the workspace, but their staticlib-only
    // `__torajs_libc_malloc` / `__torajs_libc_free` /
    // `__torajs_value_drop_heap` symbols are not visible to cargo test.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __torajs_libc_malloc(size: usize) -> *mut c_void {
        // SAFETY: cargo test host — plain libc malloc is fine.
        unsafe extern "C" {
            fn malloc(size: usize) -> *mut c_void;
        }
        unsafe { malloc(size) }
    }
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __torajs_libc_free(p: *mut c_void) {
        unsafe extern "C" {
            fn free(p: *mut c_void);
        }
        unsafe { free(p) }
    }
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __torajs_value_drop_heap(_child: *mut c_void) {
        // no-op stub — real dispatcher lives in libtorajs_value_drop.a
    }
    // torajs-rc's `__torajs_rc_dec` notifies the weak registry on
    // death; the shipped hook lives in libtorajs_weak.a and is not
    // linked into the cargo-test binary. No-op stub — same convention
    // as torajs-value-drop / torajs-anyvalue integration tests.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}
    // Cycle-root buffer hooks (RFC 20260717 blade 3) live in
    // libtorajs_cycle.a — no-op stubs, same convention as above.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __torajs_cycle_buffer(_p: *mut c_void) {}
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __torajs_cycle_unbuffer(_p: *mut c_void) {}

    #[test]
    fn number_wrapper_new_and_read() {
        unsafe {
            let p = __torajs_number_wrapper_new(3.14);
            assert!(!p.is_null());
            let hdr = &*(p as *const HeapHeader);
            assert_eq!(hdr.refcount, 1);
            assert_eq!(hdr.type_tag, Tag::NumberWrapper as u16);
            assert_eq!(hdr.flags, 0);
            let v = number_wrapper_value(p);
            assert!((v - 3.14).abs() < 1e-9);
            __torajs_number_wrapper_drop_rc(p as *mut c_void);
        }
    }

    #[test]
    fn boolean_wrapper_new_and_read() {
        unsafe {
            let p_true = __torajs_boolean_wrapper_new(1);
            let p_false = __torajs_boolean_wrapper_new(0);
            assert_eq!(
                (*(p_true as *const HeapHeader)).type_tag,
                Tag::BooleanWrapper as u16
            );
            assert_eq!(boolean_wrapper_value(p_true), 1);
            assert_eq!(boolean_wrapper_value(p_false), 0);
            __torajs_boolean_wrapper_drop_rc(p_true as *mut c_void);
            __torajs_boolean_wrapper_drop_rc(p_false as *mut c_void);
        }
    }

    #[test]
    fn boolean_wrapper_canonicalizes_nonzero() {
        unsafe {
            let p = __torajs_boolean_wrapper_new(255);
            assert_eq!(boolean_wrapper_value(p), 1);
            __torajs_boolean_wrapper_drop_rc(p as *mut c_void);
        }
    }

    #[test]
    fn string_wrapper_null_cell_ok() {
        unsafe {
            let p = __torajs_string_wrapper_new(core::ptr::null_mut());
            assert!(!p.is_null());
            let hdr = &*(p as *const HeapHeader);
            assert_eq!(hdr.type_tag, Tag::StringWrapper as u16);
            assert!(string_wrapper_cell(p).is_null());
            __torajs_string_wrapper_drop_rc(p as *mut c_void);
        }
    }
}
