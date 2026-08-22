//! `JsonBuilder` — amortized-grow UTF-8 byte accumulator backing the
//! SSA-inlined `JSON.stringify(struct)` fast path.
//!
//! ## Motivation
//!
//! Pre-P14-S5 `lower_json_stringify` for `Type::Obj` emits
//! `__torajs_str_concat(a, b)` per field key/colon/value/comma —
//! a 4-field flat struct produces ~16 concat calls. Each
//! `str_concat` is a fresh `StrBlock::alloc` + memcpy of the
//! accumulator + memcpy of the addendum, so the accumulator's
//! bytes are copied **at every concat** → ~O(N²) byte copies per
//! `JSON.stringify` call. On `json-stringify-100k` (45 byte output)
//! that's ~400 byte copies per iter vs ~45 byte copies in a single-
//! buffer builder.
//!
//! `JsonBuilder` is a thin `Vec<u8>` wrapper that the SSA emit path
//! calls into directly via per-type push helpers (`push_byte` /
//! `push_str_raw` / `push_str_quoted` / `push_i64` / `push_bool`).
//! All bytes accumulate in one growing `Vec<u8>` (jemalloc/Vec
//! exponential grow → ~3 mallocs for a 45-byte output, vs 16),
//! then `finalize` returns a fresh `Str` block via the standard
//! `StrBlock::alloc_with_encoding` path (Latin-1 if every byte
//! `< 0x80`, UTF-16 otherwise).
//!
//! Output is byte-for-byte equivalent to the concat-chain emission;
//! tests in this module pin the equivalence on representative
//! struct shapes ({i64,Str,i64,Bool}, all-i64, all-Str, mixed
//! escapes, etc).

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use crate::print::{iter_utf16_codepoints, write_utf8_for_codepoint};
use alloc::boxed::Box;
use alloc::vec::Vec;
use torajs_rc::HeapHeader;

const HEX: &[u8; 16] = b"0123456789abcdef";
const INITIAL_CAP: usize = 64;

/// The one builder kept alive between `JSON.stringify` calls.
///
/// Building the record was never the expensive part: `jsb_new` +
/// `jsb_finalize` were ~50% of `json-stringify-100k`'s samples, all
/// of it the system allocator taking and returning the `Box` and its
/// `Vec` once per call. Parking one keeps both, and keeps the
/// buffer's capacity too, so the steady state is zero allocations
/// for the accumulator — the reason serializers hold a scratch
/// buffer rather than minting one per document.
///
/// One slot, because the lane that mints builders handles
/// primitive-only layouts and so never nests. A caller that somehow
/// nests still works: the inner `new` finds the slot empty and
/// allocates.
///
/// `AtomicPtr` under `Relaxed` for the same reason `pool.rs` uses it
/// — the runtime is single-threaded, this codegens identically to
/// `static mut`, and it does not trip the 2024 `static_mut_refs`
/// lint.
static PARKED: AtomicPtr<JsonBuilder> = AtomicPtr::new(ptr::null_mut());

/// Single-threaded byte accumulator owned by SSA-emitted code via
/// `*mut JsonBuilder` opaque handles. The handle is created by
/// `__torajs_jsb_new`, mutated via push helpers, consumed by
/// `__torajs_jsb_finalize` (which `Box::from_raw`s + frees the
/// builder). Caller must not double-finalize or use the handle
/// after finalize.
pub struct JsonBuilder {
    /// Accumulated output, UTF-8 byte semantics. ASCII-only inputs
    /// (the overwhelmingly common case) keep it byte == codepoint;
    /// a UTF-16 Str argument decodes into multi-byte UTF-8 here and
    /// flips `saw_non_ascii` so `finalize` re-classifies.
    buf: Vec<u8>,
    /// True once any pushed content decoded to a codepoint > 0x7F —
    /// `finalize` then routes through the canonical-encoding
    /// classifier instead of the ASCII/Latin-1 verbatim copy.
    saw_non_ascii: bool,
    /// Runtime comma protocol (chunk 658 — §25.5.2.4 step 8.b key
    /// skip): true once a field has been emitted, so the next
    /// `begin_field` writes the `,` separator. An undefined Str
    /// field skips its key without touching this flag.
    pending_sep: bool,
}

