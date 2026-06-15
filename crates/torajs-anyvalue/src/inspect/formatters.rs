//! Inspect / nested-print shared helpers — consts, extern decls,
//! 0-libc byte writers, and Str / Substr payload walkers used by the
//! sibling `any` (top-level `__torajs_print_anyv*`) and
//! `tag_dispatch` (`__torajs_print_anyv_inline`) modules.
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

use torajs_rc::HeapHeader;

pub(super) const STR_HDR_SIZE: usize = 16;

// Str / Substr layout — mirror of `torajs-str::layout` constants.
// Duplicated to avoid an `torajs-anyvalue → torajs-str` Cargo dep
// edge (sibling crates), same cross-tier mirror pattern
// `torajs-arr::print` already uses for these constants. Any future
// drift in those layouts would already break the upstream Str / Substr
// printers — there is no "drift opportunity" introduced by mirroring.
pub(super) const STR_LEN_OFF: usize = 8;
pub(super) const STR_DATA_OFF: usize = 16;
pub(super) const STR_FLAG_IS_LATIN1: u16 = 0x0002;
pub(super) const HDR_FLAGS_OFF: usize = 6;
pub(super) const SUBSTR_LEN_OFF: usize = 8;
pub(super) const SUBSTR_PARENT_OFF: usize = 16;
pub(super) const SUBSTR_OFFSET_OFF: usize = 24;

/// Mirror of `torajs_str::substr::FLAG_SUBSTR_VIEW` (bit 10 of
/// `HeapHeader::flags`). Hardcoded here to avoid a `torajs-anyvalue
/// → torajs-str` dep edge; the bit value is part of the substr ABI
/// (set by `__torajs_substr_create` + split-tail emit) so any future
/// drift would already break drop-path dispatch. Bits 0-9 are taken
/// (see torajs-rc + torajs-str flag tables).
pub(super) const SUBSTR_VIEW_FLAG: u16 = 1 << 10;

/// Closure heap layout: `HeapHeader` at offset 0, fn body vaddr at
/// offset 8, drop_fn vaddr at offset 16, then per-captured-env
/// payload. Mirrors
/// `crates/torajs-dynobj/src/accessor.rs::CLOSURE_FN_ADDR_OFF` —
/// the closure ABI is shared substrate, so the two `8`s must move
/// together if either ever changes.
pub(super) const CLOSURE_FN_ADDR_OFF: usize = 8;

unsafe extern "C" {
    pub(super) fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    pub(super) fn __torajs_str_print(s: *const u8);
    pub(super) fn __torajs_substr_print(v: *const u8);
    pub(super) fn print_i64(n: i64);
    pub(super) fn print_f64(d: f64);
    pub(super) fn print_bool(b: bool);
    pub(super) fn __torajs_io_putc_stdout(c: i32) -> i32;
    // 0-libc decimal — `torajs-fmt`. Mirrors the extern declaration
    // shape `torajs-arr::print` uses (same staticlib provider).
    pub(super) fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32;
    pub(super) fn __torajs_fmt_dtoa(d: f64, out_buf: *mut u8, out_cap: usize) -> i32;
    // Nested-print substrate trunk Commit 4 wire — cross-staticlib
    // externs to `torajs-arr` and `torajs-dynobj` walkers added in
    // Commits 2 and 3. The Tag::Arr / Tag::Obj branches below
    // delegate to these for the pretty form.
    pub(super) fn __torajs_arr_print_any(arr: *const c_void);
    pub(super) fn __torajs_obj_print_any(obj: *const c_void);
    // Commit 5 — Date wire. Returns a fresh rc=1 Str holding the
    // ISO-8601 form. The Tag::Date branch prints the payload then
    // rc_dec's the temporary Str to balance allocation.
    pub(super) fn __torajs_date_to_iso_string(d_ptr: *const c_void) -> *mut u8;
    pub(super) fn __torajs_rc_dec(p: *mut c_void) -> i32;
    // Commit 6 — RegExp wire. Emits `/source/flags` directly via
    // the shared stdout writer (no fresh-Str alloc, no rc_dec
    // dance). Same put_byte family this module uses.
    pub(super) fn __torajs_regex_print_inline(re_ptr: *const c_void);
    // Map / Set walkers — Tag::Set (=19, runtime tag substrate)
    // discriminates Set heap cells from Map (=15), so the AnyValue
    // tag-walker routes each branch to its bun-correct printer
    // (`Map(N) {…}` / `Set(N) {…}` non-empty;`Map {}` / `Set {}`
    // empty). Inline form (no trailing '\n'); the top-level
    // `__torajs_print_anyv` arm appends '\n', the
    // `__torajs_print_anyv_inline` arm leaves it off.
    pub(super) fn __torajs_map_print(m_ptr: *const c_void);
    pub(super) fn __torajs_set_print(s_ptr: *const c_void);
    // Commit 8 — Promise wire. Tag::Promise=8 is unambiguous so
    // the AnyValue walker can route directly here.
    pub(super) fn __torajs_promise_print(p_ptr: *const c_void);
    // Fn-name registry Phase 2 Step 5 — turns a fn body vaddr into
    // `[Function: <name>]` (hit) / `[Function (anonymous)]` (miss)
    // via the rodata `__torajs_fn_name_table[]` from Steps 3b-4.
    // Defined in `crates/torajs-fnname/src/lib.rs`. Caller appends
    // the trailing '\n' (top-level prints) or nested separator.
    pub(super) fn __torajs_fn_print_inline(fn_addr: u64);
    // W-J Phase D — Tag::Obj struct-cell walker. Reads class_tag@+8,
    // looks up `__torajs_class_name_table` (W-J A3c chunk 2 substrate)
    // for the bun `Name { k: v, ... }` prefix, walks declared fields
    // via `__torajs_struct_field_{count,name,info}` (W-J A4 readers),
    // emits each value through `__torajs_print_anyv_inline`. Defined
    // in `crates/torajs-meta/src/struct_print.rs`. Inline form (no
    // trailing '\n'); `__torajs_print_anyv`'s Tag::Obj arm appends
    // '\n' at the top-level boundary.
    pub(super) fn __torajs_anyv_struct_print_inline(v: u64);
}

/// Read the fn body vaddr out of a closure heap cell at
/// `closure + CLOSURE_FN_ADDR_OFF`. Same pattern the
/// accessor-pair drop path uses; both are reading the same word.
#[inline]
pub(super) unsafe fn closure_fn_addr(closure: *const c_void) -> u64 {
    unsafe { *((closure as *const u8).add(CLOSURE_FN_ADDR_OFF) as *const u64) }
}

#[inline]
pub(super) unsafe fn heap_flags(child: *const c_void) -> u16 {
    unsafe { (*(child as *const HeapHeader)).flags }
}

#[inline]
pub(super) unsafe fn heap_type_tag(child: *const c_void) -> u16 {
    unsafe { (*(child as *const HeapHeader)).type_tag }
}

#[inline]
pub(super) fn alloc_literal(s: &[u8]) -> *mut u8 {
    let p = unsafe { __torajs_str_alloc_pooled(s.len() as u64) };
    if !s.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(STR_HDR_SIZE), s.len()) };
    }
    p
}

#[inline]
pub(super) fn write_line(s: &[u8]) {
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
pub(super) unsafe fn put_str_payload_utf8(payload: &[u8], is_latin1: bool) {
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
pub(super) unsafe fn put_str_cell_inline(child: *const c_void) {
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
pub(super) unsafe fn put_substr_cell_inline(child: *const c_void) {
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
