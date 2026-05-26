//! Array side-table for `arr.x = v` custom properties.
//!
//! Port of `runtime_str.c::__torajs_arrprops_*` (P4.1-i, 2026-05-23).
//!
//! Arrays normally don't have non-index properties, but JS allows
//! `arr.customField = ...` for any array. Rather than bloating every
//! array header with an unused dynobj slot (would shift `ARR_DATA_OFF`
//! and break ~28 call sites + cost 8 bytes per array), we keep a
//! global pointer-keyed side-table that's empty for the common case.
//!
//! Layout:
//! - 256 hash buckets, FxHash-equivalent murmur over the array pointer
//! - Each bucket is a singly-linked list of `Node { arr_ptr, dynobj, next }`
//! - `dynobj` is a tagged Any-keyed key-value store (in C runtime_str.c,
//!   not Rust yet — set/get/drop_entry call into C via cross-tier extern)
//!
//! ## Drop hook
//!
//! [`__torajs_arrprops_drop_entry`] is invoked from
//! `crate::drop::__torajs_arr_drop` / `__torajs_arr_drop_any` on
//! refcount→0. It walks the bucket, removes the matching node, and
//! releases the dynobj via `__torajs_value_drop_heap`. Arrays that
//! never had `.x = v` written: bucket walk finds nothing → cheap
//! no-op (single hash + load + null check).
//!
//! ## Single-threaded by contract
//!
//! Same rationale as `crate::pool` — tora runtime is single-threaded;
//! `AtomicPtr` + `Ordering::Relaxed` compiles to the same instructions
//! as `static mut` while satisfying Rust 2024 `static_mut_refs` lint.

use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

/// 256 hash buckets — matches C `__TORAJS_ARRPROPS_BUCKETS`.
const ARRPROPS_BUCKETS: usize = 256;

#[repr(C)]
struct Node {
    arr_ptr: *mut c_void,
    dynobj: *mut c_void,
    next: *mut Node,
}

/// Bucket head pointers. NULL = empty bucket.
static TABLE: [AtomicPtr<Node>; ARRPROPS_BUCKETS] =
    [const { AtomicPtr::new(core::ptr::null_mut()) }; ARRPROPS_BUCKETS];

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);

    /// Cross-tier — runtime_str.c's dynamic-property object alloc.
    /// (Pure C for now; ports to torajs-dynobj in P4.2.)
    fn __torajs_dynobj_alloc() -> *mut c_void;

    /// Cross-tier — set a key-value entry on `*dynobj_ptr`. May
    /// reallocate the dynobj (linear-probing hash table), so it
    /// takes a `&mut *mut c_void` to write back the new pointer.
    fn __torajs_dynobj_set(dynobj_ptr: *mut *mut c_void, key: *const c_void, tag: u64, value: u64);

    /// Cross-tier — read the tag (or ANY_UNDEF=5 on miss).
    fn __torajs_dynobj_get_tag(dynobj: *mut c_void, key: *const c_void) -> u64;

    /// Cross-tier — read the value (or 0 on miss).
    fn __torajs_dynobj_get_value(dynobj: *mut c_void, key: *const c_void) -> u64;

    /// Cross-tier — universal heap value dropper. Used to release the
    /// dynobj on drop_entry.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Splitmix64-style finalizer over the array pointer. Same shape as
/// C `__torajs_arrprops_hash` — keeps bucket distribution flat even
/// when allocator returns sequential addresses.
#[inline]
fn hash(p: *mut c_void) -> u32 {
    let mut x = p as usize as u64;
    x = (x ^ (x >> 33)).wrapping_mul(0xff51afd7ed558ccd);
    x = (x ^ (x >> 33)).wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    (x % ARRPROPS_BUCKETS as u64) as u32
}

/// Find the node for `arr_ptr` in its bucket, or `null` if absent.
unsafe fn find(arr_ptr: *mut c_void) -> *mut Node {
    let h = hash(arr_ptr) as usize;
    let mut n = TABLE[h].load(Ordering::Relaxed);
    while !n.is_null() {
        unsafe {
            if (*n).arr_ptr == arr_ptr {
                return n;
            }
            n = (*n).next;
        }
    }
    core::ptr::null_mut()
}

