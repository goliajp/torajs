//! `<key> in <obj>` for `Type::Any` rhs operands.
//!
//! ssa_lower's static `in`-op path (ssa_lower.rs T-45) dispatches on
//! the rhs operand's SSA type:
//!   Type::Arr(_) → inline numeric bounds check
//!   Type::Any   → previously: `__torajs_dynobj_has(any_unbox_value, key)`
//!                 — assumed every Any-tagged heap cell was a DynObj,
//!                 so an `arr: any = [1,2,3]; 0 in arr` SIGSEGV'd
//!                 (Array heap layout ≠ DynObj layout).
//!
//! This helper closes that gap. ssa_lower now routes the Type::Any
//! arm to one of two helpers, picked by the **key's** static SSA type:
//!
//!   `__torajs_in_op_any_num(v, key_i64)` — key is Number-typed
//!     • Tag::Arr   → bounds check  (0 ≤ key < len@+8)
//!     • else       → false (DynObj numeric-key + str ToString is
//!                    deferred — narrow ship; L3b watch)
//!
//!   `__torajs_in_op_any_str(v, key_str_ptr)` — key is String-typed
//!     • Tag::DynObj → `__torajs_dynobj_has(ptr, key)`
//!     • Tag::Arr    → canonical index within bounds
//!     • Tag::Obj    → `__torajs_any_prop_has` (declared fields +
//!                     accessor slots; pre-fix a struct through `any`
//!                     answered false even for a field it plainly has)
//!     • else        → false
//!
//! Both helpers mirror `instanceof_any` discipline: NaN-box unbox →
//! tag-gate (ANY_HEAP) → NULL-gate → `HeapHeader::type_tag@+4` read.

use core::ffi::c_void;

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_anyv_unbox_tag(v: i64) -> i64;
    fn __torajs_anyv_unbox_value(v: i64) -> i64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const u8) -> i32;
    // torajs-dynobj — hole-tombstone probe (an elision / deleted
    // index reads undefined but is NOT an own property).
    fn __torajs_dynobj_entry_is_hole(obj: *const c_void, key: *const u8) -> i32;
    // torajs-num runtime symbol — resolved at `tr build` link time;
    // torajs-rc keeps 0 Cargo deps (vision §2). Same pattern as
    // dynobj_has / anyv_unbox above.
    fn __torajs_num_to_string_radix_i(n: i64, radix: i64) -> *mut u8;
    // torajs-anyvalue — the shared own-property predicate for a
    // `Tag::Obj` struct receiver (declared fields + accessor slots).
    // Same link-time-resolved, zero-Cargo-dep pattern as the unbox
    // helpers above.
    fn __torajs_any_prop_has(recv: u64, key: *const c_void) -> i64;
    // torajs-anyvalue — non-zero when the cell is a builtin
    // `<Ctor>.prototype` singleton owning `key` as an interned family
    // method (it lives in the method-cell table, not in any entry
    // table, yet is an own property per spec). -1/0 for every other
    // receiver.
    fn __torajs_builtin_proto_own_method_cell(proto: *const c_void, key: *const c_void) -> u64;
}

// Offset of the i64 `len` slot inside the Array heap block — matches
// `ARR_LEN_OFF = 8` in ssa_lower.rs (8 bytes after the universal
// HeapHeader).
const ARR_LEN_OFF: usize = 8;

// Offset of the inline props-dynobj slot inside the Array heap block
// (`torajs_arr::layout::ARR_PROPS_OFF`) — NULL until the array takes
// its first non-index property.
const ARR_PROPS_OFF: usize = 24;

// Tag value ssa_lower emits for NaN-boxed heap pointers (mirrors
// `ANY_TAG_HEAP = 4` from torajs-anyvalue / ssa_lower).
const ANY_TAG_HEAP: i64 = 4;

// `torajs_rc::Tag` numeric values (stable ABI per assert_eq! suite
// in `lib.rs`):
const TAG_ARR: u16 = 2;
const TAG_OBJ: u16 = 1;
const TAG_DYNOBJ: u16 = 14;

// Str heap layout (mirrors `torajs_str::layout`):
//   plain Str:   [hdr@0..8][len:u32@+8][pad@+12..16][data@+16]
//   Substr:     same hdr, FLAG_SUBSTR_INLINE (bit 0 of flags@+6)
//                set, followed by [len:u64@+8][parent:*@+16][off:u64@+24].
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const SUBSTR_LEN_OFF: usize = 8;
const SUBSTR_PARENT_OFF: usize = 16;
const SUBSTR_OFFSET_OFF: usize = 24;
const HDR_FLAGS_OFF: usize = 6;
const FLAG_SUBSTR_INLINE: u16 = 1;

