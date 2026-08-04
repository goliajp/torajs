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
//! ## Self-hosted hash (v0.7-A5 step ⑥)
//!
//! 256-bucket open-chaining hash, single global `torajs-mutex`
//! protecting the bucket array + the per-bucket linked lists. Bucket
//! index = MurmurHash3 `fmix64` of the fn pointer, masked to 8 bits.
//! Replaces the v0.5 `std::sync::OnceLock<Mutex<HashMap<usize, usize>>>`
//! placeholder, which dragged libdispatch (`_dispatch_semaphore_*` for
//! OnceLock init), the libc malloc family (HashMap alloc), libc panic
//! machinery (`__tlv_atexit` / `_abort` / `___error`), and the RNG seed
//! for `DefaultHasher` (`_CCRandomGenerateBytes`) into every binary
//! that touched `fn.prop = v`. With this rewrite the only
//! cross-crate surface left is the `Box`/`alloc` path for `Node`,
//! which routes through the mmalloc `#[global_allocator]` already
//! installed by `torajs-mmalloc` — no libc dep.
//!
//! 256 buckets is the textbook starting point for a tiny side table:
//! the JS programs we run rarely have more than a handful of FnSig
//! fns that gain props at runtime, so chains stay length 1 in
//! practice. If a workload pushes them above ~4 we can grow
//! incrementally (resize is a follow-up; the bucket array sizing is
//! intentionally narrow for now).

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr;

use torajs_mutex::Mutex;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
}

const ANY_UNDEF_TAG: u64 = 5;
const BUCKET_COUNT: usize = 256;
const BUCKET_MASK: usize = BUCKET_COUNT - 1;

/// Closure-cell props-slot offset (mirrors
/// `torajs_anyvalue::member_get::CLOSURE_PROPS_OFF` — header 8 +
/// fn_addr 8 + drop_fn 8).
const CLOSURE_PROPS_OFF: usize = 24;

/// Open-chaining hash node. `next` walks the bucket's collision
/// chain; `(fn_ptr, dynobj)` is the entry payload. `cell` — RFC
/// 20260804-fnprops-canonical-cell: once the fn's canonical forward
/// cell is minted it becomes the ONE authoritative props slot
/// (`cell + CLOSURE_PROPS_OFF`), and this table delegates to it so
/// the FnSig static spelling and the any-lane cell spelling read and
/// write the same bag. `cell == 0` = un-wrapped fn, `dynobj` bag
/// mode as before. Exactly one of (dynobj, cell) is non-zero.
#[repr(C)]
struct Node {
    next: *mut Node,
    fn_ptr: usize,
    dynobj: usize,
    cell: usize,
}

/// Bucket array — 256 head pointers. Wrapped in `UnsafeCell` so we
/// can take `&mut` to it under the global mutex without inheriting
/// `&mut self`-style aliasing constraints. Access is sound because
/// every read / write goes through `FNPROPS_LOCK`.
struct BucketArray(UnsafeCell<[*mut Node; BUCKET_COUNT]>);

// SAFETY: every access to `BucketArray.0` is gated by `FNPROPS_LOCK`.
unsafe impl Sync for BucketArray {}

static BUCKETS: BucketArray = BucketArray(UnsafeCell::new([ptr::null_mut(); BUCKET_COUNT]));
static FNPROPS_LOCK: Mutex<()> = Mutex::new(());

/// MurmurHash3 64-bit finalizer (`fmix64`). Two xor-shifts + two
/// odd-multiplier multiplies. Strong avalanche for adjacent
/// pointer-sized inputs, no table, no state. Same exact mix
/// CPython uses for `id()`-derived hashing on 64-bit builds.
#[inline]
fn fmix64(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    h
}

#[inline]
fn bucket_of(fn_ptr: usize) -> usize {
    (fmix64(fn_ptr as u64) as usize) & BUCKET_MASK
}

/// Find the node for `fn_ptr` inside bucket `b`, or null if absent.
/// Caller must hold `FNPROPS_LOCK`.
#[inline]
unsafe fn find_in_bucket(
    buckets: &[*mut Node; BUCKET_COUNT],
    b: usize,
    fn_ptr: usize,
) -> *mut Node {
    let mut cur = buckets[b];
    while !cur.is_null() {
        // SAFETY: chain pointers are either null or `Box::leak`'d
        // — they outlive the program. Concurrent mutation is barred
        // by `FNPROPS_LOCK`.
        unsafe {
            if (*cur).fn_ptr == fn_ptr {
                return cur;
            }
            cur = (*cur).next;
        }
    }
    ptr::null_mut()
}

