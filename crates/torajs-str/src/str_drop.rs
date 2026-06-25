//! Str drop / free FFI shims — `__torajs_str_drop` (scope-end
//! decrement) + `__torajs_str_free` (pool-aware free). Extracted
//! from `block.rs` to keep that file under the 500-prod-LOC
//! file-size hard limit (`rules/common/file-size.md`). Pure
//! mechanical pull, no semantic change.

use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

use crate::block::StrBlock;

/// `__torajs_str_drop(s)` — Str scope-end decrement. The dominant
/// drop path emitted by ssa_lower for every Str-typed local.
/// Mirrors pre-rewrite `ssa_inkwell::define_str_drop`:
///
/// ```text
/// if s == NULL: return
/// if (s.flags & FLAG_STATIC_LITERAL) != 0: return  // .rodata
/// s.refcount -= 1
/// if s.refcount == 0: __torajs_str_free(s)         // pool-aware!
/// ```
///
/// **At rc==0 we route through [`__torajs_str_free`] (pool-aware),
/// NOT libc free directly**. The IR-emitted `define_str_drop` took
/// `__torajs_str_free` as its "free" parameter (the parameter name
/// in the IR builder was just `free` but the bound symbol was
/// `str_free`); the pool fast-path feeds the small-Str LIFO so
/// subsequent `__torajs_str_alloc_pooled` calls pop instead of
/// malloc. An earlier ship of this fn called libc::free directly
/// and broke 5 regex fixtures — the regex engine's hot path round-
/// trips Str blocks through the pool; bypassing the pool feed left
/// holes that next-alloc filled with stale-zeroed memory, manifest
/// as `"hello"` → `"he\0\0\0\0\0\0\0\0o"` byte corruption.
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
        // SAFETY: rc reached 0; we own the last reference.
        // __torajs_str_free does pool-push for small blocks and
        // libc::free for the rest. Matches what the IR-emitted
        // define_str_drop dispatched to (the `str_free` parameter).
        unsafe { __torajs_str_free(s) };
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
