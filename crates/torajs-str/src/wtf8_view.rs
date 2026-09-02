//! A Str's content as WTF-8 bytes — the spelling the compiler bakes
//! struct field names in (`.__class_field_name_*` rodata) and the
//! one a property key has to take to be compared against them.
//!
//! One kernel, called across the runtime tier through the C ABI
//! ([`__torajs_str_wtf8_into`]): the caller owns the buffer, so no
//! allocation crosses a crate boundary. A Latin-1 payload that is
//! pure ASCII IS its WTF-8 spelling already; callers keep that case
//! zero-copy and only come here for the rest.

use crate::eq::resolve_payload;
use crate::print::{iter_utf16_codepoints, write_utf8_for_codepoint};

/// Write `s`'s content as WTF-8 into `buf[..cap]`, answering the
/// full byte length. When the answer exceeds `cap` nothing past
/// `cap` is written and the caller retries with a larger buffer.
/// Every code unit is preserved: a lone surrogate becomes its
/// three-byte WTF-8 form, a surrogate pair the four-byte scalar —
/// the same bytes `Wtf8Buf` holds for that key at compile time.
///
/// # Safety
/// `s` must point at a valid owned-Str or Substr heap block; `buf`
/// must be NULL or point at `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_wtf8_into(s: *const u8, buf: *mut u8, cap: u32) -> u32 {
    let (payload, latin1) = unsafe { resolve_payload(s) };
    let cap = cap as usize;
    let mut n = 0usize;
    let mut put = |b: u8| {
        if n < cap && !buf.is_null() {
            // SAFETY: `n < cap` and the caller's buffer is `cap` bytes.
            unsafe { buf.add(n).write(b) };
        }
        n += 1;
    };
    if latin1 {
        for &b in payload {
            write_utf8_for_codepoint(b as u32, &mut put);
        }
    } else {
        iter_utf16_codepoints(payload, |cp| write_utf8_for_codepoint(cp, &mut put));
    }
    n as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::StrBlock;

    fn utf16(units: &[u16]) -> StrBlock {
        let mut block = StrBlock::alloc_with_encoding(units.len() as u32, false);
        let bytes: alloc::vec::Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u32)
                .copy_from_slice(&bytes)
        };
        block
    }

    fn latin1(bytes: &[u8]) -> StrBlock {
        let mut block = StrBlock::alloc_with_encoding(bytes.len() as u32, true);
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u32)
                .copy_from_slice(bytes)
        };
        block
    }

    fn wtf8_of(s: &StrBlock) -> alloc::vec::Vec<u8> {
        let n = unsafe { __torajs_str_wtf8_into(s.0.as_ptr(), core::ptr::null_mut(), 0) };
        let mut v = alloc::vec![0u8; n as usize];
        let m = unsafe { __torajs_str_wtf8_into(s.0.as_ptr(), v.as_mut_ptr(), n) };
        assert_eq!(n, m);
        v
    }

    #[test]
    fn ascii_and_latin1_supplement() {
        assert_eq!(wtf8_of(&latin1(b"name")), b"name");
        assert_eq!(wtf8_of(&latin1(&[0xE9])), "\u{e9}".as_bytes());
    }

    #[test]
    fn bmp_pair_and_lone_surrogate() {
        assert_eq!(wtf8_of(&utf16(&[0x4E2D])), "\u{4e2d}".as_bytes());
        assert_eq!(wtf8_of(&utf16(&[0xD83D, 0xDE00])), "\u{1f600}".as_bytes());
        assert_eq!(wtf8_of(&utf16(&[0xD800])), &[0xED, 0xA0, 0x80]);
        assert_eq!(wtf8_of(&utf16(&[0xDC00, 0x41])), &[0xED, 0xB0, 0x80, 0x41]);
    }

    #[test]
    fn short_buffer_answers_the_full_length() {
        let s = utf16(&[0x4E2D, 0x6587]);
        let mut small = [0u8; 2];
        let n = unsafe { __torajs_str_wtf8_into(s.0.as_ptr(), small.as_mut_ptr(), 2) };
        assert_eq!(n, 6);
        assert_eq!(&small, &"\u{4e2d}".as_bytes()[..2]);
    }
}
