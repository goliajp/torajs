//! The found-only lookup of a Str key, for the paths that only ask
//! which entry owns the key (the number dictionary's read and write).
//!
//! [`crate::probe::probe`] also answers where an insert would go and
//! compares keys through [`crate::probe::key_eq`], whose payload
//! compare is a `memcmp` call; that keeps a stack frame and a dozen
//! saved registers on every walk. [`find_str`] walks the same index
//! with the same hash, remembers no tombstone and compares payloads
//! in place, so it is a leaf with no frame. [`cached`] answers before
//! any walk when the same key cell found its entry before.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::layout::{IDX_EMPTY, IDX_TOMBSTONE, STR_DATA_OFF, STR_LEN_OFF, TAG_SYMBOL_KEY};
use crate::probe::{
    Entry, KEY_HDR_FLAGS_OFF, KEY_STR_FLAG_IS_LATIN1, bucket_key_ptr, cap, entries, entries_len,
    index_ptr,
};

/// Slots of [`FOUND_BY_CELL`]; a power of two.
const FOUND_SLOTS: usize = 1024;

/// The entry index [`find_str_noted`] found last for a key cell, in
/// whichever object, filed by the cell's address.
///
/// A hint, never a fact: [`cached`] accepts it only where the object
/// has a live entry at that index holding the very key cell asked for.
/// An entry owns a share of its key cell, so the cell cannot have been
/// freed and reused while the entry holds it, and an object has one
/// entry per key; a stale index (another object, another cell filed in
/// the same slot, a deleted entry, a rebuilt store) fails the test and
/// costs only the test. Keys taken from the same cells over and over
/// (a field of each record, the names `Object.keys` answered) then find
/// their entry without hashing.
static FOUND_BY_CELL: [AtomicU32; FOUND_SLOTS] = [const { AtomicU32::new(0) }; FOUND_SLOTS];

/// The [`FOUND_BY_CELL`] slot of the key cell `key`, by address above
/// the low four bits (an allocation granule, the same for most cells).
#[inline(always)]
fn found_slot(key: *const c_void) -> &'static AtomicU32 {
    &FOUND_BY_CELL[(key as usize >> 4) & (FOUND_SLOTS - 1)]
}

/// `(header flags, length in code units)` of a Str cell.
///
/// # Safety
/// `key` is a live Str cell.
#[inline(always)]
unsafe fn str_shape(key: *const c_void) -> (u16, u32) {
    unsafe {
        let p = key.cast::<u8>();
        (
            (p.add(KEY_HDR_FLAGS_OFF) as *const u16).read(),
            (p.add(STR_LEN_OFF) as *const u32).read(),
        )
    }
}

