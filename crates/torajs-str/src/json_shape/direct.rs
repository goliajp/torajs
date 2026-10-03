//! The shape stringify written straight into its result block.
//!
//! When every Str field is Latin-1 and needs no escape, the output
//! length is known before a byte is written: keys, digits, `true` /
//! `false` and the quoted strings all have fixed widths. One pass
//! sizes it, the next writes into a block of exactly that size, so
//! the builder, its growth checks and the final copy all drop out.
//! Anything else answers `None` and the builder lane runs instead.

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use torajs_fmt::itoa::{ITOA_BUF_LEN, itoa_into};
use torajs_rc::HeapHeader;

use super::OBJ_HEADER_SIZE;

/// One field of the descriptor, decoded.
struct Field<'a> {
    key: &'a [u8],
    ty: u8,
    slot: usize,
}

fn fields(d: &[u8]) -> impl Iterator<Item = Field<'_>> {
    let n = (d[0] - 1) as usize;
    let mut p = 1usize;
    (0..n).map(move |_| {
        let key_len = (d[p] - 1) as usize;
        let f = Field {
            key: &d[p + 3..p + 3 + key_len],
            ty: d[p + 1],
            slot: (d[p + 2] - 1) as usize,
        };
        p += 3 + key_len;
        f
    })
}

/// The Str field's payload when it is Latin-1 and writes verbatim
/// between quotes; `None` sends the record to the builder.
unsafe fn plain_str<'a>(v: *const u8) -> Option<&'a [u8]> {
    if v.is_null() {
        return None;
    }
    let flags = unsafe { (*(v as *const HeapHeader)).flags };
    if flags & STR_FLAG_IS_LATIN1 == 0 {
        return None;
    }
    let len = unsafe { (v.add(STR_LEN_OFF) as *const u32).read() } as usize;
    let s = unsafe { core::slice::from_raw_parts(v.add(STR_DATA_OFF), len) };
    let plain = s
        .iter()
        .all(|&c| (0x20..0x80).contains(&c) && c != b'"' && c != b'\\');
    plain.then_some(s)
}

/// Number fields a record may carry and still take this lane: their
/// spellings are made while sizing and kept for the write.
const MAX_NUMS: usize = 8;

/// `JSON.stringify(obj)` for the descriptor `d`, or `None` when a
/// Str field is null, UTF-16 or needs an escape, or the record has
/// more than [`MAX_NUMS`] number fields.
///
/// # Safety
///
/// `d` is a well-formed descriptor and `obj` a live struct object
/// whose slots match it.
pub(super) unsafe fn stringify(d: &[u8], obj: *const u8) -> Option<*mut u8> {
    let slot = |f: &Field| unsafe { obj.add(OBJ_HEADER_SIZE + f.slot * 8) };
    let mut nums = [[0u8; ITOA_BUF_LEN]; MAX_NUMS];
    let mut starts = [0usize; MAX_NUMS];
    let mut n_nums = 0usize;
    let mut total = 2usize;
    let mut present = 0usize;
    for f in fields(d) {
        let field = slot(&f);
        let width = match f.ty {
            1 => {
                if n_nums == MAX_NUMS {
                    return None;
                }
                let v = unsafe { (field as *const i64).read() };
                starts[n_nums] = itoa_into(v, &mut nums[n_nums]);
                n_nums += 1;
                ITOA_BUF_LEN - starts[n_nums - 1]
            }
            2 => {
                if unsafe { field.read() } != 0 {
                    4
                } else {
                    5
                }
            }
            _ => {
                let v = unsafe { (field as *const *const u8).read() };
                if crate::undef_sentinel::is_undef(v) {
                    continue;
                }
                unsafe { plain_str(v) }?.len() + 2
            }
        };
        total += f.key.len() + width;
        present += 1;
    }
    total += present.saturating_sub(1);

    let mut block = StrBlock::alloc_with_flags(total as u32, true, STR_FLAG_IS_ASCII);
    let out = unsafe { block.as_bytes_mut(total as u32) };
    let mut at = 0usize;
    put(out, &mut at, b"{");
    let mut first = true;
    let mut k = 0usize;
    for f in fields(d) {
        let field = slot(&f);
        let s = if f.ty == 3 {
            let v = unsafe { (field as *const *const u8).read() };
            if crate::undef_sentinel::is_undef(v) {
                continue;
            }
            unsafe { plain_str(v) }
        } else {
            None
        };
        if !first {
            put(out, &mut at, b",");
        }
        first = false;
        put(out, &mut at, f.key);
        match f.ty {
            1 => {
                put(out, &mut at, &nums[k][starts[k]..]);
                k += 1;
            }
            2 => {
                let t: &[u8] = if unsafe { field.read() } != 0 {
                    b"true"
                } else {
                    b"false"
                };
                put(out, &mut at, t);
            }
            _ => {
                let s = s.expect("sized as plain above");
                put(out, &mut at, b"\"");
                put(out, &mut at, s);
                put(out, &mut at, b"\"");
            }
        }
    }
    put(out, &mut at, b"}");
    debug_assert_eq!(at, total);
    Some(block.into_raw())
}

/// Copy `src` to `out[*at..]`. Keys and short strings are a few to a
/// dozen bytes: two overlapping moves cover them without a call.
#[inline(always)]
fn put(out: &mut [u8], at: &mut usize, src: &[u8]) {
    let n = src.len();
    assert!(*at + n <= out.len());
    let s = src.as_ptr();
    // SAFETY: the assert bounds the destination; `src` is `n` bytes
    let d = unsafe { out.as_mut_ptr().add(*at) };
    unsafe {
        match n {
            0 => {}
            1..=3 => {
                *d = *s;
                *d.add(n / 2) = *s.add(n / 2);
                *d.add(n - 1) = *s.add(n - 1);
            }
            4..=7 => {
                (d as *mut u32).write_unaligned((s as *const u32).read_unaligned());
                (d.add(n - 4) as *mut u32)
                    .write_unaligned((s.add(n - 4) as *const u32).read_unaligned());
            }
            8..=16 => {
                (d as *mut u64).write_unaligned((s as *const u64).read_unaligned());
                (d.add(n - 8) as *mut u64)
                    .write_unaligned((s.add(n - 8) as *const u64).read_unaligned());
            }
            _ => core::ptr::copy_nonoverlapping(s, d, n),
        }
    }
    *at += n;
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn put_widths() {
        let src: Vec<u8> = (b'a'..=b'z').collect();
        for n in 0..=20 {
            let mut out = [0u8; 32];
            let mut at = 3;
            put(&mut out, &mut at, &src[..n]);
            assert_eq!(at, 3 + n);
            assert_eq!(&out[3..3 + n], &src[..n]);
            assert!(out[..3].iter().chain(&out[3 + n..]).all(|&b| b == 0));
        }
    }
}
