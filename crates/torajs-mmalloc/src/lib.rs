//! v0.7-A2 — mmap-backed allocator for torajs user binary.
//!
//! Replaces libc `malloc / free / realloc / memcpy / memmove` for the
//! AOT-emitted user binary. All memory comes from `mmap`
//! (via [`torajs_syscall::mmap_anon_rw`]) — no `brk` / no
//! `libSystem.dylib` involvement.
//!
//! ## Layered structure
//!
//! 1. **Page bump** ([`page::PageBump`]) — fixed-size page (16 KB)
//!    allocated via mmap; sub-allocations bump-allocate from it
//!    until full, then a new page is requested. Fastest path for
//!    small temporaries; no per-allocation header.
//! 2. **Size-class free list** ([`size_class::SizeClassPool`]) —
//!    one LIFO free-list per power-of-two bucket (16/32/64/128/
//!    256/512/1024/2048/4096). Recycles freed blocks. Layer 2/3.
//! 3. **Direct mmap fallback** ([`large::large_alloc`]) — for
//!    `size > 4096` bytes, mmap a fresh page-aligned region and
//!    return it. `large_free` munmaps. No pooling; large allocs
//!    are assumed infrequent.
//!
//! ## Public API (v0.7-A2 scope)
//!
//! ```text
//! __torajs_malloc(size)           → *mut u8 | null on OOM
//! __torajs_free(ptr, size)        → void
//! __torajs_realloc(ptr, old, new) → *mut u8 | null
//! __torajs_memcpy(dst, src, n)    → *mut u8 (dst)
//! __torajs_memmove(dst, src, n)   → *mut u8 (dst)  — overlap-safe
//! __torajs_memcmp(a, b, n)        → i32 (lex compare)
//! ```
//!
//! The `size` parameter on `free`/`realloc` is required (no header
//! overhead) — caller must remember the original allocation size.
//! Sub-crates that need C-malloc-compatible behavior (auto-size-
//! tracking) sit a thin shim on top.

pub mod page;
pub mod size_class;

pub use size_class::Allocator;
