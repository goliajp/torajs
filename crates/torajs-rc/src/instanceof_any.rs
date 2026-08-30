//! `<v> instanceof <C>` for `Type::Any` operands.
//!
//! ssa_lower's static `instanceof` path (ssa_lower.rs:22472) folds
//! constant-class checks at compile time and emits an inline class-
//! tag OR-chain when the operand is `Type::Obj(_)`. When the operand
//! is `Type::Any` (the common shape — `catch (e: any) { e instanceof
//! TypeError }`), the inline path fast-fails to `false` because it
//! can't statically prove the operand is a class instance.
//!
//! This helper closes that gap: ssa_lower now emits one call per
//! descendant tag with the static `expected_tag` from
//! `class_name_to_tag`, and OR-chains the results. Each call:
//!
//!   1. Unboxes the NaN-box tag — non-`ANY_HEAP` returns `false`.
//!   2. Unboxes the value as `*const c_void` — NULL returns `false`.
//!   3. Reads the class tag at offset 8 (the `OBJ_CLASS_TAG_OFF`
//!      slot ssa_lower itself loads from on the typed Obj path) and
//!      compares to `expected_tag`.
//!
//! The helper is intentionally per-tag rather than accepting an
//! array — ssa_lower already enumerates the descendant set at IR
//! time, so passing each as a `ConstI64` keeps the emit shape
//! parallel to the existing `Type::Obj` OR-chain and avoids a
//! DataGlobal round-trip for the tag list.

use core::ffi::c_void;

use crate::Tag;

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_anyv_unbox_tag(v: i64) -> i64;
    fn __torajs_anyv_unbox_value(v: i64) -> i64;
}

// Offset of the class tag inside a class-instance heap block.
// Matches `OBJ_CLASS_TAG_OFF = 8` in ssa_lower.rs — the i64 slot
// right after the 8-byte universal HeapHeader.
const OBJ_CLASS_TAG_OFF: usize = 8;

// Tag value ssa_lower emits for NaN-boxed heap pointers (mirrors
// `ANY_TAG_HEAP = 4` from torajs-anyvalue / ssa_lower).
const ANY_TAG_HEAP: i64 = 4;

/// `<v> instanceof Object` for `Type::Any` operands — returns true
/// for every NaN-boxed heap object **except** the primitive-wrapper
/// tags `Tag::Str` / `Tag::Symbol` / `Tag::BigInt`, which represent
/// JS primitive values stored on the heap (tr's subset doesn't ship
/// boxed-primitive wrappers, so all string / symbol / bigint cells
/// here come from primitive literals — `"x" instanceof Object` is
/// false per spec).
///
/// Mirrors `__torajs_instanceof_builtin_any_tag` but inverts the
/// per-tag matching shape (any tag accepted, primitive tags
/// rejected) so the ssa-lower Type::Any branch can route `Object`
/// without enumerating every Tag::* the runtime can construct.
///
/// # Safety
/// `v` is an unconstrained i64. Defensive on every step
/// (non-ANY_HEAP / NULL ptr) so unexpected runtime layouts never UB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_instanceof_object_any(v: i64) -> bool {
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return false;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return false;
    }
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    // Tag values mirror `torajs_rc::Tag` (Str=0, Symbol=7, BigInt=10,
    // DynObj=14). Cannot import the enum here without a circular-dep
    // — the values are stable ABI per the assert_eq! suite in lib.rs.
    if matches!(type_tag, 0 | 7 | 10) {
        return false;
    }
    // §7.3.20 OrdinaryHasInstance walks the receiver's prototype
    // chain, and a null-prototype object has none to walk:
    // `Object.create(null) instanceof Object` is false, and so is a
    // module namespace's (§10.4.6.1 answers null). Everything else
    // reaches %Object.prototype% eventually, which is why the blanket
    // `true` was right for every other shape.
    //
    // The tag gate has to come FIRST: bit 6 is disjoint-by-tag (see
    // torajs-rc's flag table) and means the this-free marker on a
    // Closure and the sparse-tail marker on an Arr.
    if type_tag == 14 {
        let flags = unsafe { *((ptr as *const u8).add(6) as *const u16) };
        return flags & DYNOBJ_NULL_PROTO == 0;
    }
    true
}

