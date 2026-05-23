//! Pool-aware Str alloc / free + the extern "C" wrappers that
//! ssa_inkwell IR + remaining C-side runtime helpers call into.
//!
//! ## Why libc malloc / free directly
//!
//! Same reason as [`torajs-anyvalue`]: the still-C runtime
//! (`runtime_*.c`) uses libc `malloc` / `free` for everything
//! else; routing every Str alloc through the same allocator keeps
//! the cross-language contract trivial — a block alloc'd here can
//! be free'd from any of those C helpers (and vice versa, as we
//! port them) bit-identically. `std::alloc::{alloc, dealloc}`
//! would require carrying a `Layout` across the FFI boundary or
//! re-deriving it on free, both unnecessarily complex when libc's
//! `free` is one-arg.
//!
//! `extern "C" { fn malloc / free }` is a system primitive
//! declaration — not a crates.io dep — so it does not violate the
//! 0-deps pillar (`docs/design-principles.md`).
//!
//! ## Ownership model
//!
//! [`StrBlock`] is a transparent newtype around `NonNull<u8>`.
//! Constructing one (via [`StrBlock::alloc`]) implicitly grants
//! the caller ownership of a fresh refcount=1 block; methods
//! [`StrBlock::as_bytes_mut`] / [`StrBlock::write_payload`] let
//! the caller fill the bytes; [`StrBlock::into_raw`] hands the
//! pointer back out across the FFI boundary. The `Drop` impl is
//! intentionally absent — once a block is exposed to ssa_inkwell
//! IR or C-side helpers, ownership tracking moves into the
//! per-language ABI (refcount on the heap header).

use core::ptr::NonNull;
use std::ffi::c_void;

use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

use crate::layout::{STR_DATA_OFF, STR_LEN_OFF, STR_POOL_PAYLOAD, block_size, packed_header_init};
use crate::pool;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

// ============================================================
// StrBlock — owned Str heap block
// ============================================================

/// Owned Str heap block: `[header:8][len:8][bytes:N]`, prefix
/// `STR_HDR_SIZE = 16`. Constructed via [`StrBlock::alloc`];
/// destructured (and ownership released back to libc / the pool)
/// via [`StrBlock::free_pool_aware`] or handed across the FFI
/// boundary via [`StrBlock::into_raw`].
///
/// Transparent newtype around `NonNull<u8>` so the layout matches
/// the C-side `uint8_t *` pointer that ssa_inkwell-emitted GEPs
/// + runtime_str.c macros operate on. A `StrBlock` value carries
/// no separate runtime overhead.
///
/// **Not `Copy` / `Clone` by design.** Each `StrBlock` represents a
/// single live owned reference. Forgetting to call
/// [`Self::free_pool_aware`] or [`Self::into_raw`] before letting
/// the value drop leaks the underlying block — by design, since
/// the block's true ownership tracker is the heap header's
/// refcount and that lives outside Rust's lifetime model. Make
/// the leak loud by holding the value in a binding rather than
/// silently chaining; `#[must_use]` on the constructor catches the
/// most common case.
#[repr(transparent)]
#[derive(Debug)]
pub struct StrBlock(pub NonNull<u8>);

impl StrBlock {
    /// Allocate a fresh Str heap block with `refcount=1`,
    /// `type_tag=Tag::Str`, `flags=0`, and `len=len`. Bytes are
    /// uninitialized — caller must write them via
    /// [`Self::as_bytes_mut`] / [`Self::write_payload`] before
    /// exposing the block.
    ///
    /// Pool fast-path: when `len ≤ STR_POOL_PAYLOAD` and the pool
    /// has a free slot, the freshly-popped block's header + len
    /// fields are rewritten and the block returned. Otherwise
    /// falls through to a `malloc` sized via [`block_size`].
    #[inline]
    #[must_use = "StrBlock owns a heap allocation; ignore the value and the block leaks"]
    pub fn alloc(len: u64) -> Self {
        if len <= STR_POOL_PAYLOAD {
            if let Some(p) = pool::pop() {
                Self::init_header_and_len(p, len);
                return Self(p);
            }
        }
        // SAFETY: malloc is the standard libc allocator. We then
        // wrap the result in NonNull; libc returns null on OOM
        // which is a hard runtime failure here (caught by
        // `.expect`). Block size is computed by `block_size`
        // matching the C `str_block_size_` exactly.
        let raw = unsafe { malloc(block_size(len)) } as *mut u8;
        let nn = NonNull::new(raw).expect("OOM in Str alloc");
        Self::init_header_and_len(nn, len);
        Self(nn)
    }

