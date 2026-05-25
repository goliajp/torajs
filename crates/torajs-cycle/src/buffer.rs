//! Buffer of potential cycle roots (PURPLE candidates).
//!
//! Port of the `g_buffer` + `__torajs_cycle_buffer` +
//! `__torajs_cycle_unbuffer` section of `runtime_cycle.c`. The
//! `rc_dec` / Obj-walk_blk hooks push candidates here when an rc
//! transition stays positive on a cyclic-shape type; the auto-collect
//! threshold + the explicit `gc()` trigger drain the buffer through
//! the trial-deletion algorithm (in `crate::collect`).
//!
//! ## `static mut` 替代 → `AtomicPtr`
//!
//! Same rationale as `crate::registry` / `torajs-arr::pool` —
//! tora runtime is single-threaded, but Rust 2024 deprecates raw
//! `static mut`. `AtomicPtr<*mut c_void>` + `AtomicU32` + `Relaxed`
//! compiles to identical loads/stores while keeping `&'static`
//! APIs sound + pre-paying the future multi-threaded story.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::layout::{COLOR_PURPLE, FLAG_BUFFERED, HeapHeader, has_walkable_children, set_color};

/// Threshold at which `cycle_buffer` triggers a synchronous collect
/// before returning. Mirrors `runtime_cycle.c::CYCLE_AUTO_COLLECT_
/// THRESHOLD = 1024`. Keeps long-running programs that never call
/// `gc()` explicitly from leaking cycle roots unbounded.
pub const CYCLE_AUTO_COLLECT_THRESHOLD: u32 = 1024;

/// Pointer to the buffer's heap-allocated backing array. NULL until
/// first push; `realloc`'d to `g_buffer_cap` × `sizeof(*mut c_void)`
/// on growth.
static G_BUFFER: AtomicPtr<*mut c_void> = AtomicPtr::new(ptr::null_mut());

/// Number of valid entries in `G_BUFFER` (NULL holes are valid —
/// `cycle_unbuffer` may zero a slot when an object normal-drops out
/// of the cycle path).
static G_BUFFER_LEN: AtomicU32 = AtomicU32::new(0);

/// Allocated capacity. Doubles from 64 on growth.
static G_BUFFER_CAP: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    /// torajs-mmalloc libc-compat realloc — v0.7-A2 step 6b cutover.
    /// Closed-loop within cycle (this is cycle's own root buffer; the
    /// cross-crate `free` in collect.rs is the separate finale path).
    #[link_name = "__torajs_libc_realloc"]
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
}

/// Append `p` to the buffer, reallocating + doubling capacity when
/// the current cap is reached. Initial cap = 64; doubles thereafter.
/// Single-threaded — callers don't need to lock.
fn buffer_push(p: *mut c_void) {
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    let cap = G_BUFFER_CAP.load(Ordering::Relaxed);
    if len == cap {
        let new_cap = if cap == 0 { 64 } else { cap * 2 };
        let cur = G_BUFFER.load(Ordering::Relaxed);
        let new_buf = unsafe {
            realloc(
                cur as *mut c_void,
                (new_cap as usize) * core::mem::size_of::<*mut c_void>(),
            )
        } as *mut *mut c_void;
        G_BUFFER.store(new_buf, Ordering::Relaxed);
        G_BUFFER_CAP.store(new_cap, Ordering::Relaxed);
    }
    let buf = G_BUFFER.load(Ordering::Relaxed);
    unsafe { *buf.add(len as usize) = p };
    G_BUFFER_LEN.store(len + 1, Ordering::Relaxed);
}

/// Iterate every live (non-NULL) buffered candidate. Helper for the
/// `collect` module — keeps buffer iteration semantics confined to
/// this module so the algorithm code in `collect.rs` doesn't poke
/// at `G_BUFFER` directly.
#[inline]
pub fn for_each<F: FnMut(*mut c_void)>(mut f: F) {
    let buf = G_BUFFER.load(Ordering::Relaxed);
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    for i in 0..len as usize {
        let p = unsafe { *buf.add(i) };
        if !p.is_null() {
            f(p);
        }
    }
}

/// Walk EVERY slot — including NULLs — exposing the raw pointer
/// + index. Needed by `cycle_collect`'s third pass which must read
/// each slot's pointer to look at its color flag and possibly clear
/// the `FLAG_BUFFERED` bit before potentially recursing into
/// `collect_white`.
#[inline]
pub fn for_each_with_index<F: FnMut(usize, *mut c_void)>(mut f: F) {
    let buf = G_BUFFER.load(Ordering::Relaxed);
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    for i in 0..len as usize {
        let p = unsafe { *buf.add(i) };
        f(i, p);
    }
}

/// Reset the buffer length to 0 (capacity preserved — next push
/// won't realloc until cap is reached again). Called at the end
/// of `cycle_collect` to discard processed entries.
#[inline]
pub fn reset_len() {
    G_BUFFER_LEN.store(0, Ordering::Relaxed);
}

/// Current buffer length, for the early-out fast path in
/// `cycle_collect` (skip the whole thing if 0).
#[inline]
pub fn len() -> u32 {
    G_BUFFER_LEN.load(Ordering::Relaxed)
}

/// Called from rc_dec / Obj-walk_blk's else-branch when the rc
/// stayed positive on a cyclic-shape type. Marks PURPLE + pushes
/// into the buffer (with BUFFERED gate so duplicates skip). Cheap
/// fast-path: if already buffered, return without touching anything.
///
/// Auto-triggers `cycle_collect` when the buffer length crosses
/// the auto-collect threshold (V3-10 behavior).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_buffer(p: *mut c_void) {
    if !unsafe { has_walkable_children(p) } {
        return;
    }
    let h = p as *mut HeapHeader;
    if unsafe { (*h).flags } & FLAG_BUFFERED != 0 {
        return;
    }
    unsafe {
        set_color(h, COLOR_PURPLE);
        (*h).flags |= FLAG_BUFFERED;
    }
    buffer_push(p);
    if G_BUFFER_LEN.load(Ordering::Relaxed) >= CYCLE_AUTO_COLLECT_THRESHOLD {
        unsafe { crate::collect::__torajs_cycle_collect() };
    }
}

/// Scrub `p` from the cycle root buffer before its memory is freed
/// via the inline class drop, the array element walk, or
/// `value_drop_heap`'s default branch. Without this, an object that
/// was added as a cycle candidate but later normal-dropped to rc=0
/// leaves a dangling pointer in the buffer; the next collect (or
/// the end-of-program exit-drain) dereferences it and segfaults.
///
/// `#[inline(never)]` mirrors the C `__attribute__((noinline))`
/// workaround: LLVM 22 -O3 LTO miscompiles the call site inside
/// `value_drop_heap` when this body is inlined cross-TU. Keep the
/// call as a real function boundary until the upstream issue is
/// narrowed.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn __torajs_cycle_unbuffer(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let h = p as *mut HeapHeader;
    if unsafe { (*h).flags } & FLAG_BUFFERED == 0 {
        return;
    }
    unsafe { (*h).flags &= !FLAG_BUFFERED };
    let buf = G_BUFFER.load(Ordering::Relaxed);
    let len = G_BUFFER_LEN.load(Ordering::Relaxed);
    for i in 0..len as usize {
        unsafe {
            if *buf.add(i) == p {
                *buf.add(i) = ptr::null_mut();
            }
        }
    }
}
