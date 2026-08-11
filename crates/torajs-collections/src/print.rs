//! `console.log(map)` / `console.log(set)` pretty-print —
//! nested-print substrate trunk Commits 7 (Map) + 8 (Set), indent
//! threading + width accounting per the inspect wrap trunk.
//!
//! Both walkers iterate the dense `entries: *mut MapEntry` array in
//! insertion order (entries are appended on first set; tombstones
//! mark `hash == 0`). Each live entry's key + value (Map) or key
//! alone (Set) is emitted via `__torajs_print_anyv_inline_at`. bun
//! renders Map / Set keys quoted-when-string (same nested-context
//! rule as Arr cells, unlike object literals' bare key form), so we
//! use the nested-context printer directly — no bare-key variant.
//!
//! Output shape (bun-parity):
//! - empty: `Map {}` (bun omits the size parens on empty)
//! - non-empty: multi-line `Map(N) {\n  k: v,\n  ...\n}` with
//!   fields padded at `indent + 2`, the closing brace at `indent`,
//!   and a trailing comma on the last entry (matches bun exactly).
//!
//! Set form: `Set(N) {\n  v,\n  ...\n}` — value position only.
//!
//! ## Newline policy
//!
//! Both walkers emit no trailing '\n'. The `*_outer` SSA wrappers
//! and the top-level `__torajs_print_anyv` arms append it.

use core::ffi::c_void;

use crate::layout::{MapEntry, as_map};

unsafe extern "C" {
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);
    fn __torajs_io_putc_out(c: i32) -> i32;
    fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32;
    // Line-width estimate primitives (inspect wrap trunk) — hosted
    // in torajs-anyvalue::inspect::formatters.
    fn __torajs_inspect_line_reset(cols: u32);
    fn __torajs_inspect_line_add(n: u32);
}

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_out(b as i32) };
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

#[inline]
unsafe fn put_indent(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}

/// Shared Map / Set entry walk — `with_value` selects the Map
/// `k: v` row form over Set's value-only rows. Fields pad at
/// `indent + 2`, the closer at `indent`; nested composites keep
/// descending via the indent-threaded AnyValue printer. Estimate
/// seeding mirrors the dynobj walker (bun handleFirstProperty:
/// overwrite to parent indent + 1, accumulate across rows).
unsafe fn map_like_print_at(ptr: *const c_void, indent: u32, name: &[u8], with_value: bool) {
    if ptr.is_null() {
        unsafe { put_bytes(b"null") };
        return;
    }
    unsafe {
        let m = &*(as_map(ptr as *mut c_void));
        if m.n_entries == 0 {
            // Bun-specific: empty omits the size in parens
            // (`Map {}` not `Map(0) {}`).
            put_bytes(name);
            put_bytes(b" {}");
            return;
        }
        put_bytes(name);
        put_byte(b'(');
        put_i64(m.n_entries as i64);
        put_bytes(b") {\n");
        __torajs_inspect_line_reset(indent + 1);
        let entries = m.entries;
        let mut any_emitted = false;
        for i in 0..m.n_used {
            let entry = entries.add(i as usize);
            let e = &*(entry as *const MapEntry);
            if e.hash == 0 {
                // Tombstone — entry was deleted; skip.
                continue;
            }
            if any_emitted {
                put_bytes(b",\n");
                __torajs_inspect_line_add(1);
            }
            put_indent(indent + 2);
            __torajs_print_anyv_inline_at(e.key_anyv, indent + 2);
            if with_value {
                put_bytes(b": ");
                __torajs_inspect_line_add(2);
                __torajs_print_anyv_inline_at(e.value_anyv, indent + 2);
            }
            any_emitted = true;
        }
        if any_emitted {
            put_bytes(b",\n");
            __torajs_inspect_line_add(1);
            put_indent(indent);
        }
        put_byte(b'}');
    }
}

/// `console.log(m: Map<K, V>)` walker — no trailing newline,
/// indent 0 (top-level / legacy nested callers).
///
/// # Safety
///
/// `m_ptr` must be NULL or point to a valid `Map` heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_print(m_ptr: *const c_void) {
    unsafe { map_like_print_at(m_ptr, 0, b"Map", true) }
}

/// Indent-threaded Map walker export (inspect wrap trunk).
///
/// # Safety
///
/// Same contract as [`__torajs_map_print`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_print_at(m_ptr: *const c_void, indent: u32) {
    unsafe { map_like_print_at(m_ptr, indent, b"Map", true) }
}

/// SSA dispatcher entry — top-level `console.log(m: Map)` wrapper
/// that resets the line estimate and adds the trailing '\n'.
/// Routed directly by `ssa_lower`'s `(Type::Map, _)` arm so the
/// Type::Map → typed walker dispatch bypasses the AnyValue
/// tag-walker.
///
/// # Safety
///
/// Same as `__torajs_map_print`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_print_outer(m_ptr: *const c_void) {
    unsafe {
        __torajs_inspect_line_reset(0);
        __torajs_map_print(m_ptr);
        __torajs_io_putc_out(b'\n' as i32);
    }
}

/// `console.log(s: Set<T>)` walker — Set is stored as
/// `Map<T, undefined>` so this walks the same MapEntry array but
/// emits only the key column.
///
/// # Safety
///
/// `s_ptr` must be NULL or point to a valid Set heap object (which
/// shares the `Map` layout per torajs-collections).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_print(s_ptr: *const c_void) {
    unsafe { map_like_print_at(s_ptr, 0, b"Set", false) }
}

/// Indent-threaded Set walker export (inspect wrap trunk).
///
/// # Safety
///
/// Same contract as [`__torajs_set_print`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_print_at(s_ptr: *const c_void, indent: u32) {
    unsafe { map_like_print_at(s_ptr, indent, b"Set", false) }
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
        __torajs_inspect_line_reset(0);
        __torajs_set_print(s_ptr);
        __torajs_io_putc_out(b'\n' as i32);
    }
}