/// The index of the entry of `obj` whose key is the Str `key`, by
/// content (the probe's §6.1.7 key equality, restricted to a Str key:
/// a Symbol entry never matches).
///
/// # Safety
/// `obj` is a live dynobj; `key` is a live Str cell.
#[inline]
pub(crate) unsafe fn find_str(obj: *const c_void, key: *const c_void) -> Option<u32> {
    unsafe {
        let (flags, len) = str_shape(key);
        let latin1 = flags & KEY_STR_FLAG_IS_LATIN1;
        let n = if latin1 != 0 {
            len as usize
        } else {
            len as usize * 2
        };
        let data = key.cast::<u8>().add(STR_DATA_OFF);
        // FNV-1a over the payload, the hash `probe::hash_str` computes
        let mut h: u64 = 0xcbf29ce484222325;
        for i in 0..n {
            h ^= *data.add(i) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let cap = cap(obj);
        let mask = cap - 1;
        let idx = index_ptr(obj);
        let ent = entries(obj);
        let mut slot = (h as u32) & mask;
        for _ in 0..cap {
            let iv = *idx.add(slot as usize);
            if iv == IDX_EMPTY {
                return None;
            }
            if iv != IDX_TOMBSTONE {
                let kp = bucket_key_ptr((*ent.add(iv as usize)).key_ptr_tagged);
                if std::ptr::eq(kp as *const c_void, key) || same_str(kp, latin1, len, data, n) {
                    return Some(iv);
                }
            }
            slot = (slot + 1) & mask;
        }
        None
    }
}

/// True when the entry key `kp` is a Str with `key`'s encoding, length
/// and `n` payload bytes at `data`.
///
/// # Safety
/// `kp` is a live Str or Symbol cell; `data` holds `n` bytes.
#[inline(always)]
unsafe fn same_str(kp: *const c_void, latin1: u16, len: u32, data: *const u8, n: usize) -> bool {
    unsafe {
        if (kp.cast::<u8>().add(4) as *const u16).read() == TAG_SYMBOL_KEY {
            return false;
        }
        let (kflags, klen) = str_shape(kp);
        if klen != len || kflags & KEY_STR_FLAG_IS_LATIN1 != latin1 {
            return false;
        }
        let kdata = kp.cast::<u8>().add(STR_DATA_OFF);
        let mut i = 0;
        while i < n {
            if *kdata.add(i) != *data.add(i) {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// [`find_str`], remembering the index it found for [`cached`].
///
/// # Safety
/// As [`find_str`].
#[inline]
pub(crate) unsafe fn find_str_noted(obj: *const c_void, key: *const c_void) -> Option<u32> {
    let found = unsafe { find_str(obj, key) };
    if let Some(i) = found {
        found_slot(key).store(i, Ordering::Relaxed);
    }
    found
}

/// The entry of `obj` holding the very key cell `key` at the index
/// [`find_str_noted`] last found for that cell, if there is one.
///
/// # Safety
/// `obj` is a live dynobj; `key` is a live Str cell.
#[inline(always)]
pub(crate) unsafe fn cached(obj: *const c_void, key: *const c_void) -> Option<*mut Entry> {
    let i = found_slot(key).load(Ordering::Relaxed);
    unsafe {
        if i >= entries_len(obj) {
            return None;
        }
        let e = entries(obj).add(i as usize);
        std::ptr::eq(bucket_key_ptr((*e).key_ptr_tagged) as *const c_void, key).then_some(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::{__torajs_dynobj_alloc, free_dynobj_blocks};
    use crate::layout::{BUCKET_FLAGS_DEFAULT, DYNOBJ_KEY_HOLE};
    use crate::probe::{
        bucket_make_key_tagged, count, entries_cap, probe, set_count, set_entries_len,
    };
    use crate::resize::resize;

    /// A Str cell: Latin-1 bytes, or UTF-16 LE units when `wide`.
    fn make_str(units: &[u16], wide: bool) -> Vec<u64> {
        let bytes: Vec<u8> = if wide {
            units.iter().flat_map(|u| u.to_le_bytes()).collect()
        } else {
            units.iter().map(|&u| u as u8).collect()
        };
        let mut v = vec![0u64; (STR_DATA_OFF + bytes.len()).div_ceil(8)];
        unsafe {
            let p = v.as_mut_ptr() as *mut u8;
            if !wide {
                *(p.add(KEY_HDR_FLAGS_OFF) as *mut u16) = KEY_STR_FLAG_IS_LATIN1;
            }
            *(p.add(STR_LEN_OFF) as *mut u64) = units.len() as u64;
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(STR_DATA_OFF), bytes.len());
        }
        v
    }

    fn latin1(s: &str) -> Vec<u64> {
        make_str(&s.bytes().map(u16::from).collect::<Vec<_>>(), false)
    }

    /// A fresh entry the way the set path adds one (minus the key
    /// retain: the tests own their keys).
    unsafe fn insert(obj: *mut c_void, key: *mut c_void) {
        unsafe {
            if entries_len(obj) == entries_cap(obj) {
                resize(obj);
            }
            let pr = probe(obj, key);
            assert!(!pr.found);
            let e_idx = entries_len(obj);
            *entries(obj).add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, BUCKET_FLAGS_DEFAULT),
                value_anyv: 0,
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }

    /// Over a table with colliding Latin-1, UTF-16 and Symbol keys,
    /// the leaf lookup finds what the general probe finds, by content
    /// (a copy of a key in another cell) and only by content.
    #[test]
    fn find_str_agrees_with_probe() {
        let mut keys: Vec<Vec<u64>> = (0..40).map(|i| latin1(&format!("k{i}"))).collect();
        keys.push(make_str(&[0x4241], true));
        keys.push(make_str(&[0xD800, 0x41], true));
        let mut sym = vec![0u64; 2];
        unsafe { *((sym.as_mut_ptr() as *mut u8).add(4) as *mut u16) = TAG_SYMBOL_KEY };
        unsafe {
            let obj = __torajs_dynobj_alloc();
            for k in &keys {
                insert(obj, k.as_ptr() as *mut c_void);
            }
            insert(obj, sym.as_mut_ptr() as *mut c_void);
            for (i, k) in keys.iter().enumerate() {
                let copy = k.clone();
                let p = probe(obj, copy.as_ptr() as *const c_void);
                assert!(p.found);
                assert_eq!(p.entry as usize, i);
                assert_eq!(find_str(obj, copy.as_ptr() as *const c_void), Some(p.entry));
            }
            let misses = [
                latin1("AB"),
                latin1("k40"),
                latin1(""),
                make_str(&[0xD800], true),
            ];
            for miss in misses {
                assert_eq!(find_str(obj, miss.as_ptr() as *const c_void), None);
            }
            free_dynobj_blocks(obj);
        }
    }

    /// A remembered index answers only for the very key cell at that
    /// index in the object asked about.
    #[test]
    fn cached_answers_only_the_same_cell_at_the_same_index() {
        let (ka, kb) = (latin1("a"), latin1("b"));
        let ka2 = ka.clone();
        let a = ka.as_ptr() as *mut c_void;
        let b = kb.as_ptr() as *mut c_void;
        unsafe {
            let x = __torajs_dynobj_alloc();
            let y = __torajs_dynobj_alloc();
            insert(x, a);
            insert(x, b);
            insert(y, b);
            assert_eq!(find_str_noted(x, b), Some(1));
            assert_eq!(cached(x, b), Some(entries(x).add(1)));
            // another object holding the cell at another index
            assert_eq!(cached(y, b), None);
            assert_eq!(find_str_noted(x, a), Some(0));
            assert_eq!(cached(x, a), Some(entries(x)));
            // a copy in another cell has its own slot
            assert_eq!(cached(x, ka2.as_ptr() as *const c_void), None);
            assert_eq!(cached(y, a), None);
            // each cell keeps its own index
            assert_eq!(cached(x, b), Some(entries(x).add(1)));
            assert_eq!(find_str_noted(y, b), Some(0));
            assert_eq!(cached(y, b), Some(entries(y)));
            assert_eq!(cached(x, b), None);
            // a deleted entry keeps its index but not its key
            (*entries(x)).key_ptr_tagged = DYNOBJ_KEY_HOLE;
            assert_eq!(cached(x, a), None);
            free_dynobj_blocks(x);
            free_dynobj_blocks(y);
        }
    }
}
