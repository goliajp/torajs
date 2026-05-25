//! Substr layout + Substr pool + `__torajs_substr_create` /
//! `__torajs_substr_drop` extern "C" wrappers.
//!
//! Substr is the **view type** counterpart to [`crate::alloc::StrBlock`].
//! It borrows bytes from an owned parent `Str` instead of carrying its
//! own payload — same shape as Swift's `String` / `Substring` or Rust's
//! `String` / `&str`. Keeping the view type physically separate from
//! `Str` means owned-Str byte access stays a single GEP (no indirection)
//! while view access pays one extra load (`parent → +16 → bytes`) only
//! when the SSA `Type::Substr` is explicit.
//!
//! ## Layout
//!
//! ```text
//! Substr = [header:8][len:8][parent_ptr:8][offset:8]   total 32
//!          ^             ^         ^             ^
//!          0             8         16            24
//! ```
//!
//! - `header` mirrors [`torajs_rc::HeapHeader`] exactly: refcount=1 at
//!   alloc, `type_tag=Tag::Str` (SSA `Type::Substr` is still a "str" at
//!   the type-tag layer; the view-vs-owned distinction lives in the
//!   SSA `Type`, not the runtime tag), `flags=0`.
//! - `parent_ptr` is an owned `Str` heap pointer. Every live Substr
//!   holds **one refcount** on its parent (incremented at
//!   [`SubstrBlock::create`], decremented at
//!   [`SubstrBlock::drop_pool_aware`]). Parents stay alive as long as
//!   any view into them exists.
//! - `offset` is the byte index into `parent.bytes` where the view
//!   starts. Combined with `len`, this is a slice into the parent.
//!
//! ## INLINE flag
//!
//! [`FLAG_SUBSTR_INLINE`] (bit 0 of `HeapHeader::flags`) marks a Substr
//! struct that lives **inside another allocation** — typically the
//! variable-size single block emitted by `__torajs_str_split` (header +
//! N pointer slots + N 32-byte inline substr structs). Inline substr
//! drop must:
//!
//! 1. Decrement the parent's refcount (each inline substr still owns
//!    one).
//! 2. **NOT** touch its own refcount.
//! 3. **NOT** push to the Substr pool or call `libc::free` on its own
//!    storage — the enclosing block's drop handles all storage reclaim
//!    in one go.
//!
//! Standalone (non-inline) substr drop is the symmetric case:
//! dec own refcount; on reach-zero, dec parent and pool-push or
//! libc-free self.

use core::ptr::{self, NonNull};
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use torajs_rc::{__torajs_rc_dec, __torajs_rc_inc, HeapHeader, Tag};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat malloc — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(size: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(ptr: *mut c_void);
}

// ============================================================
// Layout constants
// ============================================================

/// Total bytes for one Substr struct. ssa_inkwell GEPs against this
/// constant when allocating views; the runtime_str.c macro
/// `__TORAJS_SUBSTR_SIZE` mirrors it.
pub const SUBSTR_SIZE: usize = 32;

/// Byte offset of the `len` u64 field (mirrors `__TORAJS_SUBSTR_LEN`).
pub const SUBSTR_LEN_OFF: usize = 8;

/// Byte offset of the parent-pointer field (mirrors
/// `__TORAJS_SUBSTR_PARENT_OFF`).
pub const SUBSTR_PARENT_OFF: usize = 16;

/// Byte offset of the `offset` u64 field (mirrors
/// `__TORAJS_SUBSTR_OFFSET_OFF`).
pub const SUBSTR_OFFSET_OFF: usize = 24;

/// `HeapHeader::flags` bit marking an inline Substr struct (embedded
/// inside a larger allocation such as a split-block tail). Disjoint
/// from every [`torajs_rc::FLAG_*`] constant; meaningful only when the
/// containing block's `type_tag` is `Tag::Str` (and the block is, by
/// SSA type, a Substr view). Mirrors C `__TORAJS_FLAG_SUBSTR_INLINE`.
pub const FLAG_SUBSTR_INLINE: u16 = 1 << 0;

// ============================================================
// SubstrBlock newtype
// ============================================================

