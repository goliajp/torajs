//! Custom panic infrastructure for torajs AOT user binaries.
//!
//! Layer-0 substrate (v0.7 Step 9b, 2026-05-27) — replaces std's
//! default `rust_begin_unwind` + `rust_panic` + `__rust_start_panic`
//! + `__rust_alloc_error_handler` with minimal banner+abort impls.
//! Strips ~250 KiB of `backtrace_rs` / `gimli` / `rustc_demangle` /
//! `std::path` / `std::io::error` / `std::thread::Thread` /
//! `std::panicking::*hook` from every user binary.
//!
//! ## Dual-mode crate
//!
//! This crate has **two** compile modes, gated by the
//! `torajs_panic_runtime_active` cfg flag (set by
//! `scripts/release-build.sh` via `RUSTFLAGS=--cfg=...`):
//!
//! - **Active mode** (release-build.sh, polish-A4.1) — `#![no_std]` +
//!   `#![panic_runtime]` + `#[panic_handler]` + `#[alloc_error_handler]`
//!   + `rustc_std_internal_symbol` overrides. Produces a staticlib
//!   that, when `-Wl,-force_load`'ed first, wins every panic-routing
//!   symbol in the binary and dead-strips std's panic tree.
//!
//! - **Inactive mode** (plain `cargo build` / `cargo test`) — empty
//!   placeholder fn so the staticlib still emits (torajs-core's
//!   `build.rs` requires `.a` to exist). Plain cargo test needs
//!   `panic = "unwind"` for `catch_unwind`, which conflicts with
//!   `#![panic_runtime]`; inactive mode side-steps that.
//!
//! Why a dual-mode crate instead of a separate "torajs-panic-runtime-
//! impl" crate behind a feature flag: torajs-core's `build.rs` /
//! `lib.rs` enumerates Layer-1+ staticlib paths at compile time via
//! `include_bytes!(env!("..."))`. Splitting into two crates would
//! require conditional `STATICLIBS` plumbing — a much bigger surface
//! change for the same outcome. cfg-gating preserves the single-crate
//! shape.
//!
//! ## How the active-mode override works
//!
//! Decision flow on `panic!()` / `[i]` OOB / `Option::unwrap_none`:
//!
//! 1. user/std code → `core::panicking::*` lang_item dispatch.
//! 2. lang_item dispatch → `rust_begin_unwind` (the `#[panic_handler]`
//!    symbol) **and** `rust_panic` (the `std::panicking::rust_panic`
//!    internal lang_item, mangled via `__rustc` virtual crate).
//! 3. std's default impls walk PanicInfo, format the message, capture
//!    backtrace via `std::sys::backtrace::*`, demangle frames via
//!    `rustc_demangle`, print via `std::panicking::default_hook`,
//!    then call `__rust_start_panic` to dispatch the runtime.
//!
//! Our override provides **all four** entries (`rust_begin_unwind`
//! via `#[panic_handler]`; `rust_panic` + `rust_panic_with_hook` via
//! `#[rustc_std_internal_symbol]` / `#[unsafe(no_mangle)]`;
//! `__rust_start_panic` via `#![panic_runtime]` auto-attribute;
//! `__rust_alloc_error_handler` via `#[alloc_error_handler]`). All
//! short-circuit to a fixed banner + libc abort.
//!
//! ## Wire-up
//!
//! `crates/torajs-core/build.rs` enumerates this crate in `STATICLIBS`,
//! `crates/torajs-core/src/lib.rs` embeds the `.a` bytes via
//! `include_bytes!`, and `torajs-link` merges the archive into every
//! `tr build` user binary (the LLVM-era pipeline force-loaded it
//! before every other staticlib via `-Wl,-force_load,<path>`).

#![cfg_attr(torajs_panic_runtime_active, no_std)]
#![cfg_attr(
    torajs_panic_runtime_active,
    feature(alloc_error_handler, panic_runtime, rustc_attrs, std_internals,)
)]
#![cfg_attr(torajs_panic_runtime_active, panic_runtime)]
#![cfg_attr(torajs_panic_runtime_active, allow(internal_features))]

// ===========================================================================
// Active mode — `cfg(torajs_panic_runtime_active)`
// ===========================================================================

#[cfg(torajs_panic_runtime_active)]
mod active {
    use core::alloc::Layout;
    use core::any::Any;
    use core::ffi::c_void;
    use core::panic::PanicInfo;
    use core::panic::PanicPayload;

    const STDERR_FILENO: i32 = 2;

