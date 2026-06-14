//! Any-tag inspection — `typeof v` + `console.log(v)` +
//! `ToBoolean(v)` on NaN-box [`AnyValue`] — port of
//! `runtime_str.c` L795-833 + L1040-1157.
//!
//! Two extern fns that read an [`AnyValue`]'s NaN-box discriminant
//! and route:
//!
//! - [`__torajs_anyv_typeof`] — returns a fresh Str holding the ES
//!   `typeof` result for the value (`"object"` / `"undefined"` /
//!   `"boolean"` / `"number"` / `"string"` / `"function"` /
//!   `"symbol"` / `"bigint"`).
//!
//! - [`__torajs_print_anyv`] — `console.log(v)` dispatch. Routes to
//!   the IR-emitted `print_i64` / `print_f64` / `print_bool` and
//!   `__torajs_str_print` based on the NaN-box predicate. Cell case
//!   reads the heap header's universal `type_tag`; only `Str` gets
//!   pretty-printed today, everything else falls back to
//!   `"[object]\n"` (heap-typed pretty-print is a later wedge).
//!
//! - [`__torajs_print_anyv_inline`] — nested-print substrate trunk
//!   Commit 1 (2026-06-14). Same dispatch tree as
//!   `__torajs_print_anyv` but every arm emits its bytes WITHOUT a
//!   trailing newline; used by Commit 2+ helpers
//!   (`__torajs_arr_print_any` / `__torajs_obj_print_any` / etc) to
//!   format AnyValue cells inside a larger composite. Cannot reuse
//!   the IR-emitted `print_i64 / f64 / bool` or
//!   `__torajs_str_print` — those all emit their own '\n'. Drops to
//!   0-libc `__torajs_fmt_itoa` / `__torajs_fmt_dtoa` +
//!   `__torajs_io_putc_stdout` byte writers instead. Tag::Arr /
//!   Tag::Obj / Tag::Map / Tag::Set / Tag::Promise / Tag::Date /
//!   Tag::RegExp / Tag::Closure cell printers are stubbed to
//!   `"[object]"` for Commit 1 and replaced by the typed walkers in
//!   Commits 2-8.
//!
//! Cross-tier symbols resolved at `tr build` link time:
//! - `print_i64` / `print_f64` / `print_bool` — `libtorajs_print.a`
//!   (buffered via torajs-io).
//! - `__torajs_str_print` — `libtorajs_str.a`.
//! - `__torajs_str_alloc_pooled` — `libtorajs_str.a`.
//! - `__torajs_fmt_itoa` / `__torajs_fmt_dtoa` —
//!   `libtorajs_fmt.a` (0-libc int/float → decimal; same path
//!   `torajs-arr::print` uses).
//! - `__torajs_io_putc_stdout` — `libtorajs_io.a` (per-byte
//!   stdout writer; shares the same stdio buffer as the IR-emitted
//!   `print_*` family).

use core::ffi::c_void;

use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined, short_str_bytes, short_str_len,
};
use torajs_rc::{HeapHeader, Tag};

const STR_HDR_SIZE: usize = 16;

// Str / Substr layout — mirror of `torajs-str::layout` constants.
// Duplicated to avoid an `torajs-anyvalue → torajs-str` Cargo dep
// edge (sibling crates), same cross-tier mirror pattern
// `torajs-arr::print` already uses for these constants. Any future
// drift in those layouts would already break the upstream Str / Substr
// printers — there is no "drift opportunity" introduced by mirroring.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const STR_FLAG_IS_LATIN1: u16 = 0x0002;
const HDR_FLAGS_OFF: usize = 6;
const SUBSTR_LEN_OFF: usize = 8;
const SUBSTR_PARENT_OFF: usize = 16;
const SUBSTR_OFFSET_OFF: usize = 24;

unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_print(s: *const u8);
    fn __torajs_substr_print(v: *const u8);
    fn print_i64(n: i64);
    fn print_f64(d: f64);
    fn print_bool(b: bool);
    fn __torajs_io_putc_stdout(c: i32) -> i32;
    // 0-libc decimal — `torajs-fmt`. Mirrors the extern declaration
    // shape `torajs-arr::print` uses (same staticlib provider).
    fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32;
    fn __torajs_fmt_dtoa(d: f64, out_buf: *mut u8, out_cap: usize) -> i32;
}

/// Mirror of `torajs_str::substr::FLAG_SUBSTR_VIEW` (bit 10 of
/// `HeapHeader::flags`). Hardcoded here to avoid a `torajs-anyvalue
/// → torajs-str` dep edge; the bit value is part of the substr ABI
/// (set by `__torajs_substr_create` + split-tail emit) so any future
/// drift would already break drop-path dispatch. Bits 0-9 are taken
/// (see torajs-rc + torajs-str flag tables).
const SUBSTR_VIEW_FLAG: u16 = 1 << 10;

