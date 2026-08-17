//! 420-06 (§20.2.3.5) — the class-constructor toString source table.
//!
//! The compiler interns each class declaration's type-erased text as
//! an immortal Str literal and hands it here at register time, keyed
//! by the class tag. The anyvalue toString redispatch resolves a
//! class-ctor dynobj back to its tag by cell identity (the class
//! object IS the registered singleton — one cell per class for the
//! process lifetime) and answers the recorded text; classes with no
//! recorded source (lib/eval-injected — their spans index the wrong
//! source text) miss here and keep the native-form fallback.

use core::ffi::c_void;

use super::{CLASSES_BY_TAG_IMM, MAX_CLASSES, in_range};

// Same shape as PROTOS_BY_TAG_IMM / CLASSES_BY_TAG_IMM: a
// process-lifetime, single-threaded-runtime static table written by
// direct indexing only (rust 2024 `static_mut_refs` posture — see
// classmeta.rs). Values are interned immortal Str cells (rodata
// body, static flag), so no rc traffic in either direction.
static mut CLASS_SOURCE_BY_TAG: [u64; MAX_CLASSES] = [0u64; MAX_CLASSES];

/// Record the erased class-declaration text for `tag`. `src` is an
/// interned immortal Str cell minted by the compiler's
/// string-literal machinery.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_class_source_register(tag: i64, src: *mut c_void) {
    if !in_range(tag) || src.is_null() {
        return;
    }
    // SAFETY: single-threaded JS runtime, direct indexed write.
    unsafe {
        CLASS_SOURCE_BY_TAG[tag as usize] = src as u64;
    }
}

/// The recorded source Str cell for the class whose REGISTERED class
/// object is `cell`, or NULL when the cell is no registered class /
/// no source was recorded. Cell-identity scan — the class table is
/// small and this sits on the cold toString path.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_class_source_for_cell(cell: *const c_void) -> *mut c_void {
    if cell.is_null() {
        return core::ptr::null_mut();
    }
    let mut tag = 0usize;
    while tag < MAX_CLASSES {
        // SAFETY: single-threaded JS runtime, direct indexed reads.
        let registered = unsafe { CLASSES_BY_TAG_IMM[tag] };
        if registered as *const c_void == cell {
            let src = unsafe { CLASS_SOURCE_BY_TAG[tag] };
            return src as *mut c_void;
        }
        tag += 1;
    }
    core::ptr::null_mut()
}
