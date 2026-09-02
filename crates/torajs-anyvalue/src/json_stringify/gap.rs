//! The ES §25.5.2.1 `space` argument — normalizing it into a gap and
//! spending that gap as indentation. Split out of the parent under
//! the 500-line file discipline; a child module reaches its parent's
//! private items, so the walk body it delegates to stays private.

use core::ffi::c_void;

use super::*;

/// `JSON.stringify(value, replacer, space)` under an already
/// normalized gap. `depth` is the nesting level the value sits at,
/// so an any-typed member of a statically unfolded composite keeps
/// indenting from its parent's level instead of restarting at zero.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern; `gap` is a live Str
/// block (or NULL for no indent).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_json_stringify_gap(
    v: AnyValue,
    gap: *const u8,
    depth: i64,
) -> *mut u8 {
    unsafe { stringify_with_gap_at(v, gap, depth.max(0) as u32) }
}

/// ES §25.5.2.1 steps 5-8 — normalize a `space` argument into a gap,
/// handed back as a Str cell. A Number (or Number object) becomes
/// `min(10, ToIntegerOrInfinity(space))` spaces, a String (or String
/// object) its first 10 code units, anything else the empty gap. The
/// static unfold cannot take a Rust slice, so it asks for the gap
/// once at the call site and threads that cell through its own
/// recursion.
/// Answers a fresh refcount=1 Str the caller drops (empty for "no
/// indent", which the caller's own compile-time gate makes
/// unreachable in practice).
///
/// # Safety
/// `space` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_json_gap_str(space: AnyValue) -> *mut u8 {
    unsafe {
        let gap = gap_of(space);
        __torajs_str_alloc(gap.as_ptr(), gap.len() as i64)
    }
}

/// The §25.5.2.1 step 5-8 normalization itself.
unsafe fn gap_of(space: AnyValue) -> Vec<u8> {
    unsafe {
        // Step 5 unwraps a Number / String wrapper before the split.
        let space = if is_cell(space) {
            let ptr = as_void_ptr(space);
            let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
            if tag == Tag::NumberWrapper as u16 {
                box_double(((ptr as *const u8).add(8) as *const f64).read())
            } else if tag == Tag::StringWrapper as u16 {
                let inner = ((ptr as *const u8).add(8) as *const *const c_void).read();
                if inner.is_null() {
                    return Vec::new();
                }
                box_void_ptr(inner as *mut c_void)
            } else {
                space
            }
        } else {
            space
        };
        if is_int32(space) {
            let n = as_int32(space).clamp(0, 10) as usize;
            return vec![b' '; n];
        }
        if is_double(space) {
            let d = as_double(space);
            // ToIntegerOrInfinity truncates toward zero; NaN is 0.
            let n = if d.is_nan() { 0.0 } else { d.trunc() };
            let n = n.clamp(0.0, 10.0) as usize;
            return vec![b' '; n];
        }
        // Step 7 — a string gap keeps its first 10 code units, cut
        // from the cell's WTF-8 spelling (the gap is spent into a
        // WTF-8 output buffer; the pre-560 form copied the first 10
        // PAYLOAD BYTES — half of a UTF-16 gap).
        if is_short_str(space)
            || (is_cell(space) && {
                let ptr = as_void_ptr(space);
                (ptr.cast::<u8>().add(4) as *const u16).read() == Tag::Str as u16
            })
        {
            let cell = crate::nanbox_ffi::__torajs_anyv_to_str(space);
            let spelling = torajs_rc::str_wtf8::StrWtf8::of(cell.cast());
            let out = first_code_units(spelling.as_bytes(), 10);
            __torajs_str_drop(cell);
            return out;
        }
        Vec::new()
    }
}

/// The WTF-8 prefix spelling the first `n` code units of `wtf8` — a
/// four-byte scalar counts two (its surrogate pair), everything else
/// one. A cut between the two halves of a pair keeps the high
/// surrogate alone, in its three-byte WTF-8 form, which is what
/// §25.5.2.1 step 7's code-unit slice leaves behind.
fn first_code_units(wtf8: &[u8], n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(wtf8.len().min(n * 3));
    let mut units = 0usize;
    let mut i = 0usize;
    while units < n {
        let Some(&b) = wtf8.get(i) else { break };
        let width = match b {
            0xF0.. => 4,
            0xE0.. => 3,
            0xC0.. => 2,
            _ => 1,
        };
        let Some(seq) = wtf8.get(i..i + width) else {
            break;
        };
        if width == 4 {
            if units + 2 > n {
                let cp = ((seq[0] as u32 & 0x07) << 18)
                    | ((seq[1] as u32 & 0x3F) << 12)
                    | ((seq[2] as u32 & 0x3F) << 6)
                    | (seq[3] as u32 & 0x3F);
                let hi = 0xD800 + ((cp - 0x10000) >> 10);
                out.extend_from_slice(&[
                    0xED,
                    0x80 | ((hi >> 6) & 0x3F) as u8,
                    0x80 | (hi & 0x3F) as u8,
                ]);
                break;
            }
            units += 2;
        } else {
            units += 1;
        }
        out.extend_from_slice(seq);
        i += width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::first_code_units;

    #[test]
    fn cuts_by_code_unit_not_byte() {
        // 12 CJK units → 10 kept, each three WTF-8 bytes
        let s = "中".repeat(12);
        assert_eq!(
            first_code_units(s.as_bytes(), 10),
            "中".repeat(10).as_bytes()
        );
        // Latin-1 é is two WTF-8 bytes but one unit
        let e = "é".repeat(11);
        assert_eq!(
            first_code_units(e.as_bytes(), 10),
            "é".repeat(10).as_bytes()
        );
        assert_eq!(first_code_units(b"abc", 10), b"abc");
    }

    #[test]
    fn surrogate_pair_counts_two_and_splits_at_the_cut() {
        let six = "😀".repeat(6);
        assert_eq!(
            first_code_units(six.as_bytes(), 10),
            "😀".repeat(5).as_bytes()
        );
        let mut want = b"a".to_vec();
        want.extend_from_slice("😀".repeat(4).as_bytes());
        want.extend_from_slice(&[0xED, 0xA0, 0xBD]); // lone U+D83D
        let s = format!("a{}", "😀".repeat(6));
        assert_eq!(first_code_units(s.as_bytes(), 10), want);
    }
}
