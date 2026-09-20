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

use super::{MAX_CLASSES, in_range};

/// Borrowed `(ptr, len)` over static name bytes — torajs-structmeta's
/// `__torajs_struct_class_name` return shape (`instanceof_generic`
/// keeps the same local mirror).
#[repr(C)]
struct StrSlice {
    ptr: *const u8,
    len: usize,
}

unsafe extern "C" {
    /// torajs-structmeta — the class-name table row for `class_tag`.
    fn __torajs_struct_class_name(class_tag: u32) -> StrSlice;
    /// torajs-fnname — mint `function <name>() { [native code] }`.
    fn __torajs_fn_native_form_str(name_ptr: *const u8, name_len: u32) -> *mut u8;
}

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
    // One evaluation of a unit class shares the template's source.
    match super::intrinsic::tag_of_class_cell(cell) {
        -1 => core::ptr::null_mut(),
        // SAFETY: an in-range tag (the scan / record check bounds it).
        tag => unsafe { CLASS_SOURCE_BY_TAG[tag as usize] as *mut c_void },
    }
}

/// The §20.2.3.5 native form for a registered class that recorded no
/// source — `function Error() { [native code] }`. This is the
/// fallback the module doc above names: an injected class (every
/// builtin constructor read as a value is one) has no span into the
/// user program, and answering an empty string there is not the
/// spec's other branch, it is a missing one.
///
/// NULL when `cell` is no registered class or its tag has no name
/// row, so the caller's existing miss path is unchanged.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_class_native_form_for_cell(cell: *const c_void) -> *mut c_void {
    if cell.is_null() {
        return core::ptr::null_mut();
    }
    let tag = super::intrinsic::tag_of_class_cell(cell);
    if tag < 0 {
        return core::ptr::null_mut();
    }
    // SAFETY: the two kernels read a table row and mint a Str; the
    // slice they hand back points at immortal rodata name bytes.
    unsafe {
        let name = __torajs_struct_class_name(tag as u32);
        if name.ptr.is_null() || name.len == 0 {
            return core::ptr::null_mut();
        }
        __torajs_fn_native_form_str(name.ptr, name.len as u32) as *mut c_void
    }
}