/// Read the `IS_LATIN1` bit out of a Str heap header (same shape as
/// the eq / concat / json siblings).
///
/// # Safety
///
/// `p` must point at a valid Str block.
#[inline]
unsafe fn str_is_latin1(p: *const u8) -> bool {
    let header = unsafe { &*(p as *const HeapHeader) };
    (header.flags & STR_FLAG_IS_LATIN1) != 0
}

/// True when a Latin-1 payload cannot go into the buffer as-is:
/// something in it needs a JSON escape, or it is a code point above
/// ASCII, which is one Latin-1 byte but two UTF-8 ones. Both answers
/// come out of the same single pass the copy would have paid anyway.
#[inline]
fn needs_escape(s: &[u8]) -> bool {
    s.iter()
        .any(|&c| c < 0x20 || c == b'"' || c == b'\\' || c >= 0x80)
}

/// Write `s` — a **Latin-1** payload, one byte per code point —
/// quoted and JSON-escaped into `dst` as UTF-8. Answers whether it
/// put a multi-byte sequence there, which is what tells `finalize`
/// the buffer has to be decoded rather than copied.
#[inline]
fn write_escaped_into(dst: &mut Vec<u8>, s: &[u8]) -> bool {
    let mut wide = false;
    dst.push(b'"');
    for &c in s {
        match c {
            b'"' => dst.extend_from_slice(b"\\\""),
            b'\\' => dst.extend_from_slice(b"\\\\"),
            b'\n' => dst.extend_from_slice(b"\\n"),
            b'\r' => dst.extend_from_slice(b"\\r"),
            b'\t' => dst.extend_from_slice(b"\\t"),
            0x08 => dst.extend_from_slice(b"\\b"),
            0x0c => dst.extend_from_slice(b"\\f"),
            c if c < 0x20 => {
                dst.push(b'\\');
                dst.push(b'u');
                dst.push(b'0');
                dst.push(b'0');
                dst.push(HEX[(c >> 4) as usize]);
                dst.push(HEX[(c & 0xf) as usize]);
            }
            c if c >= 0x80 => {
                // Latin-1 supplement. The byte IS the code point,
                // and its UTF-8 form is two bytes — pushing it raw
                // left a lead byte in the buffer that swallowed
                // whatever the builder wrote next.
                write_utf8_for_codepoint(c as u32, |b| dst.push(b));
                wide = true;
            }
            _ => dst.push(c),
        }
    }
    dst.push(b'"');
    wide
}

/// Rotation 466: this used to run `write!(.., "{}", n)`, and a
/// leaf-symbol profile of `JSON.stringify` in a loop put
/// `Display for i64` + `core::fmt::write` + `pad_integral` at 16%
/// of self-time — more than the string escaping the builder exists
/// for. None of that machinery does anything here: there is no
/// width, no fill, no sign option to honour, just digits appended
/// to a byte buffer.
#[inline]
fn write_i64_into(dst: &mut Vec<u8>, n: i64) {
    use torajs_fmt::itoa::{ITOA_BUF_LEN, itoa_into};
    let mut buf = [0u8; ITOA_BUF_LEN];
    let start = itoa_into(n, &mut buf);
    dst.extend_from_slice(&buf[start..]);
}

/// Allocate a fresh builder with an initial buffer capacity. The
/// capacity is a hint — the buffer grows amortized on push. Pass
/// the SSA emitter's compile-time size estimate (sum of field key
/// lengths + separator bytes + typical value width).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_new(initial_cap: u32) -> *mut JsonBuilder {
    let cap = (initial_cap as usize).max(INITIAL_CAP);
    let parked = PARKED.swap(ptr::null_mut(), Ordering::Relaxed);
    if !parked.is_null() {
        // SAFETY: only `finalize` parks, and it parks a builder it
        // has just stopped using; the swap above takes it back out,
        // so this is the only live handle.
        let b = unsafe { &mut *parked };
        if b.buf.capacity() < cap {
            b.buf.reserve(cap);
        }
        return parked;
    }
    Box::into_raw(Box::new(JsonBuilder {
        buf: Vec::with_capacity(cap),
        saw_non_ascii: false,
        pending_sep: false,
    }))
}

/// Append a single ASCII byte (e.g. `{`, `}`, `[`, `]`, `:`, `,`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_push_byte(sb: *mut JsonBuilder, b: u8) {
    unsafe { (*sb).buf.push(b) };
}