/// Owned Substr heap block (or pointer to an inline substr struct
/// inside a split-block tail when [`FLAG_SUBSTR_INLINE`] is set on
/// the header).
///
/// `NonNull<u8>` transparent newtype, same ABI shape as the legacy C
/// `void *` Substr pointer. **Not `Copy` / `Clone`** by design so each
/// value tracks single ownership; `must_use` on the constructor catches
/// the common "forget to drop" mistake.
#[repr(transparent)]
#[derive(Debug)]
pub struct SubstrBlock(pub NonNull<u8>);

impl SubstrBlock {
    /// Allocate (or pool-pop) a Substr view of `len` bytes starting at
    /// `offset` into `parent`. Bumps `parent`'s refcount so its bytes
    /// stay alive while the view exists.
    ///
    /// # Safety
    ///
    /// `parent` must be a live owned Str heap pointer (or null — null
    /// is forwarded to `__torajs_rc_inc` which is null-safe; the
    /// resulting Substr is technically valid layout-wise but reading
    /// its bytes is UB).
    #[inline]
    #[must_use = "SubstrBlock owns a heap allocation; ignore the value and the block leaks"]
    pub unsafe fn create(parent: *mut c_void, offset: u64, len: u64) -> Self {
        let raw = match pool_pop() {
            Some(p) => p,
            None => {
                // SAFETY: libc malloc with the fixed SUBSTR_SIZE. Result
                // is non-null in non-OOM regimes; OOM aborts via
                // `.expect`.
                let p = unsafe { malloc(SUBSTR_SIZE) } as *mut u8;
                NonNull::new(p).unwrap_or_else(|| torajs_abort::abort_with(b"OOM in Substr alloc"))
            }
        };

        // SAFETY: `raw` is freshly allocated or freshly pool-popped;
        // exclusively owned for the duration of this fn. We init the
        // full 32-byte struct before any other code can observe it.
        unsafe {
            let header_ptr = raw.as_ptr() as *mut HeapHeader;
            // refcount=1, type_tag=Str, flags=0 — INLINE flag is set
            // by `__torajs_str_split` at allocation time on the split-
            // block tail substrs; create() always produces standalone.
            header_ptr.write(HeapHeader {
                refcount: 1,
                type_tag: Tag::Str as u16,
                flags: 0,
            });
            (raw.as_ptr().add(SUBSTR_LEN_OFF) as *mut u64).write(len);
            (raw.as_ptr().add(SUBSTR_PARENT_OFF) as *mut *mut c_void).write(parent);
            (raw.as_ptr().add(SUBSTR_OFFSET_OFF) as *mut u64).write(offset);
        }

        // SAFETY: `__torajs_rc_inc` is null-safe; non-null parent must
        // point to a live HeapHeader per caller contract.
        unsafe { __torajs_rc_inc(parent) };

        Self(raw)
    }

    /// Drop a Substr view. Two cases, distinguished by
    /// [`FLAG_SUBSTR_INLINE`]:
    ///
    /// 1. **inline** (flag set): dec parent only; the substr's own
    ///    storage is reclaimed by the enclosing block's drop.
    /// 2. **standalone** (flag clear): dec own refcount; on
    ///    reach-zero, dec parent + pool-push or libc-free self.
    ///
    /// **Implementation note**: reads `flags` and `parent` as plain
    /// `u16` / pointer values via raw-pointer loads (NOT via the
    /// `&mut HeapHeader` reborrow). Holding a Rust borrow across a
    /// call into `__torajs_rc_dec` (which itself materializes a
    /// `&mut HeapHeader` on the same memory) would alias two mutable
    /// references — Rust 2024's stricter aliasing model gives the
    /// optimizer permission to miscompile that, and in release mode
    /// it can manifest as a SIGSEGV deep inside the dec path. By
    /// holding only `u16` / `*mut c_void` *values* across the call,
    /// no Rust reference is live during the dec.
    #[inline]
    pub fn drop_pool_aware(self) {
        // SAFETY: contract is self.0 points at a valid Substr block.
        // We read the two header fields we need as plain values, NOT
        // as a `&mut HeapHeader` reborrow — see the doc comment for
        // why.
        let flags: u16 = unsafe {
            (self
                .0
                .as_ptr()
                .add(core::mem::offset_of!(HeapHeader, flags)) as *const u16)
                .read()
        };
        let parent: *mut c_void = unsafe { self.parent() };

        if flags & FLAG_SUBSTR_INLINE != 0 {
            // Inline: dec parent only. We don't free `self` because
            // its storage is borrowed from the enclosing block.
            drop_parent(parent);
            return;
        }

        // SAFETY: `__torajs_rc_dec` is null-safe + STATIC_LITERAL-safe.
        // Returns 1 when caller must free.
        if unsafe { __torajs_rc_dec(self.0.as_ptr() as *mut c_void) } == 1 {
            drop_parent(parent);
            if !pool_push(self.0) {
                // Pool full → fall through to libc free.
                // SAFETY: block was libc-allocated (or pool-popped
                // from such); free is the matching deallocator.
                unsafe { free(self.0.as_ptr() as *mut c_void) };
            }
        }
    }