    // v0.7-A5 Step 16-c-1 — route the panic banner's write/abort
    // through torajs-syscall's `__torajs_syscall_*` C-ABI exports
    // instead of libc `write`/`abort`. Link-level binding (libtorajs_
    // syscall.a is STATICLIBS[0], present in every user binary) —
    // preserves this crate's 0 Cargo deps. Drops the last libc
    // `_write` + `_abort` undef refs from user binaries.
    unsafe extern "C" {
        fn __torajs_syscall_write(fd: i32, buf: *const u8, n: usize) -> isize;
        fn __torajs_syscall_abort() -> !;
    }

    /// `#[cold]` + `#[inline(never)]` — critical to preserving the
    /// hot-path codegen of LTO'd user binaries. Without these, LLVM
    /// under `lto = "fat"` sees our 32-byte `rust_begin_unwind` and
    /// inlines panic call sites away, which alters the cold-call /
    /// unreachable modeling at every `panic!()` / `[i]` OOB site —
    /// regressing the bench geomean by ~+15% (closure-pipeline-1m
    /// +40%, array-sum-1m +48%, etc. observed during 9b ship). The
    /// fix is `torajs-abort`'s pattern (see `crates/torajs-abort/src/
    /// lib.rs:70-72`): force the unhappy path out-of-line so LLVM
    /// leaves a single `bl <override>` at each call site and inlines
    /// the success path tightly.
    #[cold]
    #[inline(never)]
    unsafe fn write_banner_and_abort() -> ! {
        let msg = b"torajs panic\n";
        unsafe {
            __torajs_syscall_write(STDERR_FILENO, msg.as_ptr(), msg.len());
            __torajs_syscall_abort()
        }
    }

    /// `#[panic_handler]` for this `#![no_std]` crate. Also generates the
    /// `rust_begin_unwind` symbol that `core::panicking::*` calls into.
    #[cold]
    #[inline(never)]
    #[panic_handler]
    fn torajs_panic_handler(_info: &PanicInfo<'_>) -> ! {
        unsafe { write_banner_and_abort() }
    }

    /// Replacement for panic_abort's `__rust_start_panic`.
    ///
    /// # Safety
    /// Matches `core::panicking::__rust_start_panic` signature.
    #[cold]
    #[inline(never)]
    #[allow(improper_ctypes_definitions)]
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __rust_start_panic(_payload: &mut dyn Any) -> u32 {
        unsafe { write_banner_and_abort() }
    }

    /// `rust_eh_personality` — no-op under `panic = "abort"`.
    #[unsafe(no_mangle)]
    pub extern "C" fn rust_eh_personality() {}

    /// Replacement for `std::panicking::rust_panic`. Without this
    /// override, `core::panicking::panic` dispatches to std's impl
    /// which pulls `panic_with_hook` + backtrace tree.
    #[cold]
    #[inline(never)]
    #[rustc_std_internal_symbol]
    fn rust_panic(_payload: &mut dyn PanicPayload) -> ! {
        unsafe { write_banner_and_abort() }
    }

    /// Replacement for `std::panicking::rust_panic_with_hook`. Some
    /// std code paths (alloc::rust_oom in particular) reference this
    /// entry directly; providing our own keeps the dead-strip closure
    /// tight.
    #[cold]
    #[inline(never)]
    #[unsafe(no_mangle)]
    pub fn rust_panic_with_hook(
        _payload: &mut dyn PanicPayload,
        _message: Option<&core::fmt::Arguments<'_>>,
        _location: &core::panic::Location<'_>,
        _can_unwind: bool,
        _force_no_backtrace: bool,
    ) -> ! {
        unsafe { write_banner_and_abort() }
    }

    /// `#[alloc_error_handler]` — replaces
    /// `std::alloc::default_alloc_error_hook` (which pulls
    /// `std::sys::backtrace` + `gimli` to print frames before
    /// aborting). Generates `__rust_alloc_error_handler` lang item.
    #[cold]
    #[inline(never)]
    #[alloc_error_handler]
    fn torajs_alloc_error_handler(_layout: Layout) -> ! {
        unsafe { write_banner_and_abort() }
    }