/// Append the raw bytes from a Str heap block payload — no quoting,
/// no escaping. Used for field-name emission where the SSA emitter
/// has pre-quoted the key literal at intern time (`"id"`,
/// `"\"name\""`, etc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_push_str_raw(sb: *mut JsonBuilder, str_ptr: *const u8) {
    let len = unsafe { (str_ptr.add(STR_LEN_OFF) as *const u32).read() };
    let sb = unsafe { &mut *sb };
    if !unsafe { str_is_latin1(str_ptr) } {
        // UTF-16 key literal (chunk 657): decode to UTF-8 — `len`
        // is code units, the payload is LE pairs.
        let payload =
            unsafe { core::slice::from_raw_parts(str_ptr.add(STR_DATA_OFF), (len as usize) * 2) };
        iter_utf16_codepoints(payload, |cp| {
            write_utf8_for_codepoint(cp, |b| sb.buf.push(b));
        });
        sb.saw_non_ascii = true;
        return;
    }
    let bytes = unsafe { core::slice::from_raw_parts(str_ptr.add(STR_DATA_OFF), len as usize) };
    if bytes.iter().any(|&c| c >= 0x80) {
        // A Latin-1 key above ASCII: one byte here, two in the
        // buffer's encoding. No escaping — the emitter interned
        // these bytes already quoted.
        for &c in bytes {
            write_utf8_for_codepoint(c as u32, |b| sb.buf.push(b));
        }
        sb.saw_non_ascii = true;
        return;
    }
    sb.buf.extend_from_slice(bytes);
}

/// Append a Str value as a JSON string (surrounding quotes + JSON
/// escape per ES §24.5.2). Matches `__torajs_json_quote_str` output
/// bit-for-bit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_push_str_quoted(sb: *mut JsonBuilder, str_ptr: *const u8) {
    // Nullish Str slot — the undefined sentinel (missed exec/match
    // capture) or NULL (JS null / not-yet-flipped producers). Both
    // stringify to `null` inside an array/object per ES §25.5.2.
    if str_ptr.is_null() || crate::undef_sentinel::is_undef(str_ptr) {
        unsafe { (*sb).buf.extend_from_slice(b"null") };
        return;
    }
    let len = unsafe { (str_ptr.add(STR_LEN_OFF) as *const u32).read() };
    let sb = unsafe { &mut *sb };
    if !unsafe { str_is_latin1(str_ptr) } {
        // UTF-16 payload (chunk 657): the byte walk below would read
        // `len` bytes of LE pairs as garbage. Decode per codepoint,
        // escape the ASCII set, UTF-8-encode the rest.
        let payload =
            unsafe { core::slice::from_raw_parts(str_ptr.add(STR_DATA_OFF), (len as usize) * 2) };
        sb.buf.push(b'"');
        iter_utf16_codepoints(payload, |cp| match cp {
            0x22 => sb.buf.extend_from_slice(b"\\\""),
            0x5C => sb.buf.extend_from_slice(b"\\\\"),
            0x0A => sb.buf.extend_from_slice(b"\\n"),
            0x0D => sb.buf.extend_from_slice(b"\\r"),
            0x09 => sb.buf.extend_from_slice(b"\\t"),
            0x08 => sb.buf.extend_from_slice(b"\\b"),
            0x0C => sb.buf.extend_from_slice(b"\\f"),
            c if c < 0x20 => {
                sb.buf.extend_from_slice(b"\\u00");
                sb.buf.push(HEX[(c >> 4) as usize]);
                sb.buf.push(HEX[(c & 0xf) as usize]);
            }
            // §25.5.2.2 step 2.d — `iter_utf16_codepoints` joins a
            // well-formed pair into one code point, so anything
            // still in the surrogate range here is lone. It has no
            // UTF-8 spelling; the escape is how a well-formed JSON
            // text carries it.
            c if (0xD800..=0xDFFF).contains(&c) => {
                sb.buf.extend_from_slice(b"\\u");
                sb.buf.push(HEX[(c >> 12) as usize]);
                sb.buf.push(HEX[((c >> 8) & 0xf) as usize]);
                sb.buf.push(HEX[((c >> 4) & 0xf) as usize]);
                sb.buf.push(HEX[(c & 0xf) as usize]);
            }
            c => write_utf8_for_codepoint(c, |b| sb.buf.push(b)),
        });
        sb.buf.push(b'"');
        sb.saw_non_ascii = true;
        return;
    }
    let bytes = unsafe { core::slice::from_raw_parts(str_ptr.add(STR_DATA_OFF), len as usize) };
    if !needs_escape(bytes) {
        sb.buf.push(b'"');
        sb.buf.extend_from_slice(bytes);
        sb.buf.push(b'"');
    } else {
        sb.saw_non_ascii |= write_escaped_into(&mut sb.buf, bytes);
    }
}