    /// Reborrow the heap header mutably. Used by tests that toggle
    /// the INLINE flag to exercise the inline drop branch.
    ///
    /// # Safety
    ///
    /// Single-threaded contract; caller must not alias.
    #[inline]
    pub unsafe fn header(&self) -> &mut HeapHeader {
        unsafe { &mut *(self.0.as_ptr() as *mut HeapHeader) }
    }

    /// Length of the Substr view in bytes (the `len` field at offset
    /// [`SUBSTR_LEN_OFF`]).
    ///
    /// # Safety
    ///
    /// `self.0` must point at a valid Substr block.
    #[inline]
    pub unsafe fn len(&self) -> u64 {
        unsafe { (self.0.as_ptr().add(SUBSTR_LEN_OFF) as *const u64).read() }
    }

    /// Owning parent Str pointer (the `parent_ptr` field at offset
    /// [`SUBSTR_PARENT_OFF`]).
    ///
    /// # Safety
    ///
    /// `self.0` must point at a valid Substr block.
    #[inline]
    pub unsafe fn parent(&self) -> *mut c_void {
        unsafe { (self.0.as_ptr().add(SUBSTR_PARENT_OFF) as *const *mut c_void).read() }
    }

    /// Byte offset into `parent.bytes` where the view starts (the
    /// `offset` field at offset [`SUBSTR_OFFSET_OFF`]).
    ///
    /// # Safety
    ///
    /// `self.0` must point at a valid Substr block.
    #[inline]
    pub unsafe fn offset(&self) -> u64 {
        unsafe { (self.0.as_ptr().add(SUBSTR_OFFSET_OFF) as *const u64).read() }
    }

    /// Hand the raw pointer across the FFI boundary; the wrapper is
    /// consumed without running Drop (there is none — the field is
    /// just a `NonNull`).
    #[inline]
    pub fn into_raw(self) -> *mut u8 {
        let p = self.0.as_ptr();
        core::mem::forget(self);
        p
    }

    /// Rewrap an incoming raw pointer. Pure type-conversion; no
    /// ownership transfer assumed.
    ///
    /// # Safety
    ///
    /// Caller guarantees `p` is non-null and points at a valid
    /// Substr block whose layout matches [`crate::substr`].
    #[inline]
    pub const unsafe fn from_raw(p: *mut u8) -> Self {
        // SAFETY: caller contract.
        Self(unsafe { NonNull::new_unchecked(p) })
    }
}

/// Decrement the parent Str's refcount and free if it reached zero.
/// Inlined copy of `__torajs_str_drop`'s logic (defined as LLVM IR by
/// `ssa_inkwell::define_str_drop`) — see P3.1-g for the consolidation
/// that ports IR `__torajs_str_drop` to Rust and re-routes this
/// helper through it.
#[inline]
fn drop_parent(parent: *mut c_void) {
    if parent.is_null() {
        return;
    }
    // SAFETY: __torajs_rc_dec is null-safe + STATIC_LITERAL-safe; the
    // returned 1 is the canonical "you must free" signal that the
    // Substr drop path is responsible for honoring.
    if unsafe { __torajs_rc_dec(parent) } == 1 {
        // SAFETY: parent was a Str-typed heap block; __torajs_str_free
        // is the matching pool-aware deallocator.
        unsafe { crate::alloc::__torajs_str_free(parent as *mut u8) };
    }
}

// ============================================================
// Substr pool
// ============================================================

const POOL_SLOTS: usize = 32;