#[inline]
unsafe fn heap_flags(child: *const c_void) -> u16 {
    unsafe { (*(child as *const HeapHeader)).flags }
}

#[inline]
fn alloc_literal(s: &[u8]) -> *mut u8 {
    let p = unsafe { __torajs_str_alloc_pooled(s.len() as u64) };
    if !s.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(STR_HDR_SIZE), s.len()) };
    }
    p
}

#[inline]
fn write_line(s: &[u8]) {
    for &b in s {
        unsafe { __torajs_io_putc_stdout(b as i32) };
    }
}

// ============================================================
// Inline byte writers — 0-libc, no '\n' (used by print_anyv_inline
// + Commit 2+ composite walkers)
// ============================================================

#[inline]
pub(crate) unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_stdout(b as i32) };
}

#[inline]
pub(crate) unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

/// 0-libc i64 → decimal bytes (no trailing newline). Mirrors
/// `torajs-arr::print::put_snprintf_i64` (same `__torajs_fmt_itoa`
/// staticlib provider).
#[inline]
pub(crate) unsafe fn put_i64_inline(v: i64) {
    let mut buf = [0u8; 64];
    let n = unsafe { __torajs_fmt_itoa(v, buf.as_mut_ptr(), 64) };
    if n > 0 {
        let n = (n as usize).min(63);
        unsafe { put_bytes(&buf[..n]) };
    }
}

/// 0-libc f64 → decimal bytes per JS-spec shortest-roundtrip
/// (NaN / Infinity / -Infinity special cases handled by
/// `__torajs_fmt_dtoa` itself). Mirrors
/// `torajs-arr::print::put_snprintf_f64_g`.
#[inline]
pub(crate) unsafe fn put_f64_inline(v: f64) {
    let mut buf = [0u8; 64];
    let n = unsafe { __torajs_fmt_dtoa(v, buf.as_mut_ptr(), 64) };
    if n > 0 {
        let n = (n as usize).min(63);
        unsafe { put_bytes(&buf[..n]) };
    }
}

/// Emit the Latin-1 or UTF-16-LE payload of an encoded Str cell as
/// UTF-8 bytes. Duplicated from `torajs-arr::print::put_str_payload`
/// (sibling-crate textbook mirror; same constraint applies — the
/// upstream Str encoding is fixed by `torajs-str::layout`, drift
/// would already break the standalone `__torajs_str_print` walker).
unsafe fn put_str_payload_utf8(payload: &[u8], is_latin1: bool) {
    unsafe {
        if is_latin1 {
            for &b in payload {
                if b <= 0x7F {
                    put_byte(b);
                } else {
                    put_byte(0xC0 | (b >> 6));
                    put_byte(0x80 | (b & 0x3F));
                }
            }
            return;
        }
        let mut i = 0usize;
        while i + 1 < payload.len() {
            let cu = u16::from_le_bytes([payload[i], payload[i + 1]]) as u32;
            let cp = if (0xD800..=0xDBFF).contains(&cu) && i + 3 < payload.len() {
                let lo = u16::from_le_bytes([payload[i + 2], payload[i + 3]]) as u32;
                if (0xDC00..=0xDFFF).contains(&lo) {
                    i += 4;
                    0x10000 + ((cu - 0xD800) << 10) + (lo - 0xDC00)
                } else {
                    i += 2;
                    cu
                }
            } else {
                i += 2;
                cu
            };
            if cp <= 0x7F {
                put_byte(cp as u8);
            } else if cp <= 0x7FF {
                put_byte((0xC0 | (cp >> 6)) as u8);
                put_byte((0x80 | (cp & 0x3F)) as u8);
            } else if cp <= 0xFFFF {
                put_byte((0xE0 | (cp >> 12)) as u8);
                put_byte((0x80 | ((cp >> 6) & 0x3F)) as u8);
                put_byte((0x80 | (cp & 0x3F)) as u8);
            } else {
                put_byte((0xF0 | (cp >> 18)) as u8);
                put_byte((0x80 | ((cp >> 12) & 0x3F)) as u8);
                put_byte((0x80 | ((cp >> 6) & 0x3F)) as u8);
                put_byte((0x80 | (cp & 0x3F)) as u8);
            }
        }
    }
}