/// torajs-dynobj's `DYNOBJ_HDR_FLAG_NULL_PROTO` — bit 6 of the heap
/// header's `flags` u16, DynObj-private. Mirrored rather than
/// imported (torajs-rc is below torajs-dynobj); update both sides if
/// the bit ever moves.
const DYNOBJ_NULL_PROTO: u16 = 1 << 6;

/// Built-in `instanceof` for `Type::Any` operands — compares the
/// universal `HeapHeader::type_tag` (u16 at +4) to `expected_type_tag`.
///
/// Mirrors `__torajs_instanceof_class_any_tag` (which compares the
/// per-class `class_tag@+8` slot used by user-declared classes) but
/// targets the built-in heap-cell tags Tag::Arr / Tag::Date /
/// Tag::RegExp / Tag::Promise / Tag::Map / Tag::Set / Tag::WeakMap /
/// Tag::WeakSet — none of those carry a class_tag slot, so the
/// existing user-class helper always returns false for them.
///
/// Returns `true` iff `v` is a NaN-boxed heap pointer (ANY_HEAP) to
/// a live heap cell whose universal type tag equals `expected_type_tag`.
///
/// # Safety
/// `v` is an unconstrained i64. The fn is defensive on every step
/// (non-heap tag / NULL ptr) and only dereferences after both
/// invariants pass, matching the per-class helper's discipline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_instanceof_builtin_any_tag(
    v: i64,
    expected_type_tag: i64,
) -> bool {
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return false;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return false;
    }
    // A builtin's own `.prototype` is never an instance of it:
    // `instanceof` walks the receiver's [[Prototype]] chain looking
    // for the ctor's `.prototype`, and a prototype does not appear
    // on its own chain (its [[Prototype]] is %Object.prototype%).
    // Tag-comparison alone would say `Array.prototype instanceof
    // Array` is true — it is an Arr cell (ES §23.1.3), just not an
    // instance.
    if unsafe { crate::builtin_proto::__torajs_builtin_proto_tag_of(ptr) } >= 0 {
        return false;
    }
    // HeapHeader layout (torajs-rc): `{ refcount: u32@0, type_tag:
    // u16@+4, flags: u16@+6 }`. We read the u16 type_tag and
    // zero-extend to i64 for the comparison.
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) } as i64;
    type_tag == expected_type_tag
}