static POOL_SLOTS_ARR: [AtomicPtr<u8>; POOL_SLOTS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; POOL_SLOTS];
static POOL_COUNT: AtomicUsize = AtomicUsize::new(0);

#[inline]
fn pool_pop() -> Option<NonNull<u8>> {
    let count = POOL_COUNT.load(Ordering::Relaxed);
    if count == 0 {
        return None;
    }
    let new_count = count - 1;
    POOL_COUNT.store(new_count, Ordering::Relaxed);
    let p = POOL_SLOTS_ARR[new_count].swap(ptr::null_mut(), Ordering::Relaxed);
    NonNull::new(p)
}

#[inline]
fn pool_push(p: NonNull<u8>) -> bool {
    let count = POOL_COUNT.load(Ordering::Relaxed);
    if count >= POOL_SLOTS {
        return false;
    }
    POOL_SLOTS_ARR[count].store(p.as_ptr(), Ordering::Relaxed);
    POOL_COUNT.store(count + 1, Ordering::Relaxed);
    true
}

#[doc(hidden)]
pub fn pool_occupancy() -> usize {
    POOL_COUNT.load(Ordering::Relaxed)
}

#[doc(hidden)]
pub fn pool_clear_for_test() {
    for slot in POOL_SLOTS_ARR.iter() {
        slot.store(ptr::null_mut(), Ordering::Relaxed);
    }
    POOL_COUNT.store(0, Ordering::Relaxed);
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// Allocate a Substr view. Mirrors the pre-rewrite C
/// `__torajs_substr_create(void *parent, uint64_t offset, uint64_t len) -> void *`.
/// Bumps `parent`'s refcount internally.
///
/// # Safety
///
/// `parent` must be null or a live owned-Str heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_create(
    parent: *mut c_void,
    offset: u64,
    len: u64,
) -> *mut c_void {
    // SAFETY: contract forwarded from caller.
    unsafe { SubstrBlock::create(parent, offset, len) }.into_raw() as *mut c_void
}

