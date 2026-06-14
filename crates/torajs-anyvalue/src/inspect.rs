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
    // Nested-print substrate trunk Commit 4 wire — cross-staticlib
    // externs to `torajs-arr` and `torajs-dynobj` walkers added in
    // Commits 2 and 3. The Tag::Arr / Tag::Obj branches below
    // delegate to these for the pretty form.
    fn __torajs_arr_print_any(arr: *const c_void);
    fn __torajs_obj_print_any(obj: *const c_void);
    // Commit 5 — Date wire. Returns a fresh rc=1 Str holding the
    // ISO-8601 form. The Tag::Date branch prints the payload then
    // rc_dec's the temporary Str to balance allocation.
    fn __torajs_date_to_iso_string(d_ptr: *const c_void) -> *mut u8;
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    // Commit 6 — RegExp wire. Emits `/source/flags` directly via
    // the shared stdout writer (no fresh-Str alloc, no rc_dec
    // dance). Same put_byte family this module uses.
    fn __torajs_regex_print_inline(re_ptr: *const c_void);
    // Commits 7-8 — Map / Set walkers exist in torajs-collections
    // but the SSA dispatcher routes Type::Map / Type::Set directly
    // to their `*_print_outer` wrappers (bypassing this AnyValue
    // tag-walker) because runtime Tag::Map (=15) covers BOTH Map
    // and Set heap blocks — there is no separate Tag::Set. Calling
    // __torajs_map_print on a Set heap object would print the bun
    // `Map(...)` form for what should be `Set(...)`. Runtime tag
    // disambiguation is a follow-up substrate (L3b).
    // Commit 8 — Promise wire. Tag::Promise=8 is unambiguous so
    // the AnyValue walker can route directly here.
    fn __torajs_promise_print(p_ptr: *const c_void);
}