/// `<v> instanceof <expected_tag>` for `Type::Any` operands.
///
/// Returns `true` iff `v` is a NaN-boxed heap pointer to a class
/// instance whose stored class tag equals `expected_tag`.
///
/// Defensive on every step — bad inputs (non-heap tag, NULL ptr,
/// out-of-range tag) all collapse to `false` rather than UB. This
/// matches ssa_lower's `Type::Obj`-path discipline of never panic-
/// crashing on an unexpected runtime layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_instanceof_class_any_tag(v: i64, expected_tag: i64) -> bool {
    // SAFETY: extern entries are wired to the runtime NaN-box
    // helpers in the final binary; the cfg(test) stub below
    // provides them for cargo-test builds in this crate.
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return false;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    if ptr.is_null() {
        return false;
    }
    // Only a `Tag::Obj` block carries a class tag at +8 — every
    // exotic layout stores its own word there (Arr=len,
    // Promise=state, Map=n_entries), and reading it as a class tag
    // was a live wrong answer whenever the value collided with a
    // registered tag (`[1,2,3] instanceof C` with C's tag == 3).
    // An exotic cell IS a class instance exactly when it was minted
    // through an exotic-subclass factory (RFC 20260730 blade 1) —
    // the header flag gates, the blade-0 side table answers.
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    if type_tag != Tag::Obj as u16 {
        let flags = unsafe { *((ptr as *const u8).add(6) as *const u16) };
        if flags & crate::FLAG_SUBCLASSED != 0 {
            return unsafe { __torajs_subclass_class_tag(ptr) } == expected_tag;
        }
        // 405-01 face 2 — a DynObj receiver (`Object.create(
        // C.prototype)` shapes) carries no class_tag at +8; §7.3.22
        // OrdinaryHasInstance asks whether the class's reified
        // prototype sits on its [[Prototype]] chain instead. The
        // dynamic-target lane already answers this (`o instanceof K`
        // with K a runtime class value); this closes the same gap on
        // the compile-time Ident lane.
        if type_tag == Tag::DynObj as u16 {
            return unsafe { __torajs_instanceof_proto_chain_any_tag(v as u64, expected_tag) };
        }
        return false;
    }
    // The slot is a u32 (every other read point agrees); reading 8
    // bytes would fold the pad after it into the comparison.
    let class_tag = unsafe { *((ptr as *const u8).add(OBJ_CLASS_TAG_OFF) as *const u32) };
    i64::from(class_tag) == expected_tag
}

#[cfg(not(test))]
unsafe extern "C" {
    /// torajs-meta — the blade-0 subclass identity side table (-1 on
    /// miss).
    fn __torajs_subclass_class_tag(cell: *const c_void) -> i64;
    /// torajs-anyvalue — user [[Prototype]]-chain walk against the
    /// class's reified prototype identity (405-01 face 2).
    fn __torajs_instanceof_proto_chain_any_tag(v: u64, expected_tag: i64) -> bool;
}

/// Cargo-test stand-in — no test block sets `FLAG_SUBCLASSED`, so
/// the gated call never fires (mirrors the unbox stubs above).
#[cfg(test)]
unsafe fn __torajs_subclass_class_tag(_cell: *const c_void) -> i64 {
    -1
}

/// Cargo-test stand-in — no test block builds a DynObj cell with a
/// user prototype chain, so the gated call never fires.
#[cfg(test)]
unsafe fn __torajs_instanceof_proto_chain_any_tag(_v: u64, _expected_tag: i64) -> bool {
    false
}

// Cargo-test stubs for the NaN-box unbox externs. The real symbols
// live in torajs-anyvalue (linked into `tr`); unit tests in this
// crate hand-roll the NaN-box pair so we can verify the dispatch
// logic in isolation.
#[cfg(test)]
unsafe fn __torajs_anyv_unbox_tag(v: i64) -> i64 {
    // Mirror the torajs-anyvalue scheme: high 16 bits = tag,
    // low 48 bits = value payload. (This is only the test stub —
    // the real NaN-box scheme is more elaborate; tests just need a
    // consistent inverse for unbox_value below.)
    ((v as u64 >> 48) & 0xFFFF) as i64
}

