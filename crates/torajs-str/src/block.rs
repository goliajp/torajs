//! Pool-aware Str alloc / free + the extern "C" wrappers that
//! toolchain-emitted code calls into.
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
//! intentionally absent — once a block crosses the FFI boundary,
//! ownership tracking moves into the
//! per-language ABI (refcount on the heap header).
//!
//! ## P11.1-S1 layout (vs pre-S1 byte-Str)
//!
//! Block prefix is still 16 bytes, but the `len u64 @8` field
//! split into `length u32 @8 + _pad u32 @12`; `flags u16 @6`
//! now carries an `IS_LATIN1` bit (see [`crate::layout`]).
//! Public methods that used to talk in `u64 len` now talk in
//! `u32 length` (the spec's UTF-16 code unit count). S1 phase
//! forces `is_latin1 = true` at every alloc site so the payload
//! semantics — byte count, byte iteration, byte indexing — match
//! the pre-S1 byte-Str exactly. S3 lifts the force.

use core::ffi::c_void;
use core::ptr::NonNull;

use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

use crate::layout::{
    STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_HDR_SIZE, STR_LEN_OFF, STR_PAD_OFF, STR_POOL_PAYLOADS,
    block_size, byte_capacity, packed_header_init, pool_class_of,
};
use crate::pool;

