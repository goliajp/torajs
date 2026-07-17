//! DynObj allocation.
//!
//! Fresh `+1`-rc heap block, header initialized, hash index filled
//! with [`IDX_EMPTY`] (all-ones — one `write_bytes` pass), dense entry
//! array zeroed by calloc (zero `key_ptr_tagged` = hole, but
//! `entries_len = 0` means iteration never reaches them).

use core::ffi::c_void;

use crate::layout::{
    DYNOBJ_CAP_OFF, DYNOBJ_COUNT_OFF, DYNOBJ_ENTRIES_CAP_OFF, DYNOBJ_ENTRIES_LEN_OFF,
    DYNOBJ_INDEX_OFF, DYNOBJ_INITIAL_CAP, TAG_DYNOBJ, block_bytes, entries_cap_for,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat calloc — zero-init alloc; v0.7-A2
    /// step 6b cutover.
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut c_void;
}

/// `__torajs_dynobj_alloc()` — allocate a fresh empty dynobj.
///
/// Block size: `block_bytes(8)` = 24 + 8 × 4 + 7 × 16 = 168 bytes.
/// Header is initialized to `refcount = 1`, `type_tag = TAG_DYNOBJ`,
/// `flags = 0`. `count = 0`, `cap = 8`, `entries_len = 0`,
/// `entries_cap = 7`. Returns a fresh `+1`-rc heap pointer.
///
/// # Safety
/// Returned pointer is owned by the caller; release via
/// [`crate::drop::__torajs_dynobj_drop`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_alloc() -> *mut c_void {
    let cap = DYNOBJ_INITIAL_CAP;
    // Round 5 attack #3 — pool-first. A popped block carries stale
    // bytes; the re-init below covers everything a fresh dynobj
    // reads: header, counts, hash index. The dense entry region is
    // deliberately NOT cleared — `entries_len = 0` means no reader
    // reaches it before a set overwrites the slot it appends to.
    let p = match crate::pool::pop() {
        Some(nn) => nn.as_ptr(),
        None => unsafe { calloc(block_bytes(cap)) as *mut u8 },
    };
    unsafe {
        // Header init: rc=1, tag=DynObj, flags=0.
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_DYNOBJ;
        *(p.add(6) as *mut u16) = 0;
        // count = 0 / entries_len = 0; cap / entries_cap = initial
        // sizing. (Explicit stores — the block may be recycled, so
        // calloc's zero fill cannot be assumed.)
        *(p.add(DYNOBJ_COUNT_OFF) as *mut u32) = 0;
        *(p.add(DYNOBJ_CAP_OFF) as *mut u32) = cap;
        *(p.add(DYNOBJ_ENTRIES_LEN_OFF) as *mut u32) = 0;
        *(p.add(DYNOBJ_ENTRIES_CAP_OFF) as *mut u32) = entries_cap_for(cap);
        // Hash index: all slots IDX_EMPTY (all-ones fill).
        core::ptr::write_bytes(p.add(DYNOBJ_INDEX_OFF), 0xFF, cap as usize * 4);
    }
    p as *mut c_void
}

/// `__torajs_dynobj_mark_null_proto(obj)` — set the null-prototype
/// flag bit on a dynobj's heap header (see
/// [`crate::layout::DYNOBJ_HDR_FLAG_NULL_PROTO`]). Callers tag dicts
/// with `Object.create(null)` semantics (regex `.groups`) right after
/// alloc; print surfaces read it for the
/// `[Object: null prototype] ` prefix.
///
/// # Safety
/// `obj` is null (no-op) or a live dynobj heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_mark_null_proto(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    unsafe {
        let flags = (obj as *mut u8).add(6) as *mut u16;
        *flags |= crate::layout::DYNOBJ_HDR_FLAG_NULL_PROTO;
    }
}

/// `__torajs_dynobj_clear_null_proto(obj)` — clear the null-prototype
/// bit (the `Object.setPrototypeOf(o, cell)` re-parent face — RFC
/// 20260717-user-proto-chain knife 3).
///
/// # Safety
/// `obj` is null (no-op) or a live dynobj heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_clear_null_proto(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    unsafe {
        let flags = (obj as *mut u8).add(6) as *mut u16;
        *flags &= !crate::layout::DYNOBJ_HDR_FLAG_NULL_PROTO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{DYNOBJ_ENTRY_SIZE, DYNOBJ_HDR_FLAG_NULL_PROTO, IDX_EMPTY};

    /// Header + metadata fields land at the expected offsets, with
    /// initial cap = 8 power-of-2, entries_cap = 7, count/entries_len
    /// zero, and every index slot IDX_EMPTY.
    #[test]
    fn alloc_inits_header_and_metadata() {
        let p = unsafe { __torajs_dynobj_alloc() } as *mut u8;
        assert!(!p.is_null());
        unsafe {
            assert_eq!(*(p as *const u32), 1, "refcount");
            assert_eq!(*(p.add(4) as *const u16), TAG_DYNOBJ, "type_tag");
            assert_eq!(*(p.add(6) as *const u16), 0, "flags");
            assert_eq!(*(p.add(DYNOBJ_COUNT_OFF) as *const u32), 0, "count");
            assert_eq!(*(p.add(DYNOBJ_CAP_OFF) as *const u32), 8, "cap");
            assert_eq!(
                *(p.add(DYNOBJ_ENTRIES_LEN_OFF) as *const u32),
                0,
                "entries_len"
            );
            assert_eq!(
                *(p.add(DYNOBJ_ENTRIES_CAP_OFF) as *const u32),
                7,
                "entries_cap"
            );

            // Index init contract: probe relies on every slot reading
            // IDX_EMPTY in a fresh block.
            for i in 0..DYNOBJ_INITIAL_CAP as usize {
                assert_eq!(
                    *(p.add(DYNOBJ_INDEX_OFF + i * 4) as *const u32),
                    IDX_EMPTY,
                    "index[{i}]"
                );
            }

            // Dense entries zero-init (calloc): key hole + zero value.
            let ent_base = DYNOBJ_INDEX_OFF + DYNOBJ_INITIAL_CAP as usize * 4;
            for i in 0..entries_cap_for(DYNOBJ_INITIAL_CAP) as usize {
                let e = p.add(ent_base + i * DYNOBJ_ENTRY_SIZE);
                assert_eq!(*(e as *const u64), 0, "key_ptr_tagged");
                assert_eq!(*(e.add(8) as *const u64), 0, "value_anyv");
            }

            // Null-proto marking flips exactly the one flag bit.
            assert_eq!(*(p.add(6) as *const u16), 0, "flags pre-mark");
            __torajs_dynobj_mark_null_proto(p as *mut c_void);
            assert_eq!(
                *(p.add(6) as *const u16),
                DYNOBJ_HDR_FLAG_NULL_PROTO,
                "flags post-mark"
            );

            // Hand back to mmalloc (test-only path — production drop
            // helper lives in drop.rs). Layer 1 sized API: caller
            // passes original alloc size.
            unsafe extern "C" {
                #[link_name = "__torajs_free"]
                fn free(p: *mut c_void, size: usize);
            }
            free(p as *mut c_void, block_bytes(DYNOBJ_INITIAL_CAP));
        }
    }
}