#[cfg(test)]
unsafe fn __torajs_anyv_unbox_value(v: i64) -> i64 {
    (v as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;

    fn make_test_block(class_tag: i64) -> Vec<u8> {
        // 16 bytes: 8B HeapHeader (type_tag Obj at +4 — only Obj
        // blocks carry a class tag at +8) + 8B class_tag slot.
        let mut block = vec![0u8; 16];
        block[4..6].copy_from_slice(&(Tag::Obj as u16).to_ne_bytes());
        let tag_bytes = class_tag.to_ne_bytes();
        block[OBJ_CLASS_TAG_OFF..OBJ_CLASS_TAG_OFF + 8].copy_from_slice(&tag_bytes);
        block
    }

    fn nan_box(tag: i64, value: i64) -> i64 {
        (tag << 48) | (value & 0x0000_FFFF_FFFF_FFFF)
    }

    #[test]
    fn matching_class_tag_returns_true() {
        let block = make_test_block(42);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(unsafe { __torajs_instanceof_class_any_tag(boxed, 42) });
    }

    #[test]
    fn mismatched_class_tag_returns_false() {
        let block = make_test_block(42);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_instanceof_class_any_tag(boxed, 99) });
    }

    #[test]
    fn non_heap_tag_returns_false() {
        // ANY_TAG_NUMBER (whatever its numeric value) — any tag != 4.
        let boxed = nan_box(2, 0x1234_5678);
        assert!(!unsafe { __torajs_instanceof_class_any_tag(boxed, 42) });
    }

    #[test]
    fn null_ptr_returns_false() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        assert!(!unsafe { __torajs_instanceof_class_any_tag(boxed, 42) });
    }

    #[test]
    fn zero_v_returns_false() {
        assert!(!unsafe { __torajs_instanceof_class_any_tag(0, 42) });
    }

    fn make_test_heap_block(type_tag: u16) -> Vec<u8> {
        // 16 bytes: 4B refcount + 2B type_tag + 2B flags + 8B padding
        // (slot the builtin variant does not read).
        let mut block = vec![0u8; 16];
        let tag_bytes = type_tag.to_ne_bytes();
        block[4..6].copy_from_slice(&tag_bytes);
        block
    }

    #[test]
    fn builtin_matching_type_tag_returns_true() {
        // Tag::Arr = 2 — pretend this heap cell is an Array.
        let block = make_test_heap_block(2);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(unsafe { __torajs_instanceof_builtin_any_tag(boxed, 2) });
    }

    #[test]
    fn builtin_mismatched_type_tag_returns_false() {
        // Heap is Tag::Arr=2 but we ask "instanceof Date" (Tag::Date=5).
        let block = make_test_heap_block(2);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_instanceof_builtin_any_tag(boxed, 5) });
    }

    #[test]
    fn builtin_non_heap_tag_returns_false() {
        let boxed = nan_box(2, 0x1234_5678);
        assert!(!unsafe { __torajs_instanceof_builtin_any_tag(boxed, 2) });
    }

    #[test]
    fn builtin_null_ptr_returns_false() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        assert!(!unsafe { __torajs_instanceof_builtin_any_tag(boxed, 2) });
    }

    #[test]
    fn object_arr_heap_returns_true() {
        // Tag::Arr=2 — Array is an Object.
        let block = make_test_heap_block(2);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(unsafe { __torajs_instanceof_object_any(boxed) });
    }

    #[test]
    fn object_str_heap_returns_false() {
        // Tag::Str=0 — primitive string, NOT an object per spec.
        let block = make_test_heap_block(0);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_instanceof_object_any(boxed) });
    }

    #[test]
    fn object_symbol_heap_returns_false() {
        // Tag::Symbol=7 — primitive Symbol.
        let block = make_test_heap_block(7);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_instanceof_object_any(boxed) });
    }

    #[test]
    fn object_bigint_heap_returns_false() {
        // Tag::BigInt=10 — primitive BigInt.
        let block = make_test_heap_block(10);
        let ptr = block.as_ptr() as i64 & 0x0000_FFFF_FFFF_FFFF;
        let boxed = nan_box(ANY_TAG_HEAP, ptr);
        assert!(!unsafe { __torajs_instanceof_object_any(boxed) });
    }

    #[test]
    fn object_primitive_tag_returns_false() {
        // non-ANY_HEAP tag (e.g. ANY_I64=2 in box scheme) — not an object.
        let boxed = nan_box(2, 0x1234_5678);
        assert!(!unsafe { __torajs_instanceof_object_any(boxed) });
    }

    #[test]
    fn object_null_ptr_returns_false() {
        let boxed = nan_box(ANY_TAG_HEAP, 0);
        assert!(!unsafe { __torajs_instanceof_object_any(boxed) });
    }

    fn _check_c_void_used(_: *const c_void) {}
}
