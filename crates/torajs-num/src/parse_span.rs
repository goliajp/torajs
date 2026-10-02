//! `parseInt(m[k], radix)` over an exec-record span — the group read
//! straight out of the subject instead of through a fresh Str
//! (torajs-core `ssa_lower_exec_record_parse_int`). Same kernel as
//! `__torajs_num_parse_int`, fed the same bytes `__torajs_str_group_span`
//! would have copied: a negative `start` is the group that did not
//! participate, whose value is the string `"undefined"`.

use crate::parse::{narrow_utf16_prefix, parse_int, text};

/// `parseInt(subject[start..end), radix)` in code units, or
/// `parseInt("undefined", radix)` when `start` is negative.
///
/// # Safety
///
/// `subject` is a live Str or Substr heap pointer and `0 <= start <=
/// end <= subject.length` whenever `start` is not negative.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_parse_int_span(
    subject: *const u8,
    start: i64,
    end: i64,
    radix: i64,
) -> f64 {
    if start < 0 {
        return parse_int(b"undefined", radix);
    }
    let (payload, is_latin1) = unsafe { text(subject) };
    let (lo, hi) = (start as usize, end as usize);
    if is_latin1 {
        parse_int(&payload[lo..hi], radix)
    } else {
        parse_int(&narrow_utf16_prefix(&payload[2 * lo..2 * hi]), radix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_FLAGS_OFF, STR_LEN_OFF};

    fn str_block(payload: &[u8], latin1: bool) -> Vec<u8> {
        let mut s = vec![0u8; STR_DATA_OFF + payload.len()];
        let flags: u16 = if latin1 { STR_FLAG_IS_LATIN1 } else { 0 };
        s[STR_FLAGS_OFF..STR_FLAGS_OFF + 2].copy_from_slice(&flags.to_le_bytes());
        let units = if latin1 {
            payload.len()
        } else {
            payload.len() / 2
        };
        s[STR_LEN_OFF..STR_LEN_OFF + 4].copy_from_slice(&(units as u32).to_le_bytes());
        s[STR_DATA_OFF..].copy_from_slice(payload);
        s
    }

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn span_reads_the_group_not_the_subject() {
        let s = str_block(b"ab=42;cd=7", true);
        assert_eq!(
            unsafe { __torajs_num_parse_int_span(s.as_ptr(), 3, 5, 10) },
            42.0
        );
        assert_eq!(
            unsafe { __torajs_num_parse_int_span(s.as_ptr(), 9, 10, 10) },
            7.0
        );
        assert!(unsafe { __torajs_num_parse_int_span(s.as_ptr(), 0, 2, 10) }.is_nan());
        assert!(unsafe { __torajs_num_parse_int_span(s.as_ptr(), 3, 3, 10) }.is_nan());
        assert_eq!(
            unsafe { __torajs_num_parse_int_span(s.as_ptr(), 0, 2, 16) },
            171.0
        );
    }

    #[test]
    fn missing_group_is_the_string_undefined() {
        let s = str_block(b"x", true);
        assert!(unsafe { __torajs_num_parse_int_span(s.as_ptr(), -1, -1, 10) }.is_nan());
        assert_eq!(
            unsafe { __torajs_num_parse_int_span(s.as_ptr(), -1, -1, 36) },
            parse_int(b"undefined", 36)
        );
    }

    #[test]
    fn utf16_subject_narrows_the_span_only() {
        let s = str_block(&utf16("日本a=\u{3000}-0x1F;"), false);
        assert_eq!(
            unsafe { __torajs_num_parse_int_span(s.as_ptr(), 4, 10, 0) },
            -31.0
        );
        assert_eq!(
            unsafe { __torajs_num_parse_int_span(s.as_ptr(), 4, 10, 16) },
            -31.0
        );
        assert!(unsafe { __torajs_num_parse_int_span(s.as_ptr(), 0, 2, 10) }.is_nan());
    }
}
