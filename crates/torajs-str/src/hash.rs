//! `__torajs_str_hash` — a Str cell's content hash, the twin of
//! [`crate::eq::__torajs_str_eq`]: whatever that kernel calls equal
//! hashes equal here.
//!
//! The walk is over **code units**, not payload bytes. A Substr VIEW
//! inherits its parent's encoding and cannot narrow, so a UTF-16 view
//! whose units all fit Latin-1 (`"\u{6C49}abc".slice(1)`) has content
//! equal to the canonical Latin-1 `"abc"` while its payload is twice
//! as long — a byte walk lands the two in different Map buckets and
//! `map.get(s.slice(1))` misses (rotation 560-01). Reading a view's
//! payload also has to go through its parent; the owned-Str layout
//! read hashed the parent pointer and offset fields as "content".
//!
//! FNV-1a 64 with each code unit as one input word. For a Latin-1
//! payload a unit is its byte, so the value equals the plain FNV-1a
//! of the bytes — the hash an ASCII key had before this kernel.

use crate::eq::resolve_payload;

/// FNV-1a 64 over the cell's code units — Latin-1 bytes as they
/// are, UTF-16 as little-endian u16 values — view-aware. Not
/// finalized: the Map side mixes and truncates for its slot field.
///
/// # Safety
///
/// `s` must point at a valid owned-Str or Substr block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_hash(s: *const u8) -> u64 {
    // SAFETY: caller's invariant.
    let (payload, latin1) = unsafe { resolve_payload(s) };
    let mut h: u64 = 0xcbf29ce484222325;
    if latin1 {
        for &b in payload {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    } else {
        for pair in payload.chunks_exact(2) {
            h ^= u16::from_le_bytes([pair[0], pair[1]]) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::StrBlock;
    use crate::substr::SubstrBlock;
    use core::ffi::c_void;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn make_str(bytes: &[u8]) -> StrBlock {
        let mut block = StrBlock::alloc(bytes.len() as u32);
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u32)
                .copy_from_slice(bytes)
        };
        block
    }

    fn make_utf16(units: &[u16]) -> StrBlock {
        let block = StrBlock::alloc_with_encoding(units.len() as u32, false);
        let mut at = unsafe { block.0.as_ptr().add(crate::layout::STR_DATA_OFF) };
        for u in units {
            let le = u.to_le_bytes();
            unsafe {
                at.write(le[0]);
                at.add(1).write(le[1]);
                at = at.add(2);
            }
        }
        block
    }

    fn fnv_bytes(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    #[test]
    fn latin1_hash_is_the_fnv1a_of_its_bytes() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let a = make_str(b"abc");
        assert_eq!(
            unsafe { __torajs_str_hash(a.0.as_ptr()) },
            fnv_bytes(b"abc")
        );
        let e = make_str(b"");
        assert_eq!(unsafe { __torajs_str_hash(e.0.as_ptr()) }, fnv_bytes(b""));
        a.free_pool_aware();
        e.free_pool_aware();
    }

    #[test]
    fn utf16_hashes_per_unit_and_agrees_with_latin1_content() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        // Same content, both encodings: equal by `__torajs_str_eq`,
        // so equal here.
        let narrow = make_str(b"abc");
        let wide = make_utf16(&[0x61, 0x62, 0x63]);
        assert_eq!(unsafe { __torajs_str_hash(narrow.0.as_ptr()) }, unsafe {
            __torajs_str_hash(wide.0.as_ptr())
        });
        // A unit above 0xFF is one input word, not two bytes: the
        // hash of "ㄱ" (U+3131) is not the hash of "1" (0x31).
        let hangul = make_utf16(&[0x3131]);
        let one = make_str(b"1");
        assert_ne!(unsafe { __torajs_str_hash(hangul.0.as_ptr()) }, unsafe {
            __torajs_str_hash(one.0.as_ptr())
        });
        // Units that agree on their low bytes hash apart.
        let d800 = make_utf16(&[0xD800]);
        let dc00 = make_utf16(&[0xDC00]);
        assert_ne!(unsafe { __torajs_str_hash(d800.0.as_ptr()) }, unsafe {
            __torajs_str_hash(dc00.0.as_ptr())
        });
        narrow.free_pool_aware();
        wide.free_pool_aware();
        hangul.free_pool_aware();
        one.free_pool_aware();
        d800.free_pool_aware();
        dc00.free_pool_aware();
    }

    #[test]
    fn a_view_hashes_as_its_content_not_its_fields() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        crate::substr::pool_clear_for_test();
        // Latin-1 parent "xabc", view [1..4) = "abc".
        let parent = make_str(b"xabc");
        let view = unsafe { SubstrBlock::create(parent.0.as_ptr() as *mut c_void, 1, 3) };
        let owned = make_str(b"abc");
        assert_eq!(unsafe { __torajs_str_hash(view.0.as_ptr()) }, unsafe {
            __torajs_str_hash(owned.0.as_ptr())
        });
        view.drop_pool_aware();
        parent.free_pool_aware();
        // UTF-16 parent "汉abc", view [1..4) — content "abc" in a
        // wide payload — hashes with the canonical Latin-1 "abc".
        let wide_parent = make_utf16(&[0x6C49, 0x61, 0x62, 0x63]);
        let wide_view = unsafe { SubstrBlock::create(wide_parent.0.as_ptr() as *mut c_void, 1, 3) };
        assert_eq!(unsafe { __torajs_str_hash(wide_view.0.as_ptr()) }, unsafe {
            __torajs_str_hash(owned.0.as_ptr())
        });
        wide_view.drop_pool_aware();
        wide_parent.free_pool_aware();
        owned.free_pool_aware();
    }
}