/// SSA dispatcher entry for `console.log(fn_addr: Type::FnSig)` —
/// emits the bun `[Function]` form plus '\n'. Phase 1 narrow
/// (fn-name registry Step 3a) — name omitted; the fn_name_globals
/// scaffold from Steps 1-2 is populated but the link emit (Step 3b)
/// and runtime binary-search helper (Step 4) that would turn the
/// fn_addr into a bun-byte-equal `[Function: name]` form aren't
/// wired yet. This Phase 1 form at least replaces the W-7 raw
/// pointer decimal fallthrough.
///
/// # Safety
///
/// `_fn_addr` may be any 64-bit pattern (raw code-section pointer
/// from `InstKind::FnAddr` lowering); not dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_print_outer(_fn_addr: u64) {
    // Phase 1: emit `[Function]\n` directly, skip the binary search
    // (Step 4 will replace this body with the name-lookup path once
    // the rodata table from Step 3b is materialized).
    write_line(b"[Function]\n");
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
        } else if tag == Tag::Arr as u16 {
            // Nested-print substrate trunk Commit 4 — Tag::Arr
            // (typed Arr<T> heap cell) renders as bun's
            // `[ a, b, c ]` form via the Commit 2 walker. Closes
            // `Any-print arr fallback` wedge (`const a:any = [1,2,3]`).
            // SAFETY: Arr heap layout per torajs-arr::layout.
            unsafe { __torajs_arr_print_any(child) };
            unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
        } else if tag == Tag::DynObj as u16 {
            // Nested-print substrate trunk Commit 4 — Tag::DynObj
            // (object literal / Object.entries row) renders as
            // bun's `{\n  k: v,\n}` form via the Commit 3 walker.
            // Closes `Any-print obj fallback` wedge
            // (`const o:any = {a:1}`). Tag::Obj (static-layout
            // class instance) keeps the `[object]` fallback —
            // class-instance pretty-print requires struct_layouts
            // metadata and is a separate substrate trunk (W-J).
            // SAFETY: dynobj layout per torajs-dynobj::layout.
            unsafe { __torajs_obj_print_any(child) };
            unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
        } else if tag == Tag::Date as u16 {
            // Commit 5 — Date wire. Reuses the existing
            // __torajs_date_to_iso_string which returns a fresh
            // rc=1 Str holding `YYYY-MM-DDTHH:MM:SS.sssZ` (24
            // bytes). The Str payload is walked directly through
            // `put_str_cell_inline` (no quotes — bun prints Date
            // values unquoted, e.g. `1970-01-01T00:00:00.000Z`
            // not `"1970-..."`), then rc_dec'd to balance the
            // fresh allocation. Cell ptr cast as *mut for rc_dec
            // (rc operations don't actually mutate the pointee
            // beyond the refcount header).
            // SAFETY: Date layout per torajs-date::layout.
            let iso = unsafe { __torajs_date_to_iso_string(child) };
            if !iso.is_null() {
                unsafe { put_str_cell_inline(iso as *const c_void) };
                unsafe { __torajs_rc_dec(iso as *mut c_void) };
            }
            unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
        } else if tag == Tag::RegExp as u16 {
            // Commit 6 — RegExp wire. Bun prints RegExp values as
            // `/source/flags` (unquoted, both top-level and nested,
            // unlike Str which gains `"..."` inside arr / obj).
            // SAFETY: RegExp layout per torajs-regex::regex.
            unsafe { __torajs_regex_print_inline(child) };
            unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
        } else if tag == Tag::Promise as u16 {
            // Commit 8 — Promise wire. Emits the bun minimal form
            // `Promise { <pending|resolved|rejected> }` — bun
            // deliberately doesn't surface value / reason in the
            // default console.log inspect.
            // SAFETY: Promise layout per torajs-promise::layout.
            unsafe { __torajs_promise_print(child) };
            unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
        } else if tag == Tag::Closure as u16 {
            // Phase 1 narrow (fn-name registry Step 3a) — closure
            // heap object reached via Type::Any binding. Emits
            // `[Function]` (no name); Phase 2 lookup that turns the
            // closure's `fn_addr@+8` into `[Function: <name>]` via
            // the rodata table from Steps 3b-4 lands once the
            // archive_emit substrate ships.
            write_line(b"[Function]\n");
        } else {
            write_line(b"[object]\n");
        }
        return;
    }
    write_line(b"[unknown-any-tag]\n");
}

