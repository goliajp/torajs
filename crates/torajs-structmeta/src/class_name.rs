//! W-J Phase A3c — class-name registry reader.
//!
//! `__torajs_class_name_table[]` rodata: N × 24-byte entries sorted by
//! `class_tag` ascending, plus `__torajs_n_class_names: u64` count.
//! Emit-side spec: `crates/torajs-link/src/class_name_table_layout.rs`.

use crate::StrSlice;

/// One row of the `__torajs_class_name_table[]` rodata array. ABI
/// locked to `crates/torajs-link/src/class_name_table_layout.rs`
/// constants via the `const _: ()` block below.
#[repr(C)]
pub struct ClassNameTableEntry {
    pub class_tag: u32,
    pub _pad: u32,
    pub name_ptr: *const u8,
    pub name_len: u32,
    pub _pad2: u32,
}

const CLASS_NAME_ENTRY_SIZE: usize = 24;

// SAFETY: ClassNameTableEntry lives in rodata (link layer emits it
// chain-fixup-resolved), and the test stand-in is an empty stub. No
// code mutates through `name_ptr`.
unsafe impl Sync for ClassNameTableEntry {}

const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    assert!(size_of::<ClassNameTableEntry>() == CLASS_NAME_ENTRY_SIZE);
    assert!(align_of::<ClassNameTableEntry>() == 8);
    assert!(offset_of!(ClassNameTableEntry, class_tag) == 0);
    assert!(offset_of!(ClassNameTableEntry, name_ptr) == 8);
    assert!(offset_of!(ClassNameTableEntry, name_len) == 16);
};

#[cfg(not(test))]
unsafe extern "C" {
    static __torajs_class_name_table: ClassNameTableEntry;
    static __torajs_n_class_names: u64;
}

#[cfg(test)]
#[unsafe(no_mangle)]
static __torajs_n_class_names: u64 = 0;
#[cfg(test)]
#[unsafe(no_mangle)]
static __torajs_class_name_table: ClassNameTableEntry = ClassNameTableEntry {
    class_tag: 0,
    _pad: 0,
    name_ptr: core::ptr::null(),
    name_len: 0,
    _pad2: 0,
};

#[inline]
fn class_name_table() -> (*const ClassNameTableEntry, u64) {
    let table: *const ClassNameTableEntry = &raw const __torajs_class_name_table;
    // SAFETY: count global is a plain `u64` in both build paths.
    let n = unsafe { *(&raw const __torajs_n_class_names) };
    (table, n)
}

/// Binary-search the class-name table by `class_tag`. `None` for
/// `class_tag == 0` (anonymous / non-stamped) or missing entry.
fn lookup_class_name(class_tag: u32) -> Option<&'static ClassNameTableEntry> {
    if class_tag == 0 {
        return None;
    }
    let (table, n) = class_name_table();
    if n == 0 {
        return None;
    }
    let mut lo: usize = 0;
    let mut hi: usize = n as usize;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        // SAFETY: `mid < n`, `table` is a rodata array of `n` entries.
        let entry = unsafe { &*table.add(mid) };
        if entry.class_tag == class_tag {
            return Some(entry);
        } else if entry.class_tag < class_tag {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    None
}

/// Read the source-text class name for a `class_tag` as a `(ptr, len)`
/// slice. Returns an empty slice (`ptr == NULL`, `len == 0`) for
/// `class_tag == 0` or a tag the link layer never registered.
///
/// # Safety
/// Reads the link-emitted `__torajs_class_name_table` rodata; the
/// returned pointer borrows rodata and must not be mutated through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_class_name(class_tag: u32) -> StrSlice {
    match lookup_class_name(class_tag) {
        Some(entry) => StrSlice {
            ptr: entry.name_ptr,
            len: entry.name_len as usize,
        },
        None => StrSlice::EMPTY,
    }
}
