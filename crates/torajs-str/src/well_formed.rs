//! `String.prototype.isWellFormed` / `toWellFormed` (ES2024
//! §22.1.3.10 / §22.1.3.30).
//!
//! A Str is Latin-1 or UTF-16 LE, and the UTF-16 form holds lone
//! surrogates (`'a' + '\uD83D' + 'c'`), so neither answer is a
//! constant: a Latin-1 payload has no surrogate units and is always
//! well-formed; a UTF-16 one is well-formed unless a leading surrogate
//! lacks its trailing half or a trailing one lacks its leading half
//! (§11.1.4 CodePointAt's `[[IsUnpairedSurrogate]]`), and
//! `toWellFormed` writes U+FFFD over each such unit.

use alloc::vec::Vec;

use torajs_rc::HeapHeader;

use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};

const REPLACEMENT: u16 = 0xFFFD;

/// The payload and its encoding.
///
/// # Safety
/// `p` is a live Str block.
unsafe fn view<'a>(p: *const u8) -> (&'a [u8], bool) {
    let length = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() } as usize;
    let latin1 = unsafe { (*(p as *const HeapHeader)).flags } & STR_FLAG_IS_LATIN1 != 0;
    let bytes = if latin1 { length } else { length * 2 };
    (
        unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), bytes) },
        latin1,
    )
}

/// Index of every unpaired surrogate among `units`.
fn unpaired(units: &[u16]) -> impl Iterator<Item = usize> + '_ {
    let lead = |u: u16| (0xD800..=0xDBFF).contains(&u);
    let trail = |u: u16| (0xDC00..=0xDFFF).contains(&u);
    (0..units.len()).filter(move |&i| {
        let u = units[i];
        (lead(u) && !units.get(i + 1).is_some_and(|&n| trail(n)))
            || (trail(u) && !(i > 0 && lead(units[i - 1])))
    })
}

fn utf16_units(payload: &[u8]) -> Vec<u16> {
    payload
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// `s.isWellFormed()`: 1 when no code unit is an unpaired surrogate.
///
/// # Safety
/// `s` is a live Str block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_is_well_formed(s: *const u8) -> i64 {
    let (payload, latin1) = unsafe { view(s) };
    i64::from(latin1 || unpaired(&utf16_units(payload)).next().is_none())
}

/// `s.toWellFormed()`: a fresh Str with every unpaired surrogate
/// replaced by U+FFFD, owned by the caller.
///
/// # Safety
/// `s` is a live Str block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_well_formed(s: *const u8) -> *mut u8 {
    let (payload, latin1) = unsafe { view(s) };
    if latin1 {
        return crate::alloc_canonical::alloc_units_canonical(payload, true);
    }
    let mut units = utf16_units(payload);
    let bad: Vec<usize> = unpaired(&units).collect();
    for i in bad {
        units[i] = REPLACEMENT;
    }
    let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
    crate::alloc_canonical::alloc_units_canonical(&bytes, false)
}

/// Any-lane `isWellFormed`: `s` may be a Substr view.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_is_well_formed(s: *const u8) -> i64 {
    unsafe {
        let (src, tmp) = crate::method_any::owned_src(s);
        let r = __torajs_str_is_well_formed(src);
        crate::method_any::drop_tmp(tmp);
        r
    }
}

/// Any-lane `toWellFormed`: `s` may be a Substr view; the answer is an
/// owned Str, boxed by its pointer.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_to_well_formed(s: *const u8) -> u64 {
    unsafe {
        let (src, tmp) = crate::method_any::owned_src(s);
        let out = __torajs_str_to_well_formed(src);
        crate::method_any::drop_tmp(tmp);
        out as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn unpaired_surrogates_are_found_on_either_side() {
        // a, lone lead, c, lone trail, pair
        let units = [0x61, 0xD83D, 0x63, 0xDCA9, 0xD83D, 0xDCA9];
        assert_eq!(unpaired(&units).collect::<Vec<_>>(), vec![1, 3]);
        assert_eq!(unpaired(&[0xDCA9, 0xD83D]).count(), 2);
        assert_eq!(unpaired(&[0xD83D, 0xDCA9]).count(), 0);
        assert_eq!(unpaired(&[0xD83D]).count(), 1);
    }
}