/// Emit a Tag::Str cell payload (real Str layout — `len@+8`,
/// `bytes@+16`, latin1/utf16 picked from `HeapHeader::flags @+6`)
/// without a trailing newline. Mirrors `__torajs_str_print`'s walk
/// minus the final '\n'.
unsafe fn put_str_cell_inline(child: *const c_void) {
    unsafe {
        let p = child as *const u8;
        let len = *(p.add(STR_LEN_OFF) as *const u32) as usize;
        if len == 0 {
            return;
        }
        let flags = *(p.add(HDR_FLAGS_OFF) as *const u16);
        let is_latin1 = (flags & STR_FLAG_IS_LATIN1) != 0;
        let payload_len = if is_latin1 { len } else { len * 2 };
        let bytes = core::slice::from_raw_parts(p.add(STR_DATA_OFF), payload_len);
        put_str_payload_utf8(bytes, is_latin1);
    }
}

/// Emit a Substr view payload (slice of its parent Str) without a
/// trailing newline. Substr layout: `len@+8`, `parent_ptr@+16`,
/// `byte_offset@+24` (`torajs-str::substr`). Mirrors
/// `__torajs_substr_print`'s walk minus the final '\n'.
unsafe fn put_substr_cell_inline(child: *const c_void) {
    unsafe {
        let p = child as *const u8;
        let len = *(p.add(SUBSTR_LEN_OFF) as *const u32) as usize;
        if len == 0 {
            return;
        }
        let parent = *(p.add(SUBSTR_PARENT_OFF) as *const *const u8);
        let offset = *(p.add(SUBSTR_OFFSET_OFF) as *const u32) as usize;
        if parent.is_null() {
            return;
        }
        let flags = *(parent.add(HDR_FLAGS_OFF) as *const u16);
        let is_latin1 = (flags & STR_FLAG_IS_LATIN1) != 0;
        let stride = if is_latin1 { 1 } else { 2 };
        let bytes =
            core::slice::from_raw_parts(parent.add(STR_DATA_OFF + offset * stride), len * stride);
        put_str_payload_utf8(bytes, is_latin1);
    }
}

#[inline]
unsafe fn heap_type_tag(child: *const c_void) -> u16 {
    unsafe { (*(child as *const HeapHeader)).type_tag }
}

/// `typeof v` per ES §13.5.3 — NaN-box [`AnyValue`] entry point.
/// Returns a fresh Str. Dispatches on the immediate NaN-box
/// predicates (no heap struct read).
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object
/// (only the `HeapHeader::type_tag` at +4 is read).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_typeof(v: AnyValue) -> *mut u8 {
    if is_null(v) {
        return alloc_literal(b"object");
    }
    if is_undefined(v) {
        return alloc_literal(b"undefined");
    }
    if is_bool(v) {
        return alloc_literal(b"boolean");
    }
    if is_int32(v) || is_double(v) {
        return alloc_literal(b"number");
    }
    // Step 8c — ShortStr is a string at the JS surface even though
    // its bits live inline in the AnyValue immediate; report
    // `typeof` as `"string"` BEFORE the cell-pointer branch (which
    // would mis-dispatch to `"object"` via the fall-through arm).
    if is_short_str(v) {
        return alloc_literal(b"string");
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: cell pointer is non-null per is_cell guarantee +
        // caller invariant says it points to a live heap object.
        let tag = unsafe { heap_type_tag(child) };
        let kind: &[u8] = if tag == Tag::Str as u16 {
            b"string"
        } else if tag == Tag::Closure as u16 {
            b"function"
        } else if tag == Tag::Symbol as u16 {
            b"symbol"
        } else if tag == Tag::BigInt as u16 {
            b"bigint"
        } else {
            // OBJ / ARR / REGEX / DATE / WEAK* / DYNOBJ / MAP* /
            // ARR_ITER → "object"
            b"object"
        };
        return alloc_literal(kind);
    }
    // Defensive — uninitialized slot (v == 0) reads as "object"
    // (matches `typeof null` per spec).
    alloc_literal(b"object")
}