// Read the canonical-shape byte view of a Str/Substr heap block.
//
// # Safety
// `str_ptr` must point at a live torajs-str heap block (plain Str or
// Substr — both branches read inside the block's own header).
unsafe fn str_view(str_ptr: *const u8) -> (*const u8, usize) {
    let flags = unsafe { *(str_ptr.add(HDR_FLAGS_OFF) as *const u16) };
    if flags & FLAG_SUBSTR_INLINE != 0 {
        let len = unsafe { *(str_ptr.add(SUBSTR_LEN_OFF) as *const u64) } as usize;
        let parent = unsafe { *(str_ptr.add(SUBSTR_PARENT_OFF) as *const *const u8) };
        let off = unsafe { *(str_ptr.add(SUBSTR_OFFSET_OFF) as *const u64) } as usize;
        (unsafe { parent.add(STR_DATA_OFF + off) }, len)
    } else {
        let len = unsafe { *(str_ptr.add(STR_LEN_OFF) as *const u32) } as usize;
        (unsafe { str_ptr.add(STR_DATA_OFF) }, len)
    }
}

// Spec ECMA-262 §7.1.21 CanonicalNumericIndexString — accepts the
// canonical-shape integer-index strings the Array `[[HasProperty]]`
// path treats as indexes: `"0" / "1" / ... / "4294967294"`. Rejects
// every non-canonical roundtrip — `""`, leading-`+`, leading-`-`,
// leading-`0` followed by another digit, embedded non-digit, and
// values ≥ 2^32-1 (Array max length minus 1).
//
// Returns `Some(idx)` for an accepted index, `None` otherwise.
unsafe fn parse_canonical_array_index(str_ptr: *const u8) -> Option<i64> {
    let (data, len) = unsafe { str_view(str_ptr) };
    if len == 0 || len > 10 {
        return None;
    }
    if len > 1 && unsafe { *data } == b'0' {
        return None;
    }
    let mut acc: u64 = 0;
    for i in 0..len {
        let b = unsafe { *data.add(i) };
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc * 10 + (b - b'0') as u64;
    }
    if acc >= u32::MAX as u64 {
        return None;
    }
    Some(acc as i64)
}

/// `<key:number> in <v:any>` — Number-keyed dispatch.
///
/// Returns `true` iff `v` is a NaN-boxed heap pointer to a live Array
/// cell whose `len` covers `key` (`0 ≤ key < len`). Every other
/// runtime layout (non-heap tag, NULL ptr, non-Array tag) collapses
/// to `false` rather than UB.
///
/// # Safety
/// `v` is an unconstrained i64; helper is defensive on every step.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_in_op_any_num(v: i64, key: i64) -> bool {
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return false;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return false;
    }
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    if type_tag == TAG_ARR {
        let len = unsafe { *((ptr as *const u8).add(ARR_LEN_OFF) as *const i64) };
        if !(key >= 0 && key < len) {
            return false;
        }
        // §13.10.1 HasProperty — a hole (elision / deleted index)
        // is absent. Exotic-index header bit gates the probe.
        if unsafe { *((ptr as *const u8).add(6) as *const u16) } & crate::FLAG_ARR_EXOTIC_INDEX != 0
        {
            let key_str = unsafe { __torajs_num_to_string_radix_i(key, 10) };
            if !key_str.is_null() {
                let props =
                    unsafe { *((ptr as *const u8).add(ARR_PROPS_OFF) as *const *const c_void) };
                let hole = !props.is_null()
                    && unsafe { __torajs_dynobj_has(props, key_str) } != 0
                    && unsafe { __torajs_dynobj_entry_is_hole(props, key_str) } != 0;
                unsafe { rc_dec_temp_str(key_str) };
                if hole {
                    return false;
                }
            }
        }
        return true;
    }
    if type_tag == TAG_DYNOBJ {
        // Spec: dynamic property lookup ToString(key) — `0 in obj`
        // when `obj["0"]` exists is true. Alloc the canonical
        // decimal-string of `key` via torajs-num, query dynobj_has,
        // and rc_dec the temporary (refcount=1 from alloc_str → 0
        // after dec, slot freed).
        let key_str = unsafe { __torajs_num_to_string_radix_i(key, 10) };
        if key_str.is_null() {
            return false;
        }
        let r = unsafe { __torajs_dynobj_has(ptr, key_str) };
        unsafe { rc_dec_temp_str(key_str) };
        return r != 0;
    }
    false
}

#[cfg(not(test))]
unsafe fn rc_dec_temp_str(p: *mut u8) {
    unsafe {
        crate::__torajs_rc_dec(p as *mut c_void);
    }
}

