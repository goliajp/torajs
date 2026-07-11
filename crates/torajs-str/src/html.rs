//! Annex B B.2.2 `String.prototype` HTML methods — CreateHTML.
//!
//! `"x".anchor("n")` → `<a name="n">x</a>` and friends. All thirteen
//! methods share [`create_html`]: the receiver string is wrapped in
//! `<tag>` / `</tag>`, and the four attributed forms (anchor /
//! fontcolor / fontsize / link) insert ` attr="V"` where every `"`
//! code unit in V is replaced by `&quot;` (B.2.2.2.1 step 4).
//!
//! Encoding follows the concat widest-of-inputs rule: tag / attr /
//! `&quot;` text are ASCII, so the result is Latin-1 iff both the
//! receiver and the attribute value are. Single allocation, two
//! passes (size + emit) — no intermediate concat Strs.
//!
//! NULL Str operands denote the JS `undefined` value (RC-4 F1b-2
//! convention) and render as the text "undefined" per
//! ToString(undefined) — B.2.2.2.1 step 2/4 run ToString
//! unconditionally.

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use torajs_rc::HeapHeader;

const QUOT: &[u8] = b"&quot;";

#[inline]
unsafe fn str_len(p: *const u8) -> u32 {
    unsafe { (p.add(STR_LEN_OFF) as *const u32).read() }
}

#[inline]
unsafe fn str_is_latin1(p: *const u8) -> bool {
    let header = unsafe { &*(p as *const HeapHeader) };
    (header.flags & STR_FLAG_IS_LATIN1) != 0
}

#[inline]
unsafe fn str_payload<'a>(p: *const u8) -> &'a [u8] {
    let len = unsafe { str_len(p) } as usize;
    let bytes = if unsafe { str_is_latin1(p) } {
        len
    } else {
        len * 2
    };
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), bytes) }
}

/// Count of `"` code units in a Str's payload (escape expansion).
unsafe fn quote_count(p: *const u8) -> u32 {
    let payload = unsafe { str_payload(p) };
    if unsafe { str_is_latin1(p) } {
        payload.iter().filter(|&&b| b == b'"').count() as u32
    } else {
        payload
            .chunks_exact(2)
            .filter(|u| u[0] == b'"' && u[1] == 0)
            .count() as u32
    }
}

/// Sequential payload writer over the output block, Latin-1 or
/// UTF-16 LE per the result encoding. `pos` counts code units.
struct UnitWriter<'a> {
    dst: &'a mut [u8],
    pos: usize,
    latin1: bool,
}

impl UnitWriter<'_> {
    /// Append ASCII bytes (tag / attr / escape text — all < 0x80).
    fn put_ascii(&mut self, s: &[u8]) {
        for &b in s {
            if self.latin1 {
                self.dst[self.pos] = b;
            } else {
                self.dst[self.pos * 2] = b;
                self.dst[self.pos * 2 + 1] = 0;
            }
            self.pos += 1;
        }
    }

    /// Append a Str's payload verbatim, widening Latin-1 → UTF-16
    /// when the output is wide. `escape_quotes` substitutes each `"`
    /// unit with `&quot;` (attribute-value position).
    unsafe fn put_str(&mut self, p: *const u8, escape_quotes: bool) {
        let src_latin1 = unsafe { str_is_latin1(p) };
        let payload = unsafe { str_payload(p) };
        if src_latin1 {
            for &b in payload {
                if escape_quotes && b == b'"' {
                    self.put_ascii(QUOT);
                } else if self.latin1 {
                    self.dst[self.pos] = b;
                    self.pos += 1;
                } else {
                    self.dst[self.pos * 2] = b;
                    self.dst[self.pos * 2 + 1] = 0;
                    self.pos += 1;
                }
            }
        } else {
            // UTF-16 source implies a UTF-16 output (widest rule).
            for u in payload.chunks_exact(2) {
                if escape_quotes && u[0] == b'"' && u[1] == 0 {
                    self.put_ascii(QUOT);
                } else {
                    self.dst[self.pos * 2] = u[0];
                    self.dst[self.pos * 2 + 1] = u[1];
                    self.pos += 1;
                }
            }
        }
    }
}

