//! Bridge to torajs-str's allocator — extern decl only.
//!
//! torajs-num is a Layer-2 sibling of torajs-str (per architecture-
//! rewrite.md DAG: Layer-N → Layer-(N-1) only, same-layer cross
//! deps forbidden), so we declare the alloc fn as a C-ABI extern
//! instead of adding torajs-str as a Cargo dep. At `tr build` /
//! `tr run` link time the symbol resolves against libtorajs_str.a;
//! for `cargo test` we provide a `#[cfg(test)]` stub in [`crate`]
//! (see `lib.rs`) so the test binary still links.
//!
//! Why this works architecturally: the constraint is on the Rust
//! TYPE-SYSTEM dependency (Cargo dep tree), not on the FFI ABI.
//! Many same-layer pairs (anyvalue → str's to_number, regex → str's
//! slice/concat) follow the same pattern.

use crate::layout::STR_DATA_OFF;

unsafe extern "C" {
    /// `__torajs_str_alloc_pooled(len) -> *mut u8` — provided by
    /// libtorajs_str.a at link time.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

/// Allocate a fresh Str heap block and copy `payload` into it.
/// Returns the raw pointer for FFI handoff. Single hot path used by
/// every Number→Str format fn (toString radix / toFixed / toExp /
/// toPrecision).
#[inline]
pub fn alloc_str(payload: &[u8]) -> *mut u8 {
    let p = unsafe { __torajs_str_alloc_pooled(payload.len() as u64) };
    if !payload.is_empty() {
        let dst = unsafe { core::slice::from_raw_parts_mut(p.add(STR_DATA_OFF), payload.len()) };
        dst.copy_from_slice(payload);
    }
    p
}