/// Drop a Substr view. Mirrors the pre-rewrite C
/// `__torajs_substr_drop(void *v) -> void`. Null is a no-op (matches
/// the pre-rewrite C guard).
///
/// # Safety
///
/// `v` must be null or a live Substr heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_drop(v: *mut c_void) {
    if v.is_null() {
        return;
    }
    // SAFETY: just null-checked; from_raw reborrows without taking
    // ownership of a fresh refcount.
    unsafe { SubstrBlock::from_raw(v as *mut u8) }.drop_pool_aware();
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::alloc::StrBlock;
    use std::sync::Mutex;

    // Pool is a process-global static; serialize tests so they
    // don't observe each other's pushes. The WeakRef-hook stub
    // that torajs-rc's `__torajs_rc_dec` needs at test-link time
    // lives in `lib.rs` (shared across all submodules).
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn fresh_parent(len: u64) -> StrBlock {
        let mut block = StrBlock::alloc(len);
        // Fill payload with deterministic bytes so future tests can
        // verify view byte access without UB. (Substr layout doesn't
        // touch parent bytes; we just want valid data sitting there.)
        unsafe { block.as_bytes_mut(len).fill(0x42) };
        block
    }

    #[test]
    fn layout_constants_match_c_macros() {
        assert_eq!(SUBSTR_SIZE, 32);
        assert_eq!(SUBSTR_LEN_OFF, 8);
        assert_eq!(SUBSTR_PARENT_OFF, 16);
        assert_eq!(SUBSTR_OFFSET_OFF, 24);
        assert_eq!(FLAG_SUBSTR_INLINE, 1u16);
    }

    #[test]
    fn create_writes_all_four_fields_and_bumps_parent_rc() {
        let _g = TEST_LOCK.lock().unwrap();
        pool_clear_for_test();

        let parent_block = fresh_parent(8);
        let parent_ptr = parent_block.0.as_ptr() as *mut c_void;

        // Parent at refcount=1 from StrBlock::alloc.
        let view = unsafe { SubstrBlock::create(parent_ptr, 2, 5) };
        unsafe {
            let h = view.header();
            assert_eq!(h.refcount, 1);
            assert_eq!(h.type_tag, Tag::Str as u16);
            assert_eq!(h.flags, 0);
            assert_eq!(view.len(), 5);
            assert_eq!(view.parent(), parent_ptr);
            assert_eq!(view.offset(), 2);
        }

        // Parent's refcount should have been bumped from 1 → 2.
        let parent_hdr = unsafe { &*(parent_ptr as *const HeapHeader) };
        assert_eq!(parent_hdr.refcount, 2, "create should rc_inc parent");

        // Drop the view; parent goes back to refcount=1 and survives.
        view.drop_pool_aware();
        assert_eq!(parent_hdr.refcount, 1, "view drop dec'd parent");

        // Substr block went back into its pool.
        assert_eq!(pool_occupancy(), 1);

        // Finally release the parent.
        parent_block.free_pool_aware();
    }

    #[test]
    fn standalone_drop_round_trips_through_pool() {
        let _g = TEST_LOCK.lock().unwrap();
        pool_clear_for_test();

        let parent_block = fresh_parent(16);
        let parent_ptr = parent_block.0.as_ptr() as *mut c_void;

        // First view allocates fresh; drop returns it to the pool.
        let v1 = unsafe { SubstrBlock::create(parent_ptr, 0, 4) };
        let v1_ptr = v1.0;
        v1.drop_pool_aware();
        assert_eq!(pool_occupancy(), 1);

        // Second create should reuse the same backing storage.
        let v2 = unsafe { SubstrBlock::create(parent_ptr, 8, 4) };
        assert_eq!(v2.0, v1_ptr, "pool should hand back recent Substr");
        assert_eq!(pool_occupancy(), 0);

        v2.drop_pool_aware();
        parent_block.free_pool_aware();
    }

    #[test]
    fn inline_drop_skips_own_rc_and_pool_push_but_dec_parent() {
        let _g = TEST_LOCK.lock().unwrap();
        pool_clear_for_test();

        let parent_block = fresh_parent(8);
        let parent_ptr = parent_block.0.as_ptr() as *mut c_void;

        let view = unsafe { SubstrBlock::create(parent_ptr, 0, 4) };

        // Simulate a split-block-inline substr by flipping the bit.
        unsafe { view.header().flags |= FLAG_SUBSTR_INLINE };
        let view_ptr = view.0;
        let saved_view_rc_before_drop = unsafe { view.header().refcount };

        // Inline drop: no rc dec on self, no pool push, no free.
        view.drop_pool_aware();
        assert_eq!(pool_occupancy(), 0, "inline drop must not push to pool");
        let view_hdr_after = unsafe { &*(view_ptr.as_ptr() as *const HeapHeader) };
        assert_eq!(
            view_hdr_after.refcount, saved_view_rc_before_drop,
            "inline drop must not touch own refcount"
        );

        // Parent did get its rc dec'd.
        let parent_hdr = unsafe { &*(parent_ptr as *const HeapHeader) };
        assert_eq!(parent_hdr.refcount, 1, "inline drop must dec parent");

        // Manually release the substr storage (the enclosing block
        // would do this in production). The HeapHeader::refcount
        // still says 1 (we never dec'd it), so dec_ref it once first
        // to keep semantics tidy then free directly.
        unsafe { free(view_ptr.as_ptr() as *mut c_void) };
        parent_block.free_pool_aware();
    }

    #[test]
    fn extern_c_null_drop_is_noop() {
        unsafe { __torajs_substr_drop(ptr::null_mut()) };
    }

    #[test]
    fn extern_c_create_then_drop_round_trips() {
        let _g = TEST_LOCK.lock().unwrap();
        pool_clear_for_test();

        let parent_block = fresh_parent(16);
        let parent_ptr = parent_block.0.as_ptr() as *mut c_void;

        let v = unsafe { __torajs_substr_create(parent_ptr, 1, 7) };
        assert!(!v.is_null());
        let h = unsafe { &*(v as *const HeapHeader) };
        assert_eq!(h.refcount, 1);
        assert_eq!(h.type_tag, Tag::Str as u16);
        let len = unsafe { (v.cast::<u8>().add(SUBSTR_LEN_OFF) as *const u64).read() };
        assert_eq!(len, 7);

        unsafe { __torajs_substr_drop(v) };
        parent_block.free_pool_aware();
    }
}