/// `console.log(v)` single-arg dispatch — NaN-box [`AnyValue`]
/// entry point. Dispatches on the immediate NaN-box predicates.
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_anyv(v: AnyValue) {
    if is_null(v) {
        write_line(b"null\n");
        return;
    }
    if is_undefined(v) {
        write_line(b"undefined\n");
        return;
    }
    if is_bool(v) {
        // SAFETY: extern fn callable from this no_std-ish module.
        unsafe { print_bool(as_bool(v)) };
        return;
    }
    if is_int32(v) {
        let n = as_int32(v) as i64;
        // SAFETY: as above.
        unsafe { print_i64(n) };
        return;
    }
    if is_double(v) {
        let d = as_double(v);
        // SAFETY: as above.
        unsafe { print_f64(d) };
        return;
    }
    // Step 8c — ShortStr inline-print path. No heap alloc: read the
    // 8-bit length + 5-byte payload off the immediate and dump
    // bytes through `putchar`. Mirrors how `__torajs_str_print`
    // emits Heap+Str bytes, but skips the materialize round-trip
    // entirely (Heap+Str path goes through __torajs_str_print
    // below).
    if is_short_str(v) {
        let len = short_str_len(v) as usize;
        let bytes = short_str_bytes(v);
        for &b in &bytes[..len] {
            // SAFETY: __torajs_io_putc_stdout takes any i32 byte value.
            unsafe { __torajs_io_putc_stdout(b as i32) };
        }
        unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
        return;
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: live heap ptr per caller invariant.
        let tag = unsafe { heap_type_tag(child) };
        if tag == Tag::Str as u16 {
            // Tag::Str covers both real Str (`{len@+8, data@+16}`)
            // AND Substr view (`{len@+8, parent@+16, offset@+24}`).
            // Disambiguate via `FLAG_SUBSTR_VIEW`: set on every Substr
            // (standalone via `__torajs_substr_create`, inline via
            // split-tail emit) so `__torajs_str_print`'s
            // "read inline bytes @ +16" walker doesn't garble the
            // parent-ptr field. The substr-aware printer reads parent
            // + offset and prints the parent's payload slice.
            let flags = unsafe { heap_flags(child) };
            if flags & SUBSTR_VIEW_FLAG != 0 {
                // SAFETY: Substr layout per torajs-str::substr.
                // substr_print writes its own trailing newline.
                unsafe { __torajs_substr_print(child as *const u8) };
            } else {
                // SAFETY: Tag::Str header layout — print walker reads
                // len@+8 and bytes@+16.
                unsafe { __torajs_str_print(child as *const u8) };
            }
        } else {
            write_line(b"[object]\n");
        }
        return;
    }
    write_line(b"[unknown-any-tag]\n");
}

/// `console.log(...)` inline-print entry — same NaN-box dispatch
/// as [`__torajs_print_anyv`] but emits NO trailing newline.
/// Substrate for the nested-print Commit 2+ walkers
/// (`__torajs_arr_print_any` / `__torajs_obj_print_any` / typed
/// receiver helpers) — they call this fn on each cell of a
/// composite so it can be embedded mid-line.
///
/// Tag::Arr / Tag::Obj / Tag::Map / Tag::Set / Tag::Promise /
/// Tag::Date / Tag::RegExp / Tag::Closure / Tag::Symbol /
/// Tag::BigInt all degrade to `"[object]"` (without '\n') in this
/// Commit 1 scaffold. Commits 2-8 patch each into the proper
/// typed walker. The standalone `__torajs_print_anyv` above keeps
/// its current Tag::Str / Tag::Substr behaviour byte-equal — no
/// caller change yet.
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object
/// matching its `HeapHeader::type_tag` layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_anyv_inline(v: AnyValue) {
    if is_null(v) {
        unsafe { put_bytes(b"null") };
        return;
    }
    if is_undefined(v) {
        unsafe { put_bytes(b"undefined") };
        return;
    }
    if is_bool(v) {
        unsafe { put_bytes(if as_bool(v) { b"true" } else { b"false" }) };
        return;
    }
    if is_int32(v) {
        unsafe { put_i64_inline(as_int32(v) as i64) };
        return;
    }
    if is_double(v) {
        unsafe { put_f64_inline(as_double(v)) };
        return;
    }
    if is_short_str(v) {
        let len = short_str_len(v) as usize;
        let bytes = short_str_bytes(v);
        for &b in &bytes[..len] {
            unsafe { put_byte(b) };
        }
        return;
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: live heap ptr per caller invariant.
        let tag = unsafe { heap_type_tag(child) };
        if tag == Tag::Str as u16 {
            // Substr vs real Str — flag bit 10 disambiguates same as
            // the standalone print_anyv path above.
            let flags = unsafe { heap_flags(child) };
            if flags & SUBSTR_VIEW_FLAG != 0 {
                unsafe { put_substr_cell_inline(child) };
            } else {
                unsafe { put_str_cell_inline(child) };
            }
        } else {
            // All composite / typed-receiver tags fall back to
            // `[object]` for Commit 1. Commits 2-8 wire each tag to
            // its real walker.
            unsafe { put_bytes(b"[object]") };
        }
        return;
    }
    unsafe { put_bytes(b"[unknown-any-tag]") };
}
