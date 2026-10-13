//! The Latin-1 projection of TrimString's set as a byte table, for
//! compiled code that trims in place: one load per byte classifies
//! it, where a compare chain spends several integer instructions on
//! every byte a trim stops at.

use crate::transform::trim::is_trim_ws;

/// `1` at every Latin-1 code unit [`is_trim_ws`] accepts, `0` elsewhere.
/// Read-only; exported unmangled so lowering can address it with a
/// `GlobalRef` — the Mach-O symbol is `___TORAJS_LATIN1_TRIM_WS`.
#[unsafe(no_mangle)]
pub static __TORAJS_LATIN1_TRIM_WS: [u8; 256] = table();

const fn table() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = is_trim_ws(i as u8) as u8;
        i += 1;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_marks_exactly_the_latin1_trim_set() {
        let set = [0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0xa0];
        for (i, &b) in __TORAJS_LATIN1_TRIM_WS.iter().enumerate() {
            assert_eq!(b, u8::from(set.contains(&i)), "byte {i:#04x}");
        }
    }
}