unsafe extern "C" {
    /// torajs-mmalloc Layer 1 alloc — Step 4 (v0.7-A2 Phase 2e sweep).
    /// Returns raw chunk pointer (no SHIM offset). Caller passes size
    /// on free too — derived from `block_size(length, is_latin1)`.
    #[link_name = "__torajs_malloc"]
    fn malloc(size: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free(ptr: *mut c_void, size: usize);
}

// ============================================================
// StrBlock — owned Str heap block
// ============================================================

/// Owned Str heap block: `[header:8][length:4][_pad:4][bytes:N]`,
/// prefix `STR_HDR_SIZE = 16`. Constructed via [`StrBlock::alloc`];
/// destructured (and ownership released back to libc / the pool)
/// via [`StrBlock::free_pool_aware`] or handed across the FFI
/// boundary via [`StrBlock::into_raw`].
///
/// Transparent newtype around `NonNull<u8>` so the layout matches
/// the raw byte pointer that toolchain-emitted accesses operate
/// on. A `StrBlock` value carries
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
    /// Allocate a fresh Latin-1 Str heap block with `refcount=1`,
    /// `type_tag=Tag::Str`, `flags=IS_LATIN1`, and `length=length`.
    /// Bytes are uninitialized — caller must write them via
    /// [`Self::as_bytes_mut`] / [`Self::write_payload`] before
    /// exposing the block.
    ///
    /// Pool fast-path: when `byte_capacity` lands in a pool class and
    /// the pool has a free slot, the freshly-popped block's header
    /// + length fields are rewritten and the block returned.
    /// Otherwise falls through to a `malloc` sized via
    /// [`block_size`].
    ///
    /// P11.1-S1: encoding is hard-coded to Latin-1 at every alloc
    /// site. UTF-16 alloc + a free-form `is_latin1` arg land in
    /// the S3 runtime path.
    #[inline]
    #[must_use = "StrBlock owns a heap allocation; ignore the value and the block leaks"]
    pub fn alloc(length: u32) -> Self {
        // Default-Latin-1 alloc — the pre-S2 hot path. Callers that
        // need UTF-16 payload routing use
        // [`Self::alloc_with_encoding`] below.
        Self::alloc_with_encoding(length, true)
    }

    /// Allocate a fresh Str heap block with `refcount=1`,
    /// `type_tag=Tag::Str`, the requested `is_latin1` flag bit set
    /// (or cleared) on `flags`, and `length=length` code units.
    /// Bytes are uninitialized — caller must write `byte_capacity =
    /// length × (if is_latin1 { 1 } else { 2 })` bytes via
    /// [`Self::as_bytes_mut`] before exposing the block.
    ///
    /// P11.1-S2.1 — introduced when concat / coerce / etc grew
    /// encoding-aware. The pool fast-path applies whenever the
    /// requested `byte_capacity` (not the code unit count) fits in
    /// the class payload; UTF-16 short strings still pool, just
    /// at half the code-unit count compared to Latin-1.
    #[inline]
    #[must_use = "StrBlock owns a heap allocation; ignore the value and the block leaks"]
    pub fn alloc_with_encoding(length: u32, is_latin1: bool) -> Self {
        let cap = byte_capacity(length, is_latin1);
        if let Some(class) = pool_class_of(cap) {
            if let Some(p) = pool::pop(class) {
                Self::init_header_and_length(p, length, is_latin1);
                return Self(p);
            }
        }
        // SAFETY: malloc is the standard libc allocator. We then
        // wrap the result in NonNull; libc returns null on OOM
        // which is a hard runtime failure here (caught by
        // `.expect`). Block size is computed by `block_size`
        // matching the C `str_block_size_` exactly.
        let raw = unsafe { malloc(block_size(length, is_latin1)) } as *mut u8;
        let nn = NonNull::new(raw).unwrap_or_else(|| torajs_abort::abort_with(b"OOM in Str alloc"));
        Self::init_header_and_length(nn, length, is_latin1);
        Self(nn)
    }

    /// Allocate a fresh Str block holding `length` code units but
    /// owning `cap` payload bytes, with the surplus recorded in the
    /// capacity slot so `free` gives the right size back and the next
    /// append can write into the slack.
    ///
    /// `cap` must come from [`grow_capacity`] and be at least
    /// `byte_capacity(length, is_latin1)`. Bypasses the pool: a
    /// pooled block's payload is its class payload, which is exactly
    /// the case where `grow_capacity` answers that same number, so
    /// the caller reaching here wants a size the pool does not hold.
    ///
    /// # Panics
    ///
    /// Aborts on allocator OOM, like [`Self::alloc_with_encoding`].
    #[must_use = "StrBlock owns a heap allocation; ignore the value and the block leaks"]
    pub fn alloc_with_capacity(length: u32, is_latin1: bool, cap: u32) -> Self {
        debug_assert!(cap >= byte_capacity(length, is_latin1));
        if let Some(class) = pool_class_of(cap) {
            if STR_POOL_PAYLOADS[class] == cap {
                if let Some(p) = pool::pop(class) {
                    Self::init_header_and_length(p, length, is_latin1);
                    return Self(p);
                }
            }
        }
        // SAFETY: same contract as `alloc_with_encoding` — libc
        // malloc, null means OOM, header written before exposure.
        let raw = unsafe { malloc(STR_HDR_SIZE + cap as usize) } as *mut u8;
        let nn = NonNull::new(raw).unwrap_or_else(|| torajs_abort::abort_with(b"OOM in Str alloc"));
        Self::init_header_and_length(nn, length, is_latin1);
        // Only past the pool ladder does the block hold more than
        // `block_size` would say; below it the two agree and zero
        // keeps the slot inert.
        if cap != (block_size(length, is_latin1) - STR_HDR_SIZE) as u32 {
            unsafe { (nn.as_ptr().add(STR_PAD_OFF) as *mut u32).write(cap) };
        }
        Self(nn)
    }

    /// Free a Str block via the pool when eligible, otherwise via
    /// `libc::free`. Pool eligibility: `byte_capacity ≤
    /// a pool class AND that class has a free slot AND the
    /// block does not carry `FLAG_STATIC_LITERAL` (`.rodata`
    /// blocks must never be freed).
    ///
    /// Defense-in-depth: rc_dec already short-circuits
    /// STATIC_LITERAL blocks, but a stray direct caller would
    /// otherwise try to `free` `.rodata` bytes and crash. The
    /// check is kept here to keep the contract local.
    ///
    /// `byte_capacity` is recomputed from `(length, is_latin1)` on
    /// every drop — mirrors V8 SeqString sizing. Latin-1 path is a
    /// no-op multiply; UTF-16 path is a single shift.
    #[inline]
    pub fn free_pool_aware(mut self) {
        // SAFETY: caller's contract is that `self.0` points at a
        // valid Str block; the header u64 at offset 0 was written
        // by `init_header_and_length` at alloc time and may have
        // been mutated by rc_inc / rc_dec / static-literal setup.
        let header_ref = unsafe { self.header() };
        if header_ref.flags & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        // SAFETY: length u32 + capacity u32 were written at alloc
        // time; the offsets mirror runtime_str.c __TORAJS_STR_LEN.
        let cap = unsafe { self.payload_capacity() };
        // The pool hands blocks back out by class, so a block may
        // only enter one whose payload it matches EXACTLY — a
        // 32-byte cell parked in the 16-byte class would later be
        // freed at 16 and corrupt the heap. Every cell sized through
        // `block_size` satisfies this by construction (the ladder
        // rounds up); an append-grown cell satisfies it because
        // `grow_capacity` floors at a class payload.
        if let Some(class) = pool_class_of(cap) {
            if STR_POOL_PAYLOADS[class] == cap && pool::push(class, self.0) {
                return;
            }
        }
        // SAFETY: block was `malloc(STR_HDR_SIZE + cap)`-allocated by
        // `Self::alloc*` (or a future caller follows the same shape).
        // Layer 1 `free` takes the same size we alloc'd with.
        unsafe { free(self.0.as_ptr() as *mut c_void, STR_HDR_SIZE + cap as usize) };
    }

    /// Payload bytes the block actually owns — what `free` must be
    /// handed back, and how much room an in-place append has.
    ///
    /// Reads the capacity slot at [`STR_PAD_OFF`]; zero (what every
    /// alloc site writes) means "exactly what [`block_size`] asked
    /// for", so the answer stays a pure function of `(length,
    /// is_latin1)` for every cell nothing has appended to.
    ///
    /// # Safety
    ///
    /// Caller guarantees `self.0` points at a valid Str block.
    #[inline]
    pub unsafe fn payload_capacity(&self) -> u32 {
        let stored = unsafe { (self.0.as_ptr().add(STR_PAD_OFF) as *const u32).read() };
        if stored != 0 {
            return stored;
        }
        let length = unsafe { self.length() };
        let is_latin1 = unsafe { self.is_latin1() };
        (block_size(length, is_latin1) - STR_HDR_SIZE) as u32
    }

    /// Overwrite the code-unit length. Used by
    /// [`crate::append`] after it writes into the slack a previous
    /// grow reserved.
    ///
    /// # Safety
    ///
    /// Caller owns the block outright (refcount 1, not a `.rodata`
    /// literal, not a Substr view) and has already written
    /// `byte_capacity(length, is_latin1)` payload bytes.
    #[inline]
    pub unsafe fn set_length(&mut self, length: u32) {
        unsafe { (self.0.as_ptr().add(STR_LEN_OFF) as *mut u32).write(length) };
    }

    /// Length of the Str payload in **code units** (per ES spec
    /// `String.length`). Reads the u32 at `STR_LEN_OFF`.
    ///
    /// For Latin-1 strings code unit == byte; for UTF-16 strings
    /// code unit == u16 (so total payload bytes = `length × 2`).
    ///
    /// # Safety
    ///
    /// Caller guarantees `self.0` points at a valid Str block
    /// whose layout matches [`crate::layout`].
    #[inline]
    pub unsafe fn length(&self) -> u32 {
        unsafe { (self.0.as_ptr().add(STR_LEN_OFF) as *const u32).read() }
    }

    /// True when the block carries the `IS_LATIN1` flag bit
    /// (payload is 1 byte per code unit). False when payload is
    /// UTF-16 (2 bytes per code unit).
    ///
    /// # Safety
    ///
    /// Caller guarantees `self.0` points at a valid Str block.
    #[inline]
    pub unsafe fn is_latin1(&self) -> bool {
        let header = unsafe { &*(self.0.as_ptr() as *const HeapHeader) };
        (header.flags & STR_FLAG_IS_LATIN1) != 0
    }

    /// Mutable byte slice over the payload region. Caller writes
    /// into this after [`Self::alloc`] to fill the freshly-
    /// allocated block.
    ///
    /// The `byte_len` argument is the **byte** count of the
    /// region (= `byte_capacity(length, is_latin1)`), not the
    /// code unit count. For Latin-1 these are equal; for UTF-16
    /// caller must pass `length * 2`.
    ///
    /// # Safety
    ///
    /// Caller guarantees `self.0` points at a valid Str block
    /// whose payload region is at least `byte_len` bytes. Calling
    /// on a block the caller does not own (refcount > 1, or
    /// shared via the extern "C" boundary) is UB.
    #[inline]
    pub unsafe fn as_bytes_mut(&mut self, byte_len: u32) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.0.as_ptr().add(STR_DATA_OFF), byte_len as usize)
        }
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

    /// Write the packed header + length + zero the reserved
    /// padding at the start of a freshly-allocated (or pool-
    /// popped) block. Internal helper; not exposed.
    #[inline]
    fn init_header_and_length(p: NonNull<u8>, length: u32, is_latin1: bool) {
        // SAFETY: caller has just produced `p` via `malloc` or
        // `pool::pop`; the first 16 bytes are exclusively owned
        // until we return.
        unsafe {
            (p.as_ptr() as *mut u64).write(packed_header_init(is_latin1));
            (p.as_ptr().add(STR_LEN_OFF) as *mut u32).write(length);
            (p.as_ptr().add(STR_PAD_OFF) as *mut u32).write(0);
        }
    }
}