/// Append an i64 as its decimal ASCII representation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_push_i64(sb: *mut JsonBuilder, n: i64) {
    let sb = unsafe { &mut *sb };
    write_i64_into(&mut sb.buf, n);
}

/// Append `"true"` / `"false"` per JS `String(b)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_push_bool(sb: *mut JsonBuilder, b: i32) {
    let sb = unsafe { &mut *sb };
    if b != 0 {
        sb.buf.extend_from_slice(b"true");
    } else {
        sb.buf.extend_from_slice(b"false");
    }
}

/// Append `"null"` (used for `null` / `undefined` JSON-stringify
/// arms — both collapse to `null` per ES §25.5.2).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_push_null(sb: *mut JsonBuilder) {
    let sb = unsafe { &mut *sb };
    sb.buf.extend_from_slice(b"null");
}

/// Finalize: consume the builder, allocate a fresh `Str` heap
/// block, copy the accumulated bytes in. Returns a `*mut u8`
/// pointing at the new `Str` (refcount=1). The builder pointer is
/// invalid after this call. The buffer holds UTF-8; the result is
/// Latin-1-encoded when every code point fits in a byte and UTF-16
/// otherwise — the classification `__torajs_str_alloc` does. JSON
/// structure is ASCII, so the common case skips that scan.
/// Open the next object field: write the `,` separator when a
/// field was already emitted (runtime comma protocol — a skipped
/// undefined field must not leave a stray comma, §25.5.2.4).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_begin_field(sb: *mut JsonBuilder) {
    let sb = unsafe { &mut *sb };
    if sb.pending_sep {
        sb.buf.push(b',');
    }
    sb.pending_sep = true;
}