    /// Free a Str block via the pool when eligible, otherwise via
    /// `libc::free`. Pool eligibility: `len ≤ STR_POOL_PAYLOAD`
    /// AND the pool has a free slot AND the block does not carry
    /// `FLAG_STATIC_LITERAL` (`.rodata` blocks must never be
    /// freed).
    ///
    /// Defense-in-depth: rc_dec already short-circuits
    /// STATIC_LITERAL blocks, but a stray direct caller would
    /// otherwise try to `free` `.rodata` bytes and crash. The
    /// check is kept here to keep the contract local.
    #[inline]
    pub fn free_pool_aware(mut self) {
        // SAFETY: caller's contract is that `self.0` points at a
        // valid Str block; the header u64 at offset 0 was written
        // by `init_header_and_len` at alloc time and may have
        // been mutated by rc_inc / rc_dec / static-literal setup.
        let header_ref = unsafe { self.header() };
        if header_ref.flags & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        // SAFETY: len u64 was written at alloc time; offset
        // STR_LEN_OFF mirrors runtime_str.c __TORAJS_STR_LEN.
        let len = unsafe { self.len() };
        if len <= STR_POOL_PAYLOAD && pool::push(self.0) {
            return;
        }
        // SAFETY: block was libc-allocated by `Self::alloc` (or
        // by a C-side `malloc(str_block_size_(...))` in the
        // pre-rewrite path — same allocator). `free` is the
        // matching libc deallocator.
        unsafe { free(self.0.as_ptr() as *mut c_void) };
    }

    /// Length of the Str payload in bytes. Reads the u64 at
    /// `STR_LEN_OFF`.
    ///
    /// # Safety
    ///
    /// Caller guarantees `self.0` points at a valid Str block
    /// whose layout matches [`crate::layout`].
    #[inline]
    pub unsafe fn len(&self) -> u64 {
        unsafe { (self.0.as_ptr().add(STR_LEN_OFF) as *const u64).read() }
    }

    /// Mutable byte slice over the payload region. Caller writes
    /// into this after [`Self::alloc`] to fill the freshly-
    /// allocated block.
    ///
    /// # Safety
    ///
    /// Caller guarantees `self.0` points at a valid Str block
    /// with `len` matching the slice length. Calling on a block
    /// the caller does not own (refcount > 1, or shared via the
    /// extern "C" boundary) is UB.
    #[inline]
    pub unsafe fn as_bytes_mut(&mut self, len: u64) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.0.as_ptr().add(STR_DATA_OFF), len as usize) }
    }

    /// Reborrow the heap header as a mutable reference. Used by
    /// future Str sub-step ops (rc_inc / rc_dec / set_color /
    /// freeze).
    ///
    /// # Safety
    ///
    /// Single-threaded runtime; caller must not alias the returned
    /// mut ref with any other access to the same header.
    #[inline]
    pub unsafe fn header(&mut self) -> &mut HeapHeader {
        unsafe { &mut *(self.0.as_ptr() as *mut HeapHeader) }
    }

    /// Hand the raw pointer out across the FFI boundary. Ownership
    /// transfers with the pointer; the [`StrBlock`] wrapper is
    /// consumed by value (no `Drop` impl runs since the field is
    /// just a `NonNull`).
    #[inline]
    pub fn into_raw(self) -> *mut u8 {
        let p = self.0.as_ptr();
        core::mem::forget(self);
        p
    }

    /// Wrap an FFI-incoming raw pointer back into a [`StrBlock`]
    /// without taking ownership. Used at every extern "C" entry
    /// point that receives a `*mut u8` Str block.
    ///
    /// # Safety
    ///
    /// Caller guarantees `p` is non-null and points at a valid
    /// Str block whose layout matches [`crate::layout`].
    #[inline]
    pub const unsafe fn from_raw(p: *mut u8) -> Self {
        // SAFETY: caller's contract is non-null + valid Str.
        Self(unsafe { NonNull::new_unchecked(p) })
    }

    /// Write the packed header + len at the start of a freshly-
    /// allocated (or pool-popped) block. Internal helper; not
    /// exposed.
    #[inline]
    fn init_header_and_len(p: NonNull<u8>, len: u64) {
        // SAFETY: caller has just produced `p` via `malloc` or
        // `pool::pop`; the first 16 bytes are exclusively owned
        // until we return.
        unsafe {
            (p.as_ptr() as *mut u64).write(packed_header_init());
            (p.as_ptr().add(STR_LEN_OFF) as *mut u64).write(len);
        }
    }
}