/// Find or insert a node for `arr_ptr`. Newly inserted node has
/// `dynobj = NULL`; caller (`set`) allocates the dynobj lazily.
unsafe fn intern(arr_ptr: *mut c_void) -> *mut Node {
    unsafe {
        let existing = find(arr_ptr);
        if !existing.is_null() {
            return existing;
        }
        let h = hash(arr_ptr) as usize;
        let n = malloc(core::mem::size_of::<Node>()) as *mut Node;
        (*n).arr_ptr = arr_ptr;
        (*n).dynobj = core::ptr::null_mut();
        // Push at head of bucket chain.
        (*n).next = TABLE[h].load(Ordering::Relaxed);
        TABLE[h].store(n, Ordering::Relaxed);
        n
    }
}

/// `arr.key = (tag, value)` — lazily allocate the dynobj on first set.
///
/// # Safety
/// `arr_ptr` is an array heap pointer (lifetime ≥ the calling scope);
/// `key` is a Str pointer (rodata-baked or rc'd).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_set(
    arr_ptr: *mut c_void,
    key: *const c_void,
    tag: i64,
    value: i64,
) {
    unsafe {
        let n = intern(arr_ptr);
        if (*n).dynobj.is_null() {
            (*n).dynobj = __torajs_dynobj_alloc();
        }
        __torajs_dynobj_set(&raw mut (*n).dynobj, key, tag as u64, value as u64);
    }
}

/// Read `arr.key`'s tag, or `ANY_UNDEF=5` if not set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_get_tag(
    arr_ptr: *mut c_void,
    key: *const c_void,
) -> u64 {
    unsafe {
        let n = find(arr_ptr);
        if n.is_null() || (*n).dynobj.is_null() {
            return 5; // ANY_UNDEF
        }
        __torajs_dynobj_get_tag((*n).dynobj, key)
    }
}

/// Read `arr.key`'s value, or 0 if not set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_get_value(
    arr_ptr: *mut c_void,
    key: *const c_void,
) -> u64 {
    unsafe {
        let n = find(arr_ptr);
        if n.is_null() || (*n).dynobj.is_null() {
            return 0;
        }
        __torajs_dynobj_get_value((*n).dynobj, key)
    }
}

/// Drop hook — called from `arr_drop` / `arr_drop_any` on rc→0.
/// Walks the bucket chain, removes + frees the matching node + dec's
/// the dynobj's refcount. Arrays that never had props written: cheap
/// no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_drop_entry(arr_ptr: *mut c_void) {
    let h = hash(arr_ptr) as usize;
    let mut prev: *mut AtomicPtr<Node> = &TABLE[h] as *const _ as *mut _;
    unsafe {
        let mut n = (*prev).load(Ordering::Relaxed);
        while !n.is_null() {
            if (*n).arr_ptr == arr_ptr {
                // Unlink: write `next` into the slot that pointed at `n`.
                let next = (*n).next;
                // First entry case — prev points to TABLE[h] head;
                // mid-chain case — prev points to predecessor's `next`
                // field. Both updated via AtomicPtr / raw ptr store.
                if prev as *const _ == &TABLE[h] as *const _ {
                    TABLE[h].store(next, Ordering::Relaxed);
                } else {
                    // prev is &(predecessor.next) cast to AtomicPtr; raw
                    // write since predecessor.next is a regular *mut Node.
                    *(prev as *mut *mut Node) = next;
                }
                if !(*n).dynobj.is_null() {
                    __torajs_value_drop_heap((*n).dynobj);
                }
                free(n as *mut c_void);
                return;
            }
            // Advance: prev now tracks `&n.next`, which is a regular
            // *mut Node — alias it via AtomicPtr for the head-vs-mid
            // check above (same byte layout under single-threaded model).
            prev = &raw mut (*n).next as *mut AtomicPtr<Node>;
            n = (*n).next;
        }
    }
}