/// B.2.2.2.1 CreateHTML(string, tag, attribute, value): a fresh
/// refcount=1 Str holding `<tag attribute="escaped(value)">S</tag>`.
/// `attr` empty ⇔ the no-attribute forms. `s` / `val` may be NULL
/// (the JS `undefined` value → the text "undefined").
unsafe fn create_html(s: *const u8, tag: &[u8], attr: &[u8], val: *const u8) -> *mut u8 {
    // NULL operands materialize the "undefined" literal once and
    // recurse with real cells (concat's F1b-2 idiom).
    if s.is_null() || (!attr.is_empty() && val.is_null()) {
        let undef = unsafe { crate::literals::__torajs_undefined_to_str() };
        let s2 = if s.is_null() { undef as *const u8 } else { s };
        let v2 = if !attr.is_empty() && val.is_null() {
            undef as *const u8
        } else {
            val
        };
        let r = unsafe { create_html(s2, tag, attr, v2) };
        unsafe { crate::str_drop::__torajs_str_drop(undef) };
        return r;
    }
    let s_len = unsafe { str_len(s) };
    let s_latin1 = unsafe { str_is_latin1(s) };
    // "<" tag ">" S "</" tag ">"
    let mut units = 1 + tag.len() as u32 + 1 + s_len + 2 + tag.len() as u32 + 1;
    let mut out_latin1 = s_latin1;
    if !attr.is_empty() {
        // " " attr "=\"" V… "\"" with each `"` in V costing +5.
        let v_len = unsafe { str_len(val) };
        let v_quotes = unsafe { quote_count(val) };
        units += 1 + attr.len() as u32 + 2 + v_len + v_quotes * (QUOT.len() as u32 - 1) + 1;
        out_latin1 = out_latin1 && unsafe { str_is_latin1(val) };
    }
    let mut block = StrBlock::alloc_with_encoding(units, out_latin1);
    let byte_cnt = if out_latin1 { units } else { units * 2 };
    let dst = unsafe { block.as_bytes_mut(byte_cnt) };
    let mut w = UnitWriter {
        dst,
        pos: 0,
        latin1: out_latin1,
    };
    w.put_ascii(b"<");
    w.put_ascii(tag);
    if !attr.is_empty() {
        w.put_ascii(b" ");
        w.put_ascii(attr);
        w.put_ascii(b"=\"");
        unsafe { w.put_str(val, true) };
        w.put_ascii(b"\"");
    }
    w.put_ascii(b">");
    unsafe { w.put_str(s, false) };
    w.put_ascii(b"</");
    w.put_ascii(tag);
    w.put_ascii(b">");
    debug_assert_eq!(w.pos as u32, units);
    block.into_raw()
}

macro_rules! html_attr {
    ($name:ident, $tag:literal, $attr:literal) => {
        /// # Safety
        /// `s` / `val` are valid Str heap blocks or NULL (JS
        /// `undefined`). Returns a fresh refcount=1 Str.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(s: *const u8, val: *const u8) -> *mut u8 {
            unsafe { create_html(s, $tag, $attr, val) }
        }
    };
}

macro_rules! html_plain {
    ($name:ident, $tag:literal) => {
        /// # Safety
        /// `s` is a valid Str heap block or NULL (JS `undefined`).
        /// Returns a fresh refcount=1 Str.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(s: *const u8) -> *mut u8 {
            unsafe { create_html(s, $tag, b"", core::ptr::null()) }
        }
    };
}

