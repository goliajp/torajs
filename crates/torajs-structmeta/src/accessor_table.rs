//! Accessor resolution over the class layout — the read side of the
//! synthetic `__getter_<p>` / `__setter_<p>` slot spelling.
//!
//! Two representations reach the same property, and both resolve here
//! from the PLAIN name (ES §10.4 keys the own property by `p`, never by
//! the mangled slot):
//!
//! * object-literal accessors live in the layout as a `Closure` FIELD
//!   named `__getter_<p>` ([`__torajs_struct_accessor_find`]);
//! * class accessors are prototype-level, so they live in the
//!   `.__class_methods_<i>` dispatch table under the same spelling
//!   (`method_table::__torajs_struct_accessor_method_find`).
//!
//! Carved out of `lib.rs` when the crate crossed the 500-line file
//! limit (the accessor surface is what pushed it over).
//!
//! The prefixes mirror torajs-core's
//! `check_type_of_object_lit::accessor_slot` — the compile-side source
//! of truth for the spelling.

use crate::StructLayoutEntry;

/// Which half of an accessor pair a lookup wants (RFC
/// 20260714-objlit-accessor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessorKind {
    Getter,
    Setter,
}

impl AccessorKind {
    pub(crate) fn prefix(self) -> &'static [u8] {
        match self {
            AccessorKind::Getter => b"__getter_",
            AccessorKind::Setter => b"__setter_",
        }
    }

    /// Decode the FFI shell's `kind` byte. Anything other than the two
    /// live spellings is not an accessor request.
    pub(crate) fn from_raw(kind: u8) -> Option<Self> {
        match kind {
            0 => Some(AccessorKind::Getter),
            1 => Some(AccessorKind::Setter),
            _ => None,
        }
    }

    /// The kind a mangled slot name spells, or `None` for a plain name.
    /// Splitting a name is the inverse of [`Self::prefix`] — it keeps
    /// the two spellings in one place for the consumers that must
    /// REJECT the mangled form (an `any` member read of
    /// `o.__getter_v` is not a property read).
    pub(crate) fn of_slot_name(name: &[u8]) -> Option<Self> {
        for kind in [AccessorKind::Getter, AccessorKind::Setter] {
            let prefix = kind.prefix();
            if name.len() > prefix.len() && &name[..prefix.len()] == prefix {
                return Some(kind);
            }
        }
        None
    }
}

/// Does `name` (a byte slice of `name_len`) spell an accessor slot?
/// Answers `0` (getter) / `1` (setter) — matching the `kind` byte the
/// find shells take — or `255` for a plain property name.
///
/// The `any`-lane member probes call this to keep the mangled spelling
/// off the user-visible property surface: `o.__getter_v` must read
/// `undefined`, not hand back the getter closure sitting in the slot.
///
/// # Safety
/// `name` must be NULL or point at `name_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_name_kind(name: *const u8, name_len: u32) -> u8 {
    if name.is_null() {
        return 255;
    }
    // SAFETY: caller contract above.
    let bytes = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    match AccessorKind::of_slot_name(bytes) {
        Some(AccessorKind::Getter) => 0,
        Some(AccessorKind::Setter) => 1,
        None => 255,
    }
}

impl StructLayoutEntry {
    /// Find an object-literal accessor SLOT by the property it stands
    /// for: `prop = "v"` matches the field named `__getter_v` (or
    /// `__setter_v`). The layout stores accessors under a synthetic
    /// name, but ES §10.4 keys the own property by the plain name —
    /// the reflection consumers ask by the plain name and this walk
    /// resolves it without allocating the mangled spelling (this crate
    /// is `no_std` and the name may be arbitrarily long).
    pub(crate) fn find_accessor(&self, prop: &[u8], kind: AccessorKind) -> Option<u32> {
        let prefix = kind.prefix();
        let n = self.n_fields();
        let mut i = 0;
        while i < n {
            if let Some(f) = self.field(i)
                && slot_name_matches(f.name_bytes(), prefix, prop)
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// `name == prefix ++ prop`, without materializing the concatenation.
pub(crate) fn slot_name_matches(name: &[u8], prefix: &[u8], prop: &[u8]) -> bool {
    name.len() == prefix.len() + prop.len()
        && &name[..prefix.len()] == prefix
        && &name[prefix.len()..] == prop
}

/// Find the accessor slot standing for property `name`: `kind` 0 asks
/// for the getter (`__getter_<name>`), 1 for the setter. Returns
/// `u32::MAX` when the layout is NULL, the name pointer is NULL, the
/// kind is unknown, or the class has no such accessor.
///
/// # Safety
/// `layout` must be NULL or a pointer returned by
/// [`crate::__torajs_struct_layout_lookup`]; `name` must be NULL or
/// point at `name_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_accessor_find(
    layout: *const StructLayoutEntry,
    name: *const u8,
    name_len: u32,
    kind: u8,
) -> u32 {
    if layout.is_null() || name.is_null() {
        return u32::MAX;
    }
    let Some(kind) = AccessorKind::from_raw(kind) else {
        return u32::MAX;
    };
    // SAFETY: caller contract — `layout` is a live lookup result and
    // `name` points at `name_len` readable bytes.
    let entry = unsafe { &*layout };
    let prop = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    entry.find_accessor(prop, kind).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_name_kind_decodes_both_spellings_and_rejects_plain_names() {
        assert_eq!(
            AccessorKind::of_slot_name(b"__getter_v"),
            Some(AccessorKind::Getter)
        );
        assert_eq!(
            AccessorKind::of_slot_name(b"__setter_v"),
            Some(AccessorKind::Setter)
        );
        assert_eq!(AccessorKind::of_slot_name(b"v"), None);
        // A bare prefix stands for no property — not an accessor slot.
        assert_eq!(AccessorKind::of_slot_name(b"__getter_"), None);
        assert_eq!(AccessorKind::of_slot_name(b"__getter"), None);
    }

    #[test]
    fn name_kind_shell_answers_the_sentinel_for_plain_and_null_names() {
        assert_eq!(
            unsafe { __torajs_accessor_name_kind(b"__getter_v".as_ptr(), 10) },
            0
        );
        assert_eq!(
            unsafe { __torajs_accessor_name_kind(b"__setter_v".as_ptr(), 10) },
            1
        );
        assert_eq!(
            unsafe { __torajs_accessor_name_kind(b"v".as_ptr(), 1) },
            255
        );
        assert_eq!(
            unsafe { __torajs_accessor_name_kind(core::ptr::null(), 0) },
            255
        );
    }
}