// ============================================================
// extern "C" wrappers — ABI mirrors runtime_str.c originals
// ============================================================

/// Pool-aware Str allocation. Mirrors the pre-rewrite C
/// `__torajs_str_alloc_pooled(uint64_t len) -> uint8_t *`. ssa_
/// inkwell's IR-emitted `__torajs_str_alloc` delegates to this
/// for short strings; remaining C helpers in
/// `crates/torajs-runtime/src/runtime_str.c` call it directly.
///
/// Returns a fresh refcount=1 block with `len` payload bytes
/// reserved (uninitialized). On allocator failure the function
/// panics — matching the pre-rewrite "abort on OOM" behavior
/// (`malloc` returning null leads to `expect` here; rc_inc /
/// rc_dec semantics aren't reached).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(len: u64) -> *mut u8 {
    StrBlock::alloc(len).into_raw()
}

/// Str alloc + payload copy in one call. Equivalent to:
/// `__torajs_str_alloc_pooled(len)` followed by `memcpy(data, src, len)`.
///
/// Used by every literal materialization (`"hello"` lowers to
/// `__torajs_str_alloc(literal_ptr, 5)`) and by call sites that
/// build a Str from an already-laid-out byte buffer. The pool fast-
/// path applies via `StrBlock::alloc`. Ported from
/// `ssa_inkwell::define_str_alloc` (P3.1-g.2, 2026-05-23) — the
/// IR version emitted a 2-call sequence (`str_alloc_pooled` then
/// `memcpy`); the Rust path collapses both into one extern fn
/// while preserving the alloc-noalias whitelist entry.
///
/// # Safety
///
/// `src` must point at a readable region of at least `len` bytes
/// (or be NULL when `len == 0`). Returned pointer is a fresh
/// refcount=1 Str block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8 {
    let len_u = len as u64;
    let mut block = StrBlock::alloc(len_u);
    if len_u > 0 {
        let dst = unsafe { block.as_bytes_mut(len_u) };
        let src_slice = unsafe { core::slice::from_raw_parts(src, len_u as usize) };
        dst.copy_from_slice(src_slice);
    }
    block.into_raw()
}