#[cfg(test)]
unsafe fn rc_dec_temp_str(_p: *mut u8) {
    // Tests construct mock str blocks on the stack via the
    // `make_str_heap_block` Vec — they're not refcount-managed and
    // the rc_dec path would underflow into the drop dispatch. No-op
    // here mirrors the test stubs above (unbox / dynobj_has).
}

/// `<key:string> in <v:any>` — String-keyed dispatch.
///
/// Returns `true` iff `v` is a NaN-boxed heap pointer to a live
/// DynObj cell that owns the property named by `key`. Defensive on
/// every step.
///
/// # Safety
/// `v` is an unconstrained i64; `key` must be a torajs-str heap block
/// pointer (the same shape ssa_lower already passes to
/// `__torajs_dynobj_has`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_in_op_any_str(v: i64, key: *const u8) -> bool {
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return false;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return false;
    }
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    if type_tag == TAG_DYNOBJ {
        let r = unsafe { __torajs_dynobj_has(ptr, key) };
        return r != 0;
    }
    if type_tag == TAG_ARR {
        if let Some(idx) = unsafe { parse_canonical_array_index(key) } {
            let len = unsafe { *((ptr as *const u8).add(ARR_LEN_OFF) as *const i64) };
            if !(idx >= 0 && idx < len) {
                return false;
            }
            // Hole probe — see the num-keyed arm.
            if unsafe { *((ptr as *const u8).add(6) as *const u16) } & crate::FLAG_ARR_EXOTIC_INDEX
                != 0
            {
                let props =
                    unsafe { *((ptr as *const u8).add(ARR_PROPS_OFF) as *const *const c_void) };
                if !props.is_null()
                    && unsafe { __torajs_dynobj_has(props, key) } != 0
                    && unsafe { __torajs_dynobj_entry_is_hole(props, key) } != 0
                {
                    return false;
                }
            }
            return true;
        }
        // §10.4.2 — `length` is an own property of every array…
        let (bytes, len) = unsafe { str_view(key) };
        if unsafe { core::slice::from_raw_parts(bytes, len) } == b"length" {
            return true;
        }
        // …and a non-index write (`xs.foo = 1`) lands in the side
        // props dynobj, which the arm never looked at: `"foo" in xs`
        // answered false for a property `xs.foo` reads back fine.
        let props = unsafe { *((ptr as *const u8).add(ARR_PROPS_OFF) as *const *const c_void) };
        if !props.is_null() && unsafe { __torajs_dynobj_has(props, key) } != 0 {
            return true;
        }
        // `Array.prototype` is itself an Arr (ES §23.1.3), so its
        // interned family methods have to answer here — `"map" in
        // Array.prototype` is true. Ordinary arrays are not any
        // builtin's prototype and fall out at false.
        return unsafe { __torajs_builtin_proto_own_method_cell(ptr, key as *const c_void) } != 0;
    }
    if type_tag == TAG_OBJ {
        // A static-layout struct owns its declared fields — and, since
        // RFC 20260714-objlit-accessor, its accessor slots too. Pre-fix
        // every `<key> in <struct-through-any>` answered false, even for
        // a plain declared field. Delegating keeps one own-property
        // predicate for `in` / `hasOwnProperty` / `propertyIsEnumerable`
        // instead of a third copy of the layout walk.
        return unsafe { __torajs_any_prop_has(v as u64, key as *const c_void) } != 0;
    }
    false
}

/// `Array.isArray(v)` for a `Type::Any` argument — runtime tag dispatch.
///
/// Compile-time `Array.isArray(x)` resolves statically when `x: Array<T>`
/// (→ true) or when `x` is any other concrete SSA type (→ false). For
/// `x: any` the static answer would always be "false" even when the boxed
/// value at runtime is an Array — wrong per ES §22.1.2.2. This helper
/// peeks the NaN-box: tag must be `ANY_HEAP`, the heap header's
/// `type_tag@+4` must be `TAG_ARR`. Same `instanceof_any` discipline as
/// the rest of this module.
///
/// # Safety
/// `v` is an unconstrained i64; helper is defensive on every step.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_is_arr(v: i64) -> bool {
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return false;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return false;
    }
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    type_tag == TAG_ARR
}

