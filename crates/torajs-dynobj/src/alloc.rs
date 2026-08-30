//! DynObj allocation.
//!
//! Two fresh blocks (RFC 20260809-dynobj-store-split): a 32-byte
//! header cell (address-stable for the object's lifetime) and an
//! initial-cap store block, wired through the header's store pointer.
//! Header initialized to `+1`-rc, hash index filled with [`IDX_EMPTY`]
//! (all-ones — one `write_bytes` pass), dense entry array zeroed by
//! calloc (zero `key_ptr_tagged` = hole, but `entries_len = 0` means
//! iteration never reaches them).

use core::ffi::c_void;

use crate::layout::{
    DYNOBJ_CAP_OFF, DYNOBJ_COUNT_OFF, DYNOBJ_ENTRIES_CAP_OFF, DYNOBJ_ENTRIES_LEN_OFF,
    DYNOBJ_HEADER_BYTES, DYNOBJ_INITIAL_CAP, TAG_DYNOBJ, entries_cap_for, store_bytes,
};
use crate::probe::{set_store_ptr, store_ptr};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat calloc — zero-init alloc; v0.7-A2
    /// step 6b cutover.
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut c_void;
}

/// `__torajs_dynobj_alloc()` — allocate a fresh empty dynobj.
///
/// Header cell: 32 bytes, `refcount = 1`, `type_tag = TAG_DYNOBJ`,
/// `flags = 0`, `count = 0`, `cap = 8`, `entries_len = 0`,
/// `entries_cap = 7`, store wired. Store block: `store_bytes(8)` =
/// 8 × 4 + 7 × 16 = 144 bytes. Returns the fresh `+1`-rc header
/// pointer.
///
/// # Safety
/// Returned pointer is owned by the caller; release via
/// [`crate::drop::__torajs_dynobj_drop`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_alloc() -> *mut c_void {
    let cap = DYNOBJ_INITIAL_CAP;
    // Round 5 attack #3 — pool-first. A pooled header still carries
    // its initial-cap store on the store pointer; both blocks' bytes
    // are stale, and the re-init below covers everything a fresh
    // dynobj reads: header, counts, hash index. The dense entry
    // region is deliberately NOT cleared — `entries_len = 0` means no
    // reader reaches it before a set overwrites the slot it appends to.
    let p = match crate::pool::pop() {
        Some(nn) => nn.as_ptr(),
        None => unsafe {
            let h = calloc(DYNOBJ_HEADER_BYTES) as *mut u8;
            let s = calloc(store_bytes(cap)) as *mut u8;
            set_store_ptr(h as *mut c_void, s);
            h
        },
    };
    unsafe {
        // Header init: rc=1, tag=DynObj, flags=0.
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_DYNOBJ;
        *(p.add(6) as *mut u16) = 0;
        // count = 0 / entries_len = 0; cap / entries_cap = initial
        // sizing. (Explicit stores — the blocks may be recycled, so
        // calloc's zero fill cannot be assumed.)
        *(p.add(DYNOBJ_COUNT_OFF) as *mut u32) = 0;
        *(p.add(DYNOBJ_CAP_OFF) as *mut u32) = cap;
        *(p.add(DYNOBJ_ENTRIES_LEN_OFF) as *mut u32) = 0;
        *(p.add(DYNOBJ_ENTRIES_CAP_OFF) as *mut u32) = entries_cap_for(cap);
        // Hash index: all slots IDX_EMPTY (all-ones fill).
        core::ptr::write_bytes(store_ptr(p as *const c_void), 0xFF, cap as usize * 4);
    }
    p as *mut c_void
}

/// Test-only teardown for a raw two-block dynobj born from
/// [`__torajs_dynobj_alloc`] — sized frees for store + header,
/// bypassing the rc'd drop path (tests build bare keys the
/// production dropper must not touch).
#[cfg(test)]
pub(crate) unsafe fn free_dynobj_blocks(obj: *mut c_void) {
    unsafe extern "C" {
        #[link_name = "__torajs_free"]
        fn free(p: *mut c_void, size: usize);
    }
    unsafe {
        let cap = crate::probe::cap(obj);
        free(store_ptr(obj) as *mut c_void, store_bytes(cap));
        free(obj, DYNOBJ_HEADER_BYTES);
    }
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

/// `__torajs_dynobj_mark_module_ns(obj)` — set the module-namespace
/// flag bit (see [`crate::layout::DYNOBJ_HDR_FLAG_MODULE_NS`]) on a
/// namespace object right after its §10.4.6 attributes land. The write
/// paths read it to refuse; nothing else changes about the cell.
///
/// # Safety
/// `obj` is null (no-op) or a live dynobj heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_mark_module_ns(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    unsafe {
        let flags = (obj as *mut u8).add(6) as *mut u16;
        *flags |= crate::layout::DYNOBJ_HDR_FLAG_MODULE_NS;
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

/// `__torajs_dynobj_mark_class_ctor(obj)` — set the class-constructor
/// flag bit (see [`crate::layout::DYNOBJ_HDR_FLAG_CLASS_CTOR`]) on a
/// `__class_<C>` singleton dynobj. Called once per class from
/// `__torajs_anyv_class_register` at module init; `typeof` reads the
/// bit to answer `"function"` (RFC 20260717-class-first-class-value
/// knife A). Non-dynobj cells are ignored (defensive — the register
/// path already guards shape).
///
/// # Safety
/// `obj` is null (no-op) or a live heap pointer with a universal
/// header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_mark_class_ctor(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    if unsafe { crate::get::type_tag(obj) } != crate::layout::TAG_DYNOBJ {
        return;
    }
    unsafe {
        let flags = (obj as *mut u8).add(6) as *mut u16;
        *flags |= crate::layout::DYNOBJ_HDR_FLAG_CLASS_CTOR;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{DYNOBJ_ENTRY_SIZE, DYNOBJ_HDR_FLAG_NULL_PROTO, IDX_EMPTY};

    /// Header + metadata fields land at the expected offsets, with
    /// initial cap = 8 power-of-2, entries_cap = 7, count/entries_len
    /// zero, a wired store, and every index slot IDX_EMPTY.
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

            // Store is an independent live block.
            let s = store_ptr(p as *const c_void);
            assert!(!s.is_null(), "store wired");

            // Index init contract: probe relies on every slot reading
            // IDX_EMPTY in a fresh store (index at store offset 0).
            for i in 0..DYNOBJ_INITIAL_CAP as usize {
                assert_eq!(*(s.add(i * 4) as *const u32), IDX_EMPTY, "index[{i}]");
            }

            // Dense entries zero-init (calloc): key hole + zero value.
            let ent_base = DYNOBJ_INITIAL_CAP as usize * 4;
            for i in 0..entries_cap_for(DYNOBJ_INITIAL_CAP) as usize {
                let e = s.add(ent_base + i * DYNOBJ_ENTRY_SIZE);
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

            // Hand both blocks back to mmalloc (test-only path —
            // production drop helper lives in drop.rs).
            free_dynobj_blocks(p as *mut c_void);
        }
    }
}