// The `extern "C"` alloc entry points live in `block_ffi.rs` and
// `__torajs_str_drop` / `__torajs_str_free` in `str_drop.rs` (both
// sibling modules, registered in `lib.rs`). Pulled out to keep this
// file under the 500-prod-LOC file-size hard limit. Re-export here
// so existing `crate::block::__torajs_str_*` callers keep working.
pub use crate::block_ffi::{
    __torajs_str_alloc, __torajs_str_alloc_ascii, __torajs_str_alloc_pooled,
    __torajs_str_alloc_pooled_enc,
};
pub use crate::str_drop::{__torajs_str_drop, __torajs_str_free};

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
    fn alloc_short_writes_header_and_length() {
        let _g = TEST_LOCK.lock().unwrap();
        pool::clear_for_test();

        let mut block = StrBlock::alloc(8);
        // SAFETY: just allocated, single owner; valid Str layout.
        unsafe {
            let header = block.header();
            assert_eq!(header.refcount, 1);
            assert_eq!(header.type_tag, 0); // Tag::Str
            assert_eq!(header.flags, STR_FLAG_IS_LATIN1, "S1 alloc forces Latin-1");
        }
        assert_eq!(unsafe { block.length() }, 8);
        assert!(unsafe { block.is_latin1() });

        // payload region is `STR_HDR_SIZE` bytes past the start
        unsafe { block.as_bytes_mut(8).fill(0x41) };
        assert!(unsafe { block.as_bytes_mut(8) }.iter().all(|&b| b == 0x41));

        block.free_pool_aware();
    }

    #[test]
    fn alloc_long_bypasses_pool() {
        let _g = TEST_LOCK.lock().unwrap();
        pool::clear_for_test();

        // largest pool class is 64 bytes; ask for 128.
        let block = StrBlock::alloc(128);
        assert_eq!(unsafe { block.length() }, 128);

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
        // refcount=1, tag=Str, flags=IS_LATIN1 at offset 0
        let header = unsafe { &*(p as *const HeapHeader) };
        assert_eq!(header.refcount, 1);
        assert_eq!(header.type_tag, 0);
        assert_eq!(header.flags, STR_FLAG_IS_LATIN1);
        // length at offset STR_LEN_OFF (u32 now)
        let length = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
        assert_eq!(length, 12);
        // reserved padding zero
        let pad = unsafe { (p.add(STR_PAD_OFF) as *const u32).read() };
        assert_eq!(pad, 0);
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
        // free_pool_aware deliberately skipped real free. Use
        // the alloc-time params (length=4, is_latin1=true).
        unsafe { free(ptr.as_ptr() as *mut c_void, block_size(4, true)) };
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