/// `__torajs_str_drop(s)` — Str scope-end decrement. The dominant
/// drop path emitted by ssa_lower for every Str-typed local.
/// Mirrors pre-rewrite `ssa_inkwell::define_str_drop` bit-for-bit
/// (P3.1-g.6, 2026-05-23):
///
/// ```text
/// if s == NULL: return
/// if (s.flags & FLAG_STATIC_LITERAL) != 0: return  // .rodata
/// s.refcount -= 1
/// if s.refcount == 0: libc::free(s)               // NOT pool!
/// ```
///
/// **Pool-bypass is intentional**: the IR version called libc free
/// directly (not pool-aware [`__torajs_str_free`]). Preserved here
/// to match the previous shipped behavior bit-for-bit. The pool is
/// only fed by explicit drops from C-side helpers / Rust ops that
/// call [`__torajs_str_free`] directly (concat result drops,
/// transform/replace intermediate drops, etc.).
///
/// # Safety
///
/// `s` must be null or a valid Str heap block with the universal
/// `{refcount: u32, type_tag: u16, flags: u16}` header at offset 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_drop(s: *mut u8) {
    if s.is_null() {
        return;
    }
    // SAFETY: caller guarantees `s` points at a valid Str block;
    // header is the universal HeapHeader layout, 8 bytes at offset 0.
    let header = unsafe { &mut *(s as *mut HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    header.refcount -= 1;
    if header.refcount == 0 {
        // SAFETY: rc reached 0; we own the last reference. libc::free
        // is the matching deallocator for libc::malloc + pool block
        // allocations (both reachable via __torajs_str_alloc_pooled).
        unsafe { free(s as *mut c_void) };
    }
}

/// Pool-aware Str free. Mirrors the pre-rewrite C
/// `__torajs_str_free(uint8_t *p) -> void`. Called by C-side
/// helpers and Rust ops that release intermediate allocations
/// (concat result drops, transform/replace temp drops, etc.) —
/// NOT by the IR-emitted Str scope-end drop, which routes
/// through [`__torajs_str_drop`] above to libc free directly.
///
/// Null is a no-op (matches the pre-rewrite C guard). Blocks
/// carrying [`FLAG_STATIC_LITERAL`] are also a no-op —
/// `.rodata` Str literals must never be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_free(p: *mut u8) {
    if p.is_null() {
        return;
    }
    // SAFETY: caller guarantees `p` is null-or-Str; we just
    // null-checked. `from_raw` reborrows without taking ownership
    // since the original allocation was via libc malloc.
    unsafe { StrBlock::from_raw(p) }.free_pool_aware();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::STR_HDR_SIZE;
    use crate::pool;

    use std::sync::Mutex;

    // The pool is a process-global static; serialize tests so a
    // push from one test doesn't leak into another's pop
    // expectations.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn alloc_short_writes_header_and_len() {
        let _g = TEST_LOCK.lock().unwrap();
        pool::clear_for_test();

        let mut block = StrBlock::alloc(8);
        // SAFETY: just allocated, single owner; valid Str layout.
        unsafe {
            let header = block.header();
            assert_eq!(header.refcount, 1);
            assert_eq!(header.type_tag, 0); // Tag::Str
            assert_eq!(header.flags, 0);
        }
        assert_eq!(unsafe { block.len() }, 8);

        // payload region is `STR_HDR_SIZE` bytes past the start
        unsafe { block.as_bytes_mut(8).fill(0x41) };
        assert!(unsafe { block.as_bytes_mut(8) }.iter().all(|&b| b == 0x41));

        block.free_pool_aware();
    }

    #[test]
    fn alloc_long_bypasses_pool() {
        let _g = TEST_LOCK.lock().unwrap();
        pool::clear_for_test();

        // STR_POOL_PAYLOAD = 16; ask for 128.
        let block = StrBlock::alloc(128);
        assert_eq!(unsafe { block.len() }, 128);

        block.free_pool_aware();
        // Long block freed straight to libc, pool stays empty.
        assert_eq!(pool::occupancy(), 0);
    }

    #[test]
    fn short_alloc_then_free_round_trips_through_pool() {
        let _g = TEST_LOCK.lock().unwrap();
        pool::clear_for_test();

        let a = StrBlock::alloc(4);
        let a_ptr = a.0;
        a.free_pool_aware();
        assert_eq!(pool::occupancy(), 1);

        // Next short alloc should reuse the same block from the
        // pool LIFO.
        let b = StrBlock::alloc(4);
        assert_eq!(b.0, a_ptr, "pool should hand back the freed block");
        assert_eq!(pool::occupancy(), 0);

        b.free_pool_aware();
    }

    #[test]
    fn extern_c_null_free_is_noop() {
        unsafe { __torajs_str_free(core::ptr::null_mut()) };
    }

    #[test]
    fn extern_c_alloc_then_free_round_trips() {
        let _g = TEST_LOCK.lock().unwrap();
        pool::clear_for_test();

        let p = unsafe { __torajs_str_alloc_pooled(12) };
        assert!(!p.is_null());
        // refcount=1, tag=Str, flags=0 at offset 0
        let header = unsafe { &*(p as *const HeapHeader) };
        assert_eq!(header.refcount, 1);
        assert_eq!(header.type_tag, 0);
        assert_eq!(header.flags, 0);
        // len at offset STR_LEN_OFF
        let len = unsafe { (p.add(STR_LEN_OFF) as *const u64).read() };
        assert_eq!(len, 12);
        unsafe { __torajs_str_free(p) };
    }

    #[test]
    fn static_literal_free_is_skipped() {
        let _g = TEST_LOCK.lock().unwrap();
        pool::clear_for_test();

        // Heap-alloc, then flip STATIC_LITERAL flag to simulate
        // a `.rodata`-like block. `free_pool_aware` must skip
        // both pool push AND libc free.
        let mut block = StrBlock::alloc(4);
        let ptr = block.0;
        unsafe { block.header().flags |= FLAG_STATIC_LITERAL };

        // No assertion beyond "does not crash" — if we ever did
        // push it to the pool, the next test's allocator would
        // hand back this immortal block and write into it,
        // corrupting whatever real .rodata block has the same
        // address. The defensive `if` in free_pool_aware is what
        // protects against that.
        block.free_pool_aware();
        assert_eq!(pool::occupancy(), 0, "static-literal must not enter pool");

        // Drain the (still-alive) block manually since
        // free_pool_aware deliberately skipped real free.
        unsafe { free(ptr.as_ptr() as *mut c_void) };
    }

    #[test]
    fn payload_offset_matches_layout() {
        let _g = TEST_LOCK.lock().unwrap();
        pool::clear_for_test();

        let mut block = StrBlock::alloc(4);
        let block_addr = block.0.as_ptr() as usize;
        let payload_addr = unsafe { block.as_bytes_mut(4).as_ptr() } as usize;
        assert_eq!(payload_addr - block_addr, STR_HDR_SIZE);
        block.free_pool_aware();
    }
}