// Cargo-test stubs for the NaN-box / dynobj externs. Real symbols
// live in torajs-anyvalue / torajs-dynobj; unit tests in this crate
// hand-roll a NaN-box + dynobj_has stub so dispatch logic verifies
// in isolation.
#[cfg(test)]
unsafe fn __torajs_anyv_unbox_tag(v: i64) -> i64 {
    ((v as u64 >> 48) & 0xFFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_anyv_unbox_value(v: i64) -> i64 {
    (v as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_dynobj_has(_obj: *const c_void, _key: *const u8) -> i32 {
    // Tests that exercise the DynObj path set this thread-local
    // ahead of the call.
    DYNOBJ_HAS_RESULT.with(|r| r.get())
}

#[cfg(test)]
unsafe fn __torajs_dynobj_entry_is_hole(_obj: *const c_void, _key: *const u8) -> i32 {
    // Tests never construct a hole shadow entry; the exotic-index
    // gate keeps this unreached. Conformance covers the real probe.
    0
}

#[cfg(test)]
unsafe fn __torajs_builtin_proto_own_method_cell(
    _proto: *const c_void,
    _key: *const c_void,
) -> u64 {
    // Only `Array.prototype` (an Arr singleton the runtime mints at
    // link time) ever answers non-zero here; the tag dispatch under
    // test never constructs one. Conformance covers the real probe.
    0
}

#[cfg(test)]
unsafe fn __torajs_any_prop_has(_recv: u64, _key: *const c_void) -> i64 {
    // The struct arm delegates to torajs-anyvalue's own-property
    // predicate, which only exists once the staticlibs are linked at
    // `tr build` time. Tests here cover this module's tag dispatch,
    // not that predicate — same stub discipline as unbox / dynobj_has
    // above. Struct-receiver behaviour is covered end-to-end by the
    // conformance fixtures.
    STRUCT_PROP_HAS_RESULT.with(|r| r.get())
}

#[cfg(test)]
thread_local! {
    static STRUCT_PROP_HAS_RESULT: core::cell::Cell<i64> = const { core::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    static DYNOBJ_HAS_RESULT: core::cell::Cell<i32> = const { core::cell::Cell::new(0) };
}

// Test stub for the cross-crate num→str alloc. Returns a static-
// buffer pointer the dynobj_has stub never deref-reads (it consults
// the thread-local result instead). Pairs with rc_dec_temp_str's
// cfg(test) no-op above so the mock pointer never enters the
// drop-dispatch path.
#[cfg(test)]
static TEST_NUM_TO_STR_BUF: [u8; 16] = [0u8; 16];

#[cfg(test)]
unsafe fn __torajs_num_to_string_radix_i(_n: i64, _radix: i64) -> *mut u8 {
    TEST_NUM_TO_STR_BUF.as_ptr() as *mut u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_arr_heap_block(len: i64) -> Vec<u8> {
        // 16 bytes: 4B refcount + 2B type_tag + 2B flags + 8B len.
        let mut block = vec![0u8; 16];
        block[4..6].copy_from_slice(&TAG_ARR.to_ne_bytes());
        block[ARR_LEN_OFF..ARR_LEN_OFF + 8].copy_from_slice(&len.to_ne_bytes());
        block
    }

    fn make_dynobj_heap_block() -> Vec<u8> {
        // 16 bytes: 4B refcount + 2B type_tag + 2B flags + 8B payload.
        let mut block = vec![0u8; 16];
        block[4..6].copy_from_slice(&TAG_DYNOBJ.to_ne_bytes());
        block
    }

    fn make_other_heap_block(type_tag: u16) -> Vec<u8> {
        let mut block = vec![0u8; 16];
        block[4..6].copy_from_slice(&type_tag.to_ne_bytes());
        block
    }

    fn nan_box(tag: i64, value: i64) -> i64 {
        (tag << 48) | (value & 0x0000_FFFF_FFFF_FFFF)
    }

    #[test]
    fn num_arr_key_in_bounds_returns_true() {
        let block = make_arr_heap_block(3);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(unsafe { __torajs_in_op_any_num(boxed, 0) });
        assert!(unsafe { __torajs_in_op_any_num(boxed, 2) });
    }

    #[test]
    fn num_arr_key_out_of_bounds_returns_false() {
        let block = make_arr_heap_block(3);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 3) });
        assert!(!unsafe { __torajs_in_op_any_num(boxed, -1) });
    }

    #[test]
    fn num_non_arr_tag_returns_false() {
        let block = make_other_heap_block(TAG_DYNOBJ);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 0) });
    }

    #[test]
    fn num_non_heap_tag_returns_false() {
        let boxed = nan_box(2, 0x1234_5678);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 0) });
    }

    #[test]
    fn num_null_ptr_returns_false() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 0) });
    }

    #[test]
    fn str_dynobj_dynobj_has_true_returns_true() {
        DYNOBJ_HAS_RESULT.with(|r| r.set(1));
        let block = make_dynobj_heap_block();
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        let key = b"foo\0".as_ptr();
        assert!(unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn str_dynobj_dynobj_has_false_returns_false() {
        DYNOBJ_HAS_RESULT.with(|r| r.set(0));
        let block = make_dynobj_heap_block();
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        let key = b"foo\0".as_ptr();
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn str_arr_tag_returns_false() {
        let block = make_arr_heap_block(3);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        let key = b"0\0".as_ptr();
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn str_non_heap_tag_returns_false() {
        let boxed = nan_box(2, 0x1234_5678);
        let key = b"foo\0".as_ptr();
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn str_null_ptr_returns_false() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        let key = b"foo\0".as_ptr();
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key) });
    }

    #[test]
    fn is_arr_any_wrapped_array_returns_true() {
        let block = make_arr_heap_block(3);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(unsafe { __torajs_any_is_arr(boxed) });
    }

    #[test]
    fn is_arr_any_wrapped_dynobj_returns_false() {
        let block = make_dynobj_heap_block();
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_any_is_arr(boxed) });
    }

    #[test]
    fn is_arr_non_heap_tag_returns_false() {
        // Int32 boxed
        let boxed = nan_box(2, 0x42);
        assert!(!unsafe { __torajs_any_is_arr(boxed) });
    }

    #[test]
    fn is_arr_null_heap_ptr_returns_false() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        assert!(!unsafe { __torajs_any_is_arr(boxed) });
    }

    fn make_str_heap_block(s: &str) -> Vec<u8> {
        // Plain Str: [hdr 0..8][len u32 @+8][pad @+12..16][data @+16].
        let bytes = s.as_bytes();
        let mut block = vec![0u8; STR_DATA_OFF + bytes.len()];
        // type_tag=0 (Tag::Str), flags=0 (not Substr).
        // refcount slot stays 0 for the test stub.
        let len = bytes.len() as u32;
        block[STR_LEN_OFF..STR_LEN_OFF + 4].copy_from_slice(&len.to_ne_bytes());
        block[STR_DATA_OFF..].copy_from_slice(bytes);
        block
    }

    #[test]
    fn str_arr_canonical_index_in_bounds_true() {
        let arr_block = make_arr_heap_block(3);
        let arr_ptr = arr_block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, arr_ptr);
        let key_block = make_str_heap_block("0");
        assert!(unsafe { __torajs_in_op_any_str(boxed, key_block.as_ptr()) });
        let key_block2 = make_str_heap_block("2");
        assert!(unsafe { __torajs_in_op_any_str(boxed, key_block2.as_ptr()) });
    }

    #[test]
    fn str_arr_canonical_index_out_of_bounds_false() {
        let arr_block = make_arr_heap_block(3);
        let arr_ptr = arr_block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, arr_ptr);
        let key_block = make_str_heap_block("3");
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key_block.as_ptr()) });
    }

    #[test]
    fn str_arr_leading_zero_non_canonical_false() {
        let arr_block = make_arr_heap_block(3);
        let arr_ptr = arr_block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, arr_ptr);
        let key_block = make_str_heap_block("01");
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key_block.as_ptr()) });
    }

    #[test]
    fn str_arr_empty_string_false() {
        let arr_block = make_arr_heap_block(3);
        let arr_ptr = arr_block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, arr_ptr);
        let key_block = make_str_heap_block("");
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key_block.as_ptr()) });
    }

    #[test]
    fn str_arr_non_digit_false() {
        let arr_block = make_arr_heap_block(3);
        let arr_ptr = arr_block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, arr_ptr);
        let key_block = make_str_heap_block("foo");
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key_block.as_ptr()) });
    }

    #[test]
    fn num_dynobj_dynobj_has_true_returns_true() {
        DYNOBJ_HAS_RESULT.with(|r| r.set(1));
        let block = make_dynobj_heap_block();
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(unsafe { __torajs_in_op_any_num(boxed, 0) });
        assert!(unsafe { __torajs_in_op_any_num(boxed, 42) });
    }

    #[test]
    fn num_dynobj_dynobj_has_false_returns_false() {
        DYNOBJ_HAS_RESULT.with(|r| r.set(0));
        let block = make_dynobj_heap_block();
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_in_op_any_num(boxed, 0) });
    }

    #[test]
    fn str_arr_zero_idx_zero_len_false() {
        let arr_block = make_arr_heap_block(0);
        let arr_ptr = arr_block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, arr_ptr);
        let key_block = make_str_heap_block("0");
        assert!(!unsafe { __torajs_in_op_any_str(boxed, key_block.as_ptr()) });
    }
}