/// Emit the bytes of a Str / Substr heap cell **unquoted** —
/// nested-print substrate trunk Commit 4 helper for dynobj-key /
/// Map-key / Set-elem-string callers that need raw key bytes without
/// the `"..."` wrapper [`__torajs_print_anyv_inline`] adds for
/// nested-context string values.
///
/// `cell` must point to a Tag::Str heap object (real Str layout
/// `{len@+8, data@+16}` OR Substr view `{len@+8, parent@+16,
/// offset@+24}`); the inspect path picks via `FLAG_SUBSTR_VIEW`.
/// Non-Str tags emit nothing (callers feed Str ptrs from
/// `__torajs_dynobj_iter_key` / Map key slots / etc by contract).
///
/// # Safety
///
/// `cell` must point to a valid Tag::Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_str_cell_unquoted(cell: *const c_void) {
    unsafe {
        if cell.is_null() {
            return;
        }
        let tag = heap_type_tag(cell);
        if tag != Tag::Str as u16 {
            return;
        }
        let flags = heap_flags(cell);
        if flags & SUBSTR_VIEW_FLAG != 0 {
            put_substr_cell_inline(cell);
        } else {
            put_str_cell_inline(cell);
        }
    }
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
        // Nested context — strings get `"..."` quotes (bun inspect
        // form, e.g. `[ "hi" ]` not `[ hi ]`). Top-level
        // `__torajs_print_anyv` skips the quoting (matches bun's
        // `console.log("hi")` → `hi`).  Escapes (`"`, `\`, control
        // chars) not yet honoured — pure-ASCII fixtures only for
        // this commit; full bun escape table is a later commit.
        let len = short_str_len(v) as usize;
        let bytes = short_str_bytes(v);
        unsafe { put_byte(b'"') };
        for &b in &bytes[..len] {
            unsafe { put_byte(b) };
        }
        unsafe { put_byte(b'"') };
        return;
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: live heap ptr per caller invariant.
        let tag = unsafe { heap_type_tag(child) };
        if tag == Tag::Str as u16 {
            // Substr vs real Str — flag bit 10 disambiguates same as
            // the standalone print_anyv path above. Nested context
            // → `"..."` quotes (see ShortStr arm above for rationale
            // and escape caveat).
            let flags = unsafe { heap_flags(child) };
            unsafe { put_byte(b'"') };
            if flags & SUBSTR_VIEW_FLAG != 0 {
                unsafe { put_substr_cell_inline(child) };
            } else {
                unsafe { put_str_cell_inline(child) };
            }
            unsafe { put_byte(b'"') };
        } else if tag == Tag::Arr as u16 {
            // Commit 4 — recursion enable. Nested Arr<Any> /
            // Arr<Arr<_>> walks via this branch (the outer
            // __torajs_arr_print_any calls
            // __torajs_print_anyv_inline on each slot; if that slot
            // is itself a Tag::Arr cell, we land here and recurse
            // back into __torajs_arr_print_any). No '\n' (inline
            // contract). Cyclic graphs trigger SO with no
            // `[Circular]` sentinel — known limitation tracked in
            // L3b (bun emits `[Circular *N]`, v0.7 doesn't).
            // SAFETY: Tag::Arr layout per torajs-arr::layout.
            unsafe { __torajs_arr_print_any(child) };
        } else if tag == Tag::DynObj as u16 {
            // Commit 4 — Tag::DynObj nested. Same recursion form
            // as Tag::Arr above.
            // SAFETY: dynobj layout per torajs-dynobj::layout.
            unsafe { __torajs_obj_print_any(child) };
        } else if tag == Tag::Date as u16 {
            // Commit 5 — nested Date prints unquoted (bun:
            // `[ 1970-01-01T00:00:00.000Z ]` not
            // `[ "1970-..." ]`). Same fresh-Str + rc_dec dance as
            // the top-level Tag::Date arm above, minus the
            // trailing '\n'.
            // SAFETY: Date layout per torajs-date::layout.
            let iso = unsafe { __torajs_date_to_iso_string(child) };
            if !iso.is_null() {
                unsafe { put_str_cell_inline(iso as *const c_void) };
                unsafe { __torajs_rc_dec(iso as *mut c_void) };
            }
        } else if tag == Tag::RegExp as u16 {
            // Commit 6 — nested RegExp prints unquoted same as
            // top-level (bun: `[ /abc/g ]` not `[ "/abc/g" ]`).
            // SAFETY: RegExp layout per torajs-regex::regex.
            unsafe { __torajs_regex_print_inline(child) };
        } else if tag == Tag::Promise as u16 {
            // Commit 8 — nested Promise prints same minimal form.
            // SAFETY: Promise layout per torajs-promise::layout.
            unsafe { __torajs_promise_print(child) };
        } else if tag == Tag::Closure as u16 {
            // Phase 1 narrow — nested closure prints `[Function]`
            // (no '\n', same wedge as top-level Tag::Closure above).
            unsafe { put_bytes(b"[Function]") };
        } else {
            // All other composite / typed-receiver tags
            // (Tag::Obj / Tag::Closure / Tag::Symbol / Tag::BigInt /
            // Tag::Map / Tag::Set / Tag::Promise / Tag::Date /
            // Tag::RegExp / Tag::Response / Tag::Weak* /
            // Tag::MapIter / Tag::ArrIter / Tag::AccessorPair / etc)
            // fall back to `[object]` (no '\n'). Commits 5-8 wire
            // each tag to its typed walker.
            unsafe { put_bytes(b"[object]") };
        }
        return;
    }
    unsafe { put_bytes(b"[unknown-any-tag]") };
}