    // ===========================================================
    // v0.7-A5 step 16-d — `#[global_allocator]` single-marker site
    // ===========================================================
    //
    // Why panic-runtime: single marker site eliminates the
    // duplicate-symbol class, and this crate already owns
    // `rust_eh_personality` / `__rust_start_panic` / the
    // alloc-error handler.
    //
    // "Resolves first" is NOT automatic, and this comment used to
    // claim it was. Under the LLVM-era link it came from
    // `-Wl,-force_load` ordering; the in-house linker has no such
    // rule — it pulls archive members on demand and builds its
    // address table last-write-wins, so a shim declared here loses
    // to the default `__rust_alloc -> __rdl_alloc` that every
    // sibling staticlib's bundled `core` also defines. User
    // binaries ran on libc malloc from the linker swap until
    // 2026-08-21. Two things now make the claim true, and removing
    // either one silently reverts it:
    //   1. `libtorajs_panic_runtime.a` leads `TORAJS_STATICLIBS`
    //      (`torajs-core/src/staticlibs.rs`);
    //   2. `is_allocator_shim_sym` in
    //      `torajs-link/src/archive_emit.rs` makes this symbol
    //      family first-definition-wins.
    //
    // Why route through `__torajs_libc_malloc / _free / _realloc`
    // (libtorajs_mmalloc-exported `extern "C"`): keeps the
    // allocator state in mmalloc (single source of truth) while
    // satisfying panic-runtime's 0 Cargo deps invariant — the
    // binding is link-level, not Cargo-level.
    //
    // Alignment contract: `__torajs_libc_malloc` guarantees
    // 16-byte alignment via the SHIM_HEADER path; for
    // `Layout::align()` > 16 we over-allocate and store the
    // original base in a one-`usize` pre-header (mirrors the
    // alignment math in `torajs_mmalloc::global_alloc`).

    use core::alloc::GlobalAlloc;
    use core::mem::size_of;

    const ALLOC_HEADER: usize = size_of::<usize>();
    const NATIVE_ALIGN: usize = 16;

    unsafe extern "C" {
        fn __torajs_libc_malloc(size: usize) -> *mut c_void;
        fn __torajs_libc_free(ptr: *mut c_void);
        fn __torajs_libc_realloc(ptr: *mut c_void, new_size: usize) -> *mut c_void;
    }

    /// Zero-sized marker type for the staticlib's
    /// `#[global_allocator]`. All state lives in mmalloc's
    /// `CORE_*` statics; this is a stateless trampoline.
    pub struct TorajsAllocator;

    unsafe impl GlobalAlloc for TorajsAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let size = layout.size();
            let align = layout.align();
            if align <= NATIVE_ALIGN {
                return unsafe { __torajs_libc_malloc(size) as *mut u8 };
            }
            let Some(total) = size
                .checked_add(align - 1)
                .and_then(|n| n.checked_add(ALLOC_HEADER))
            else {
                return core::ptr::null_mut();
            };
            let raw = unsafe { __torajs_libc_malloc(total) } as *mut u8;
            if raw.is_null() {
                return raw;
            }
            let after_hdr = (raw as usize) + ALLOC_HEADER;
            let aligned = (after_hdr + align - 1) & !(align - 1);
            unsafe {
                core::ptr::write((aligned - ALLOC_HEADER) as *mut usize, raw as usize);
            }
            aligned as *mut u8
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            let align = layout.align();
            if align <= NATIVE_ALIGN {
                unsafe { __torajs_libc_free(ptr as *mut c_void) };
                return;
            }
            let raw = unsafe { core::ptr::read((ptr as usize - ALLOC_HEADER) as *const usize) }
                as *mut c_void;
            unsafe { __torajs_libc_free(raw) };
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { self.alloc(layout) };
            if !p.is_null() {
                unsafe { core::ptr::write_bytes(p, 0, layout.size()) };
            }
            p
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let align = layout.align();
            if align <= NATIVE_ALIGN {
                return unsafe { __torajs_libc_realloc(ptr as *mut c_void, new_size) as *mut u8 };
            }
            let new_layout = match Layout::from_size_align(new_size, align) {
                Ok(l) => l,
                Err(_) => return core::ptr::null_mut(),
            };
            let new_ptr = unsafe { self.alloc(new_layout) };
            if new_ptr.is_null() {
                return new_ptr;
            }
            let copy_len = core::cmp::min(layout.size(), new_size);
            unsafe { core::ptr::copy_nonoverlapping(ptr, new_ptr, copy_len) };
            unsafe { self.dealloc(ptr, layout) };
            new_ptr
        }
    }

    #[global_allocator]
    static GLOBAL: TorajsAllocator = TorajsAllocator;
}

// ===========================================================================
// Inactive mode — plain `cargo build` / `cargo test`
// ===========================================================================
//
// Empty placeholder so the staticlib emits a non-zero-byte `.a`. The
// build.rs / include_bytes! pipeline in torajs-core still requires the
// archive to exist; an empty staticlib (just the placeholder fn) is
// valid.

#[cfg(not(torajs_panic_runtime_active))]
mod inactive {
    /// Placeholder so the staticlib has at least one symbol. Never
    /// referenced; dead-strippable. Required because cargo emits a
    /// "no symbols" warning on an empty staticlib that would flunk
    /// the workspace's `warnings = "deny"` lint config.
    #[unsafe(no_mangle)]
    pub extern "C" fn __torajs_panic_runtime_placeholder() {}
}
