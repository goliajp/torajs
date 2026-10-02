//! mmap-backed allocator for the torajs user binary.
//!
//! Replaces libc `malloc / free / realloc / memcpy / memmove` for the
//! AOT-emitted program. All memory comes from `mmap` (via
//! [`torajs_syscall::mmap_anon_rw`]): no `brk`, no `libSystem.dylib`.
//!
//! ## Structure
//!
//! 1. **Rows** ([`tlab::TlabCache`]) — per size class, a stack
//!    of free blocks the fast path pops and pushes without atomics;
//!    the toolchain inlines both at object-cell sites.
//! 2. **Spans** ([`span::SpanMeta`]) — a run of arena pages carved
//!    into equal slots of one class, with an owner-local free list,
//!    an atomic list for other threads' releases, and a `used` count
//!    that tells when every slot is back.
//! 3. **Arenas** ([`arena::Arena`]) — 1 MiB aligned mappings whose
//!    header page holds the span metadata and a page bitmap; a block
//!    names its arena by address and its span by one table read. An
//!    empty arena goes back to the kernel.
//! 4. **Large** ([`large::large_alloc`]) — `size > 4096` maps a region
//!    of its own, tracked in the [`region_registry`].
//!
//! ## Public API
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
//! The `size` parameter on `free` / `realloc` is required (no header
//! overhead): the caller remembers the allocation size. Sub-crates
//! that need C-malloc-compatible behavior sit a thin shim on top.

pub mod arena;
pub mod core;
pub mod extern_api;
pub mod global_alloc;
pub mod heap;
pub mod large;
pub mod region_registry;
pub mod size_class;
pub mod span;
pub mod tlab;

pub use global_alloc::TorajsAllocator;
pub use size_class::bucket_for;