/// Get-or-create the node for `fn_ptr`. The miss path pushes a fresh
/// `Box::leak`'d empty node (no bag until the first write) onto the
/// bucket head. Caller must hold `FNPROPS_LOCK`.
#[inline]
unsafe fn intern_node(buckets: &mut [*mut Node; BUCKET_COUNT], fn_ptr: usize) -> *mut Node {
    let b = bucket_of(fn_ptr);
    let existing = unsafe { find_in_bucket(buckets, b, fn_ptr) };
    if !existing.is_null() {
        return existing;
    }
    // `Box::leak` to give the node program-lifetime ownership; the
    // C side table this replaces never freed its entries either.
    let node = Box::leak(Box::new(Node {
        next: buckets[b],
        fn_ptr,
        dynobj: 0,
        cell: 0,
    }));
    buckets[b] = node as *mut Node;
    node
}

/// The authoritative props slot for a delegated node: the canonical
/// cell's own `props_dynobj` field. `dynobj_set` writes a grown bag
/// back through this slot, which is exactly the cell-spelling slot —
/// both spellings stay on one bag by construction.
#[inline]
fn cell_props_slot(cell: usize) -> *mut *mut c_void {
    (cell + CLOSURE_PROPS_OFF) as *mut *mut c_void
}

/// Bind `fn_ptr`'s props storage to its canonical forward cell (RFC
/// 20260804-fnprops-canonical-cell). Called once from the cell's
/// lazy-mint block. A bag the FnSig spelling already filled migrates
/// into the cell's (still empty at mint) props slot, so earlier
/// writes stay visible. Idempotent — the canonical cell is a per-fn
/// singleton, so a repeat bind carries the same cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fnprops_bind_cell(fn_ptr: *mut c_void, cell: *mut c_void) {
    if fn_ptr.is_null() || cell.is_null() {
        return;
    }
    let _g = FNPROPS_LOCK.lock();
    // SAFETY: lock held → exclusive access to BUCKETS.0.
    let buckets = unsafe { &mut *BUCKETS.0.get() };
    let node = unsafe { intern_node(buckets, fn_ptr as usize) };
    unsafe {
        if (*node).cell == cell as usize {
            return;
        }
        if (*node).dynobj != 0 {
            *cell_props_slot(cell as usize) = (*node).dynobj as *mut c_void;
            (*node).dynobj = 0;
        }
        (*node).cell = cell as usize;
    }
}

/// `fn.x = value` (FnSig spelling) — write through the fn's
/// authoritative props slot: the canonical cell's when bound, the
/// node's own bag otherwise. `dynobj_set` may grow and reassign the
/// bag; the slot it wrote through keeps the fresh pointer either way.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fnprops_set(
    fn_ptr: *mut c_void,
    key: *const c_void,
    tag: i64,
    value: i64,
) {
    let _g = FNPROPS_LOCK.lock();
    let buckets = unsafe { &mut *BUCKETS.0.get() };
    let node = unsafe { intern_node(buckets, fn_ptr as usize) };
    unsafe {
        if (*node).cell != 0 {
            __torajs_dynobj_set(
                cell_props_slot((*node).cell),
                key as *const u8,
                tag as u64,
                value as u64,
            );
        } else {
            if (*node).dynobj == 0 {
                (*node).dynobj = __torajs_dynobj_alloc() as usize;
            }
            let mut bag = (*node).dynobj as *mut c_void;
            __torajs_dynobj_set(&mut bag, key as *const u8, tag as u64, value as u64);
            (*node).dynobj = bag as usize;
        }
    }
}

/// Resolve the fn's current bag for a read — the cell's props when
/// bound, the node's own bag otherwise, null when neither exists.
#[inline]
fn lookup_bag(fn_ptr: *mut c_void) -> *mut c_void {
    let _g = FNPROPS_LOCK.lock();
    // SAFETY: lock held → exclusive (shared-immutable here) access.
    let buckets = unsafe { &*BUCKETS.0.get() };
    let b = bucket_of(fn_ptr as usize);
    let n = unsafe { find_in_bucket(buckets, b, fn_ptr as usize) };
    if n.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if (*n).cell != 0 {
            *cell_props_slot((*n).cell)
        } else {
            (*n).dynobj as *mut c_void
        }
    }
}

/// `fn.x` — return the slot's tag, or `ANY_UNDEF` if no fnprops
/// entry / no key.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fnprops_get_tag(fn_ptr: *mut c_void, key: *const c_void) -> u64 {
    let dynobj = lookup_bag(fn_ptr);
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
    let dynobj = lookup_bag(fn_ptr);
    if dynobj.is_null() {
        return 0;
    }
    unsafe { __torajs_dynobj_get_value(dynobj, key as *const u8) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmix64_zero_input_diffuses() {
        // The textbook fmix64 of 0 is 0 — well-known property of the
        // bit-mixer. Any non-zero ptr scatters into a non-zero slot.
        assert_eq!(fmix64(0), 0);
        assert_ne!(fmix64(1), fmix64(2));
        assert_ne!(fmix64(0x1000), fmix64(0x1008));
    }

    #[test]
    fn bucket_of_is_in_range() {
        for &p in &[0usize, 1, 0x1000, 0xdead_beef, 0xffff_ffff_ffff_ffff] {
            assert!(bucket_of(p) < BUCKET_COUNT);
        }
    }

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
