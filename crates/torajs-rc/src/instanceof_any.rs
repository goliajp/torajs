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
    // SAFETY: per ssa_lower convention, every heap block whose
    // ssa-level type is `Type::Obj(_)` carries a class tag in the
    // i64 slot at +OBJ_CLASS_TAG_OFF. Non-class heap blocks
    // (Str / Arr / dynobj / ...) carry different data there;
    // their stored value won't match any class's registered tag,
    // so the comparison returns `false` and we don't UB.
    let class_tag = unsafe { *((ptr as *const u8).add(OBJ_CLASS_TAG_OFF) as *const i64) };
    class_tag == expected_tag
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
        // 16 bytes: 8B HeapHeader placeholder + 8B class_tag slot.
        let mut block = vec![0u8; 16];
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

    fn _check_c_void_used(_: *const c_void) {}
}