/// Emit one Str-typed object field (`<sep?>"key":<quoted val>`), or
/// NOTHING when `val` is the undefined sentinel — §25.5.2.4 step
/// 8.b: SerializeJSONProperty returning undefined skips the key.
/// NULL is JS `null` and keeps the key (quoted lane prints `null`).
///
/// # Safety
///
/// `sb` is a live builder; `key_raw` is a live Str block holding the
/// pre-quoted `"key":` bytes; `val` is NULL, the sentinel, or a live
/// Str block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_push_field_str(
    sb: *mut JsonBuilder,
    key_raw: *const u8,
    val: *const u8,
) {
    if crate::undef_sentinel::is_undef(val) {
        return;
    }
    unsafe {
        __torajs_jsb_begin_field(sb);
        __torajs_jsb_push_str_raw(sb, key_raw);
        __torajs_jsb_push_str_quoted(sb, val);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_jsb_finalize(sb: *mut JsonBuilder) -> *mut u8 {
    // SAFETY: the caller's contract is one live handle from
    // `__torajs_jsb_new`, consumed here.
    let out = unsafe { finalize_bytes(&*sb) };
    park(sb);
    out
}

/// Hand the builder back to the parking slot, cleared and ready. The
/// buffer keeps its capacity, which is the whole point: the next
/// `JSON.stringify` writes into bytes this call already had.
///
/// A builder arrives here only from `finalize`, so a full slot means
/// two are somehow live at once; that one goes back to the allocator
/// rather than displacing the parked one.
fn park(sb: *mut JsonBuilder) {
    // SAFETY: the caller has just finished with `sb`.
    let b = unsafe { &mut *sb };
    b.buf.clear();
    b.saw_non_ascii = false;
    b.pending_sep = false;
    if PARKED.load(Ordering::Relaxed).is_null() {
        PARKED.store(sb, Ordering::Relaxed);
        return;
    }
    // SAFETY: a builder reaches `park` exactly once, from the
    // `finalize` that consumed it.
    drop(unsafe { Box::from_raw(sb) });
}

/// The Str a finished builder answers, read out of its buffer rather
/// than moving the buffer away — the allocation stays with the
/// builder so the next call can reuse it.
unsafe fn finalize_bytes(sb: &JsonBuilder) -> *mut u8 {
    let bytes = &sb.buf;
    if sb.saw_non_ascii {
        // Chunk 657 — a UTF-16 Str argument decoded into multi-byte
        // UTF-8 above; classify to the canonical encoding (Latin-1
        // vs UTF-16) like every other UTF-8-input alloc site. The
        // ASCII-only common case below keeps the scan-free path.
        return unsafe { crate::block::__torajs_str_alloc(bytes.as_ptr(), bytes.len() as i64) };
    }
    // V0.2 P14-S7 — nothing wrote a multi-byte sequence, so every
    // byte here is ASCII and is already its own Latin-1 code unit:
    // copy it in and skip the classification scan, which is worth
    // ~5-10 ns/iter on `JSON.stringify`-heavy loops. JSON structure
    // is ASCII (braces, commas, colons, quotes, `\u00XX` escapes,
    // the i64 / bool / null literals), so this is the ordinary
    // case. Only string CONTENT above ASCII flips `saw_non_ascii`,
    // and every producer that can emit such content sets it — a
    // Latin-1 payload was once copied in raw, which left a lead
    // byte in the buffer that ate whatever came after it.
    let mut block = StrBlock::alloc_with_encoding(bytes.len() as u32, true);
    let dst = unsafe { block.as_bytes_mut(bytes.len() as u32) };
    dst.copy_from_slice(bytes);
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::__torajs_str_free;
    use alloc::string::String;

    fn make_str(bytes: &[u8]) -> *mut u8 {
        let mut block = StrBlock::alloc_with_encoding(bytes.len() as u32, true);
        let dst = unsafe { block.as_bytes_mut(bytes.len() as u32) };
        dst.copy_from_slice(bytes);
        block.into_raw()
    }

    /// A UTF-16 LE Str: `length` counts u16 code units, payload is
    /// twice that many bytes, `IS_LATIN1` clear.
    fn make_utf16_str(units: &[u16]) -> *mut u8 {
        let length = units.len() as u32;
        let mut block = StrBlock::alloc_with_encoding(length, false);
        let dst = unsafe { block.as_bytes_mut(length * 2) };
        for (i, &u) in units.iter().enumerate() {
            dst[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        block.into_raw()
    }

    /// Drop whatever is parked so a test's first `jsb_new` takes the
    /// allocating path, and a later test cannot inherit a buffer this
    /// one grew.
    fn clear_parked() {
        let p = PARKED.swap(ptr::null_mut(), Ordering::Relaxed);
        if !p.is_null() {
            drop(unsafe { Box::from_raw(p) });
        }
    }

    #[test]
    fn a_finalized_builder_is_reused_with_its_buffer() {
        unsafe {
            clear_parked();
            let first = __torajs_jsb_new(64);
            __torajs_jsb_push_byte(first, b'{');
            __torajs_jsb_push_byte(first, b'}');
            let out = __torajs_jsb_finalize(first);
            assert_eq!(read_finalized(out), "{}");
            __torajs_str_free(out);
            // The next call gets the same builder back, cleared —
            // no leftover bytes, no leftover comma protocol.
            let second = __torajs_jsb_new(64);
            assert_eq!(second, first, "parked builder reused");
            __torajs_jsb_push_byte(second, b'{');
            __torajs_jsb_begin_field(second);
            __torajs_jsb_push_byte(second, b'1');
            __torajs_jsb_push_byte(second, b'}');
            let out2 = __torajs_jsb_finalize(second);
            assert_eq!(read_finalized(out2), "{1}", "no stale separator");
            __torajs_str_free(out2);
            clear_parked();
        }
    }

    #[test]
    fn a_second_live_builder_does_not_displace_the_parked_one() {
        unsafe {
            clear_parked();
            let outer = __torajs_jsb_new(64);
            let inner = __torajs_jsb_new(64);
            assert_ne!(outer, inner, "both live at once");
            __torajs_jsb_push_byte(inner, b'1');
            let a = __torajs_jsb_finalize(inner);
            __torajs_jsb_push_byte(outer, b'2');
            let b = __torajs_jsb_finalize(outer);
            assert_eq!(read_finalized(a), "1");
            assert_eq!(read_finalized(b), "2");
            __torajs_str_free(a);
            __torajs_str_free(b);
            clear_parked();
        }
    }

    fn read_finalized(p: *mut u8) -> String {
        unsafe {
            let len = (p.add(STR_LEN_OFF) as *const u32).read();
            let bytes = core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize);
            String::from_utf8(bytes.to_vec()).unwrap()
        }
    }

    /// Decode a finalized Str whatever encoding it landed in — the
    /// ASCII-only reader above cannot see a Latin-1 supplement byte
    /// or a UTF-16 payload.
    fn read_finalized_text(p: *mut u8) -> String {
        unsafe {
            let latin1 = str_is_latin1(p);
            let len = (p.add(STR_LEN_OFF) as *const u32).read() as usize;
            let width = if latin1 { len } else { len * 2 };
            let bytes = core::slice::from_raw_parts(p.add(STR_DATA_OFF), width);
            if latin1 {
                bytes.iter().map(|&b| b as char).collect()
            } else {
                let units: alloc::vec::Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                alloc::string::String::from_utf16(&units).unwrap()
            }
        }
    }

    #[test]
    fn primitive_struct_round_trip() {
        // Emulates lower_json_stringify Type::Obj emit for
        // `{id: 42, name: "row", score: 294, active: true}`.
        unsafe {
            let sb = __torajs_jsb_new(64);
            __torajs_jsb_push_byte(sb, b'{');
            // Field 0: "id":42
            __torajs_jsb_push_byte(sb, b'"');
            let k1 = make_str(b"id");
            __torajs_jsb_push_str_raw(sb, k1);
            __torajs_jsb_push_byte(sb, b'"');
            __torajs_jsb_push_byte(sb, b':');
            __torajs_jsb_push_i64(sb, 42);
            // Field 1: ,"name":"row"
            __torajs_jsb_push_byte(sb, b',');
            __torajs_jsb_push_byte(sb, b'"');
            let k2 = make_str(b"name");
            __torajs_jsb_push_str_raw(sb, k2);
            __torajs_jsb_push_byte(sb, b'"');
            __torajs_jsb_push_byte(sb, b':');
            let v2 = make_str(b"row");
            __torajs_jsb_push_str_quoted(sb, v2);
            // Field 2: ,"score":294
            __torajs_jsb_push_byte(sb, b',');
            __torajs_jsb_push_byte(sb, b'"');
            let k3 = make_str(b"score");
            __torajs_jsb_push_str_raw(sb, k3);
            __torajs_jsb_push_byte(sb, b'"');
            __torajs_jsb_push_byte(sb, b':');
            __torajs_jsb_push_i64(sb, 294);
            // Field 3: ,"active":true
            __torajs_jsb_push_byte(sb, b',');
            __torajs_jsb_push_byte(sb, b'"');
            let k4 = make_str(b"active");
            __torajs_jsb_push_str_raw(sb, k4);
            __torajs_jsb_push_byte(sb, b'"');
            __torajs_jsb_push_byte(sb, b':');
            __torajs_jsb_push_bool(sb, 1);
            __torajs_jsb_push_byte(sb, b'}');
            let out = __torajs_jsb_finalize(sb);
            let s = read_finalized(out);
            assert_eq!(s, r#"{"id":42,"name":"row","score":294,"active":true}"#);
            __torajs_str_free(out);
            __torajs_str_free(k1);
            __torajs_str_free(k2);
            __torajs_str_free(k3);
            __torajs_str_free(k4);
            __torajs_str_free(v2);
        }
    }

    #[test]
    fn quoted_str_escapes() {
        unsafe {
            let sb = __torajs_jsb_new(64);
            let v = make_str(b"a\"b\\c\nd");
            __torajs_jsb_push_str_quoted(sb, v);
            let out = __torajs_jsb_finalize(sb);
            let s = read_finalized(out);
            assert_eq!(s, r#""a\"b\\c\nd""#);
            __torajs_str_free(out);
            __torajs_str_free(v);
        }
    }

    #[test]
    fn quoted_str_control_unicode_escape() {
        unsafe {
            let sb = __torajs_jsb_new(64);
            let v = make_str(b"x\x01y");
            __torajs_jsb_push_str_quoted(sb, v);
            let out = __torajs_jsb_finalize(sb);
            let s = read_finalized(out);
            // Control byte 0x01 must emit `\u0001` per JSON spec; matches `__torajs_json_quote_str`.
            assert_eq!(s.as_bytes(), b"\"x\\u0001y\"");
            __torajs_str_free(out);
            __torajs_str_free(v);
        }
    }

    #[test]
    fn i64_negative_and_boundary() {
        unsafe {
            let sb = __torajs_jsb_new(64);
            __torajs_jsb_push_i64(sb, -123);
            __torajs_jsb_push_byte(sb, b',');
            __torajs_jsb_push_i64(sb, i64::MIN);
            __torajs_jsb_push_byte(sb, b',');
            __torajs_jsb_push_i64(sb, i64::MAX);
            let out = __torajs_jsb_finalize(sb);
            let s = read_finalized(out);
            assert_eq!(s, "-123,-9223372036854775808,9223372036854775807");
            __torajs_str_free(out);
        }
    }

    #[test]
    fn bool_and_null_arms() {
        unsafe {
            let sb = __torajs_jsb_new(64);
            __torajs_jsb_push_bool(sb, 1);
            __torajs_jsb_push_byte(sb, b',');
            __torajs_jsb_push_bool(sb, 0);
            __torajs_jsb_push_byte(sb, b',');
            __torajs_jsb_push_null(sb);
            let out = __torajs_jsb_finalize(sb);
            let s = read_finalized(out);
            assert_eq!(s, "true,false,null");
            __torajs_str_free(out);
        }
    }

    #[test]
    fn empty_struct_just_braces() {
        unsafe {
            let sb = __torajs_jsb_new(8);
            __torajs_jsb_push_byte(sb, b'{');
            __torajs_jsb_push_byte(sb, b'}');
            let out = __torajs_jsb_finalize(sb);
            let s = read_finalized(out);
            assert_eq!(s, "{}");
            __torajs_str_free(out);
        }
    }

    #[test]
    fn latin1_supplement_content_survives_a_utf16_neighbour() {
        // The shape that corrupted: one Latin-1 element (payload
        // byte 0xC9) beside one UTF-16 element. The UTF-16 side
        // sets `saw_non_ascii`, so `finalize` decodes the buffer as
        // UTF-8 — and the raw 0xC9 left there was a lead byte that
        // ate the closing quote after it.
        unsafe {
            clear_parked();
            let sb = __torajs_jsb_new(64);
            let a = make_str(&[0xC9]);
            let b = make_utf16_str(&[0x4E2D]);
            __torajs_jsb_push_byte(sb, b'[');
            __torajs_jsb_push_str_quoted(sb, a);
            __torajs_jsb_push_byte(sb, b',');
            __torajs_jsb_push_str_quoted(sb, b);
            __torajs_jsb_push_byte(sb, b']');
            let out = __torajs_jsb_finalize(sb);
            assert_eq!(read_finalized_text(out), "[\"\u{c9}\",\"\u{4e2d}\"]");
            __torajs_str_free(a);
            __torajs_str_free(b);
            __torajs_str_free(out);
            clear_parked();
        }
    }

    #[test]
    fn latin1_supplement_content_alone_round_trips() {
        // No UTF-16 neighbour, so `finalize` takes the verbatim
        // copy — which is only right because the value stays one
        // code point either way.
        unsafe {
            clear_parked();
            let sb = __torajs_jsb_new(64);
            let a = make_str(&[0xE9, b'x']);
            __torajs_jsb_push_str_quoted(sb, a);
            let out = __torajs_jsb_finalize(sb);
            assert_eq!(read_finalized_text(out), "\"\u{e9}x\"");
            __torajs_str_free(a);
            __torajs_str_free(out);
            clear_parked();
        }
    }

    #[test]
    fn latin1_supplement_key_survives_a_utf16_neighbour() {
        // `push_str_raw` carries pre-quoted key bytes and does no
        // escaping, so it needs the same widening on its own.
        unsafe {
            clear_parked();
            let sb = __torajs_jsb_new(64);
            let k = make_str(b"\"\xe9\":");
            let v = make_utf16_str(&[0x4E2D]);
            __torajs_jsb_push_byte(sb, b'{');
            __torajs_jsb_push_str_raw(sb, k);
            __torajs_jsb_push_str_quoted(sb, v);
            __torajs_jsb_push_byte(sb, b'}');
            let out = __torajs_jsb_finalize(sb);
            assert_eq!(read_finalized_text(out), "{\"\u{e9}\":\"\u{4e2d}\"}");
            __torajs_str_free(k);
            __torajs_str_free(v);
            __torajs_str_free(out);
            clear_parked();
        }
    }
}
