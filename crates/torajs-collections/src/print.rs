//! `console.log(map)` / `console.log(set)` pretty-print —
//! nested-print substrate trunk Commits 7 (Map) + 8 (Set).
//!
//! Both walkers iterate the dense `entries: *mut MapEntry` array in
//! insertion order (entries are appended on first set; tombstones
//! mark `hash == 0`). Each live entry's key + value (Map) or key
//! alone (Set) is emitted via `__torajs_print_anyv_inline` (Commit 1
//! substrate). bun renders Map / Set keys quoted-when-string (same
//! nested-context rule as Arr cells, unlike object literals' bare
//! key form), so we use `__torajs_print_anyv_inline` directly — no
//! unquoted variant.
//!
//! Output shape (bun-parity):
//! - empty: `Map(0) {}\n` (top-level) / `Map(0) {}` (nested)
//! - non-empty: multi-line `Map(N) {\n  k: v,\n  ...\n}\n` with
//!   `key_anyv`'s bun form (e.g. `"a"` for Str, `1` for I64) +
//!   trailing comma on the last entry (matches bun exactly).
//!
//! Set form: `Set(N) {\n  v,\n  ...\n}` — value position only.
//!
//! ## Newline policy
//!
//! Both helpers emit no trailing '\n'. The caller
//! (`__torajs_print_anyv` Tag::Map / Tag::Set arms in
//! `torajs-anyvalue::inspect`) appends '\n' at the top level; the
//! nested `__torajs_print_anyv_inline` arms omit it.

use core::ffi::c_void;

use crate::layout::{MapEntry, as_map};

unsafe extern "C" {
    fn __torajs_print_anyv_inline(v: u64);
    fn __torajs_io_putc_stdout(c: i32) -> i32;
    fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32;
}

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_stdout(b as i32) };
}

#[inline]
unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

#[inline]
unsafe fn put_i64(v: i64) {
    let mut buf = [0u8; 64];
    let n = unsafe { __torajs_fmt_itoa(v, buf.as_mut_ptr(), 64) };
    if n > 0 {
        let n = (n as usize).min(63);
        unsafe { put_bytes(&buf[..n]) };
    }
}

/// `console.log(m: Map<K, V>)` walker — emits `Map(N) {\n k: v,\n}`
/// with no trailing newline. Iterates the dense entries array
/// (insertion order, `hash == 0` tombstones skipped).
///
/// # Safety
///
/// `m_ptr` must point to a valid `Map` heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_print(m_ptr: *const c_void) {
    if m_ptr.is_null() {
        unsafe { put_bytes(b"null") };
        return;
    }
    unsafe {
        let m = &*(as_map(m_ptr as *mut c_void));
        if m.n_entries == 0 {
            // Bun-specific: empty Map omits the size in parens
            // (`Map {}` not `Map(0) {}`). Non-empty form keeps the
            // size — same asymmetry bun applies to Set.
            put_bytes(b"Map {}");
            return;
        }
        put_bytes(b"Map(");
        put_i64(m.n_entries as i64);
        put_bytes(b") {\n");
        let entries = m.entries;
        let mut any_emitted = false;
        for i in 0..m.n_used {
            let entry = entries.add(i as usize);
            let e = &*(entry as *const MapEntry);
            if e.hash == 0 {
                // Tombstone — entry was deleted; skip.
                continue;
            }
            put_bytes(b"  ");
            __torajs_print_anyv_inline(e.key_anyv);
            put_bytes(b": ");
            __torajs_print_anyv_inline(e.value_anyv);
            put_bytes(b",\n");
            any_emitted = true;
        }
        // Defensive — if hash==0 mid-array makes everything look
        // tombstoned (shouldn't happen with positive-bias hash but
        // belt-and-braces) emit the trailing brace alone.
        if !any_emitted {
            put_bytes(b"}");
        } else {
            put_bytes(b"}");
        }
    }
}

/// SSA dispatcher entry — top-level `console.log(m: Map)` wrapper
/// that adds the trailing '\n' after the Map walker. Routed
/// directly by `ssa_lower`'s `(Type::Map, _)` arm so the Type::Map
/// → typed walker dispatch bypasses the AnyValue tag-walker (which
/// can't distinguish Tag::Map / Tag::Set at runtime — they share
/// tag 15).
///
/// # Safety
///
/// Same as `__torajs_map_print`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_print_outer(m_ptr: *const c_void) {
    unsafe {
        __torajs_map_print(m_ptr);
        __torajs_io_putc_stdout(b'\n' as i32);
    }
}

/// `console.log(s: Set<T>)` walker — emits `Set(N) {\n v,\n}` with
/// no trailing newline. Set is stored as `Map<T, undefined>` so we
/// iterate the same MapEntry array but emit only the key column.
///
/// # Safety
///
/// `s_ptr` must point to a valid Set heap object (which shares the
/// `Map` layout per torajs-collections).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_print(s_ptr: *const c_void) {
    if s_ptr.is_null() {
        unsafe { put_bytes(b"null") };
        return;
    }
    unsafe {
        let m = &*(as_map(s_ptr as *mut c_void));
        if m.n_entries == 0 {
            // Bun: empty Set is `Set {}` not `Set(0) {}` (same rule
            // as the Map walker above).
            put_bytes(b"Set {}");
            return;
        }
        put_bytes(b"Set(");
        put_i64(m.n_entries as i64);
        put_bytes(b") {\n");
        let entries = m.entries;
        for i in 0..m.n_used {
            let entry = entries.add(i as usize);
            let e = &*(entry as *const MapEntry);
            if e.hash == 0 {
                continue;
            }
            put_bytes(b"  ");
            __torajs_print_anyv_inline(e.key_anyv);
            put_bytes(b",\n");
        }
        put_bytes(b"}");
    }
}

/// SSA dispatcher entry — top-level `console.log(s: Set)` wrapper.
/// Mirrors `__torajs_map_print_outer` for the Type::Set arm.
///
/// # Safety
///
/// Same as `__torajs_set_print`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_print_outer(s_ptr: *const c_void) {
    unsafe {
        __torajs_set_print(s_ptr);
        __torajs_io_putc_stdout(b'\n' as i32);
    }
}