html_attr!(__torajs_str_anchor, b"a", b"name");
html_attr!(__torajs_str_fontcolor, b"font", b"color");
html_attr!(__torajs_str_fontsize, b"font", b"size");
html_attr!(__torajs_str_link, b"a", b"href");
html_plain!(__torajs_str_big, b"big");
html_plain!(__torajs_str_blink, b"blink");
html_plain!(__torajs_str_bold, b"b");
html_plain!(__torajs_str_fixed, b"tt");
html_plain!(__torajs_str_italics, b"i");
html_plain!(__torajs_str_small, b"small");
html_plain!(__torajs_str_strike, b"strike");
html_plain!(__torajs_str_sub, b"sub");
html_plain!(__torajs_str_sup, b"sup");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::byte_capacity;
    use alloc::vec::Vec;

    fn make_latin1(payload: &[u8]) -> *mut u8 {
        let mut block = StrBlock::alloc_with_encoding(payload.len() as u32, true);
        let dst = unsafe { block.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        block.into_raw()
    }

    fn make_utf16(units: &[u16]) -> *mut u8 {
        let mut block = StrBlock::alloc_with_encoding(units.len() as u32, false);
        let dst = unsafe { block.as_bytes_mut((units.len() * 2) as u32) };
        for (i, &u) in units.iter().enumerate() {
            dst[i * 2] = (u & 0xFF) as u8;
            dst[i * 2 + 1] = (u >> 8) as u8;
        }
        block.into_raw()
    }

    fn read_str(p: *const u8) -> (Vec<u8>, bool, u32) {
        let len = unsafe { str_len(p) };
        let l1 = unsafe { str_is_latin1(p) };
        (unsafe { str_payload(p) }.to_vec(), l1, len)
    }

    fn free(p: *mut u8) {
        let len = unsafe { str_len(p) };
        let l1 = unsafe { str_is_latin1(p) };
        let _ = byte_capacity(len, l1);
        unsafe { crate::block::__torajs_str_free(p) };
    }

    #[test]
    fn bold_plain() {
        let s = make_latin1(b"x");
        let r = unsafe { __torajs_str_bold(s) };
        let (payload, l1, len) = read_str(r);
        assert!(l1);
        assert_eq!(len, 8);
        assert_eq!(payload, b"<b>x</b>");
        free(s);
        free(r);
    }

    #[test]
    fn anchor_with_name() {
        let s = make_latin1(b"x");
        let v = make_latin1(b"n");
        let r = unsafe { __torajs_str_anchor(s, v) };
        let (payload, l1, _) = read_str(r);
        assert!(l1);
        assert_eq!(payload, br#"<a name="n">x</a>"#);
        free(s);
        free(v);
        free(r);
    }

    #[test]
    fn attribute_quote_escapes() {
        let s = make_latin1(b"x");
        let v = make_latin1(br#"a"b"#);
        let r = unsafe { __torajs_str_fontcolor(s, v) };
        let (payload, _, _) = read_str(r);
        assert_eq!(payload, br#"<font color="a&quot;b">x</font>"#);
        free(s);
        free(v);
        free(r);
    }

    #[test]
    fn utf16_receiver_widens_result() {
        // "汉" U+6C49
        let s = make_utf16(&[0x6C49]);
        let r = unsafe { __torajs_str_big(s) };
        let (payload, l1, len) = read_str(r);
        assert!(!l1);
        assert_eq!(len, 12); // <big>汉</big>
        // spot the tag start '<' as UTF-16 LE
        assert_eq!(&payload[0..2], &[b'<', 0]);
        // the wide unit sits at index 5
        assert_eq!(&payload[10..12], &[0x49, 0x6C]);
        free(s);
        free(r);
    }

    #[test]
    fn null_operands_render_undefined() {
        let r = unsafe { __torajs_str_link(core::ptr::null(), core::ptr::null()) };
        let (payload, _, _) = read_str(r);
        assert_eq!(payload, br#"<a href="undefined">undefined</a>"#);
        free(r);
    }
}
