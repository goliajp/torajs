//! 刀 4 (RFC 20260714-t262-top-clusters) — class-methods dispatch
//! table read side, over the per-class `.__class_methods_<i>` inner
//! globals the link layer bakes next to the field metadata:
//!
//! ```text
//!   MethodMetaArrayHeader { u32 n_methods; u32 _pad }  // 8 bytes
//!   MethodMeta[n_methods] { *name; u32 name_len;       // 32 bytes
//!                           u32 flags; *adapter;        //   each
//!                           *twin }
//! ```
//!
//! Each `adapter` is the `__cm_<C>__<m>` body's boxed dual-entry fn
//! (`(this-as-env, argv, argc) -> AnyValue`); torajs-anyvalue's
//! `struct_method` resolves a name here after the field probe misses
//! and invokes the hit through the uniform boxed ABI with the
//! instance in the env slot. ABI consts mirror
//! `torajs-link/src/user_class_layouts_layout/types.rs` (`METHOD_META_*`
//! / `INNER_METHOD_META_*`), locked by the compile-time block below.

use crate::{AccessorKind, StructLayoutEntry};

/// `OUTER_METHOD_TABLE_PTR_OFFSET_IN_ENTRY`.
const OUTER_METHOD_TABLE_PTR_OFFSET: usize = 24;
/// `INNER_METHOD_META_HEADER_SIZE` — `{ u32 n_methods, u32 _pad }`.
const INNER_METHOD_META_HEADER_SIZE: usize = 8;
/// `INNER_METHOD_META_ELEM_SIZE` — one [`MethodMeta`].
const INNER_METHOD_META_ELEM_SIZE: usize = 32;
/// `METHOD_META_NAME_PTR_OFFSET_IN_ELEM`.
const METHOD_META_NAME_PTR_OFFSET: usize = 0;
/// `METHOD_META_NAME_LEN_OFFSET_IN_ELEM`.
const METHOD_META_NAME_LEN_OFFSET: usize = 8;
/// `METHOD_META_ADAPTER_PTR_OFFSET_IN_ELEM`.
const METHOD_META_ADAPTER_PTR_OFFSET: usize = 16;
/// `METHOD_META_TWIN_PTR_OFFSET_IN_ELEM` (blade 3).
const METHOD_META_TWIN_PTR_OFFSET: usize = 24;

/// Header at the top of every non-empty `.__class_methods_<i>` inner
/// global (mirrors the FieldMeta header shape).
#[repr(C)]
struct MethodMetaArrayHeader {
    n_methods: u32,
    _pad: u32,
}

/// One method-metadata record.
///
/// Layout: `{ *const u8 name; u32 name_len; u32 flags;
/// *const c_void adapter; *const c_void twin }` — 32 bytes,
/// 8-aligned.
#[repr(C)]
pub struct MethodMeta {
    /// UTF-8 method-name bytes (no NUL terminator).
    pub name_ptr: *const u8,
    /// Length of the method name in bytes.
    pub name_len: u32,
    /// S2.38 — flags word (formerly pad; link bakes 0 for older
    /// semantics). Bit 0 = [`METHOD_FLAG_THIS_FREE`].
    pub flags: u32,
    /// The boxed adapter's vaddr (rebased at load time).
    pub adapter: *const core::ffi::c_void,
    /// Blade 3 (RFC 20260804-method-rebind-generic-body) — the
    /// receiver-polymorphic `__cmany_` twin's boxed adapter vaddr;
    /// NULL when the method minted no twin (this-free body, or the
    /// super-route residue).
    pub twin: *const core::ffi::c_void,
}

/// S2.38 — MethodMeta flags bit 0: the `__cm_` body never reads its
/// receiver (compiler-proven at the SSA level), so a bare call may
/// run it with a null receiver per ES §10.2.1.2.
pub const METHOD_FLAG_THIS_FREE: u32 = 1;

/// 404-01 — MethodMeta flags bit 1: the record's ADAPTER is the
/// receiver-polymorphic `__cmany_` twin, whose calling convention is
/// recv-first (the receiver box is prepended in argv[0], the env
/// argument is dropped). Minted for a GENERIC class's rows — the
/// mono body reads fields at one specialization's offsets and would
/// misread another's, while the twin reads through GetV. An env-slot
/// dispatch site must not invoke such a record; [`find_method`]
/// therefore skips it, and only the flags-aware
/// `__torajs_struct_method_find_flags` answers it.
pub const METHOD_FLAG_TWIN_PRIMARY: u32 = 2;

// Compile-time ABI lock against the emit side (same posture as the
// field-meta block in lib.rs).
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    assert!(offset_of!(StructLayoutEntry, method_table_ptr) == OUTER_METHOD_TABLE_PTR_OFFSET);
    assert!(size_of::<MethodMeta>() == INNER_METHOD_META_ELEM_SIZE);
    assert!(align_of::<MethodMeta>() == 8);
    assert!(offset_of!(MethodMeta, name_ptr) == METHOD_META_NAME_PTR_OFFSET);
    assert!(offset_of!(MethodMeta, name_len) == METHOD_META_NAME_LEN_OFFSET);
    assert!(offset_of!(MethodMeta, adapter) == METHOD_META_ADAPTER_PTR_OFFSET);
    assert!(offset_of!(MethodMeta, twin) == METHOD_META_TWIN_PTR_OFFSET);
    assert!(size_of::<MethodMetaArrayHeader>() == INNER_METHOD_META_HEADER_SIZE);
};

impl StructLayoutEntry {
    /// Number of method-metadata records, or `0` when the class has
    /// no dispatch table (`method_table_ptr` is NULL).
    #[inline]
    fn n_methods(&self) -> u32 {
        if self.method_table_ptr.is_null() {
            return 0;
        }
        // SAFETY: a non-NULL `method_table_ptr` points at a
        // `MethodMetaArrayHeader` (8-aligned, 8 bytes) in rodata.
        unsafe { (*(self.method_table_ptr as *const MethodMetaArrayHeader)).n_methods }
    }

    /// The `idx`-th method-metadata record.
    #[inline]
    fn method(&self, idx: u32) -> Option<&'static MethodMeta> {
        if idx >= self.n_methods() {
            return None;
        }
        let body = INNER_METHOD_META_HEADER_SIZE + (idx as usize) * INNER_METHOD_META_ELEM_SIZE;
        // SAFETY: `method_table_ptr` is non-NULL (n_methods() > 0)
        // and `idx < n_methods`, so the element lies within the
        // `.__class_methods_<i>` global.
        let p = unsafe { self.method_table_ptr.add(body) } as *const MethodMeta;
        Some(unsafe { &*p })
    }

    /// Linear scan for a method's record by name. `None` if absent
    /// (same small-count posture as `find_field`).
    #[inline]
    fn find_method_meta(&self, name: &[u8]) -> Option<&'static MethodMeta> {
        let n = self.n_methods();
        let mut i = 0;
        while i < n {
            if let Some(m) = self.method(i) {
                if m.name_bytes() == name {
                    return Some(m);
                }
            }
            i += 1;
        }
        None
    }

    /// Linear scan for a method's boxed adapter by name. A
    /// twin-primary record answers `None` here: its adapter is
    /// recv-first-shaped, and every caller of this finder invokes
    /// through the env slot (see [`METHOD_FLAG_TWIN_PRIMARY`]).
    #[inline]
    fn find_method(&self, name: &[u8]) -> Option<*const core::ffi::c_void> {
        let m = self.find_method_meta(name)?;
        if m.flags & METHOD_FLAG_TWIN_PRIMARY != 0 {
            return None;
        }
        Some(m.adapter)
    }

    /// Linear scan for a CLASS accessor's boxed adapter by the property
    /// it stands for. A class accessor is prototype-level, so unlike an
    /// object-literal one it has no layout field to sit in — the
    /// dispatch table carries it under the same `__getter_<p>` /
    /// `__setter_<p>` spelling, and the plain-name walk here mirrors
    /// `find_accessor`'s (no allocation, `no_std`).
    #[inline]
    fn find_accessor_method(
        &self,
        prop: &[u8],
        kind: AccessorKind,
    ) -> Option<*const core::ffi::c_void> {
        self.find_accessor_method_meta(prop, kind)
            .map(|m| m.adapter)
    }

    /// The record behind [`find_accessor_method`] — the flags-aware
    /// finder reads it so a twin-primary row (RFC 20260815 刀 5 — a
    /// GENERIC class's accessor rides its `__cmany_` twin, recv-first
    /// calling convention) can be invoked with the right shape. The
    /// presence probes keep using the adapter finder: a twin-primary
    /// row still makes the property present.
    #[inline]
    fn find_accessor_method_meta(
        &self,
        prop: &[u8],
        kind: AccessorKind,
    ) -> Option<&'static MethodMeta> {
        let prefix = kind.prefix();
        let n = self.n_methods();
        let mut i = 0;
        while i < n {
            if let Some(m) = self.method(i)
                && crate::accessor_table::slot_name_matches(m.name_bytes(), prefix, prop)
            {
                return Some(m);
            }
            i += 1;
        }
        None
    }
}

impl MethodMeta {
    /// The method name as a byte slice (mirrors `FieldMeta::name_bytes`).
    #[inline]
    fn name_bytes(&self) -> &'static [u8] {
        if self.name_ptr.is_null() || self.name_len == 0 {
            return &[];
        }
        // SAFETY: a non-NULL `name_ptr` points at `name_len` UTF-8
        // bytes in the `.__class_method_name_<i>_<j>` rodata.
        unsafe { core::slice::from_raw_parts(self.name_ptr, self.name_len as usize) }
    }
}

/// Resolve a class method's boxed dual-entry adapter by name. Answers
/// NULL when the layout is NULL, the class has no dispatch table, or
/// no method matches — the caller (torajs-anyvalue `struct_method`)
/// keeps its honest no-such-method TypeError on a miss.
///
/// # Safety
/// `layout` must be NULL or a live result of
/// `__torajs_struct_layout_lookup`; `name` must be NULL or point at
/// `name_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_method_find(
    layout: *const StructLayoutEntry,
    name: *const u8,
    name_len: u32,
) -> *const core::ffi::c_void {
    if layout.is_null() || name.is_null() {
        return core::ptr::null();
    }
    // SAFETY: caller contract above.
    let entry = unsafe { &*layout };
    let needle = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    entry.find_method(needle).unwrap_or(core::ptr::null())
}

/// The flags-aware variant of [`__torajs_struct_method_find`] — the
/// ONE finder that also answers twin-primary records. Writes the
/// record's flags word through `out_flags` on a hit (untouched on a
/// miss); the caller reads [`METHOD_FLAG_TWIN_PRIMARY`] to pick the
/// recv-first calling convention.
///
/// # Safety
/// `layout` / `name` as in [`__torajs_struct_method_find`];
/// `out_flags` must be NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_method_find_flags(
    layout: *const StructLayoutEntry,
    name: *const u8,
    name_len: u32,
    out_flags: *mut u32,
) -> *const core::ffi::c_void {
    if layout.is_null() || name.is_null() {
        return core::ptr::null();
    }
    // SAFETY: caller contract above.
    let entry = unsafe { &*layout };
    let needle = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    match entry.find_method_meta(needle) {
        Some(m) => {
            if !out_flags.is_null() {
                // SAFETY: caller contract above.
                unsafe { out_flags.write(m.flags) };
            }
            m.adapter
        }
        None => core::ptr::null(),
    }
}

/// Resolve a CLASS accessor's boxed adapter by the property it stands
/// for — `kind` 0 asks for the getter, 1 for the setter (the byte the
/// `__torajs_struct_accessor_find` shell takes). NULL when the layout
/// is NULL, the kind is unknown, or the class declares no such
/// accessor.
///
/// The adapter is the `__cm_<C>__<p>_get` body's boxed dual entry, so
/// the caller invokes it with the instance in the env slot and an
/// EMPTY argv (the `__this` param is the env) — the same shape
/// `struct_method` uses for a plain method.
///
/// # Safety
/// `layout` must be NULL or a live result of
/// `__torajs_struct_layout_lookup`; `name` must be NULL or point at
/// `name_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_accessor_method_find(
    layout: *const StructLayoutEntry,
    name: *const u8,
    name_len: u32,
    kind: u8,
) -> *const core::ffi::c_void {
    if layout.is_null() || name.is_null() {
        return core::ptr::null();
    }
    let Some(kind) = AccessorKind::from_raw(kind) else {
        return core::ptr::null();
    };
    // SAFETY: caller contract above.
    let entry = unsafe { &*layout };
    let prop = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    entry
        .find_accessor_method(prop, kind)
        .unwrap_or(core::ptr::null())
}

/// The flags-aware variant of [`__torajs_struct_accessor_method_find`]
/// — mirror of [`__torajs_struct_method_find_flags`]. Writes the
/// record's flags word through `out_flags` on a hit (untouched on a
/// miss); the caller reads [`METHOD_FLAG_TWIN_PRIMARY`] to pick the
/// recv-first calling convention (RFC 20260815 刀 5 — a GENERIC
/// class's accessor row is its `__cmany_` twin).
///
/// # Safety
/// `layout` / `name` as in [`__torajs_struct_accessor_method_find`];
/// `out_flags` must be NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_accessor_method_find_flags(
    layout: *const StructLayoutEntry,
    name: *const u8,
    name_len: u32,
    kind: u8,
    out_flags: *mut u32,
) -> *const core::ffi::c_void {
    if layout.is_null() || name.is_null() {
        return core::ptr::null();
    }
    let Some(kind) = AccessorKind::from_raw(kind) else {
        return core::ptr::null();
    };
    // SAFETY: caller contract above.
    let entry = unsafe { &*layout };
    let prop = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    match entry.find_accessor_method_meta(prop, kind) {
        Some(m) => {
            if !out_flags.is_null() {
                // SAFETY: caller contract above.
                unsafe { out_flags.write(m.flags) };
            }
            m.adapter
        }
        None => core::ptr::null(),
    }
}

/// Enumerate side — the record count for the register-time method
/// reification walk (`__torajs_anyv_class_register`, RFC
/// 20260717-class-first-class-value knife B). NULL layout answers 0.
///
/// # Safety
/// `layout` must be NULL or a live result of
/// `__torajs_struct_layout_lookup`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_method_count(layout: *const StructLayoutEntry) -> u32 {
    if layout.is_null() {
        return 0;
    }
    // SAFETY: caller contract above.
    unsafe { &*layout }.n_methods()
}

/// The `idx`-th record's name span + adapter — the enumerate pair of
/// [`__torajs_struct_method_count`]. Returns NULL past the end;
/// `out_name` / `out_len` are written only on a hit.
///
/// # Safety
/// `layout` as above; `out_name` / `out_len` point at writable slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_method_at(
    layout: *const StructLayoutEntry,
    idx: u32,
    out_name: *mut *const u8,
    out_len: *mut u32,
) -> *const core::ffi::c_void {
    if layout.is_null() {
        return core::ptr::null();
    }
    // SAFETY: caller contract above.
    let Some(m) = (unsafe { &*layout }).method(idx) else {
        return core::ptr::null();
    };
    // SAFETY: caller passes writable out-slots.
    unsafe {
        *out_name = m.name_ptr;
        *out_len = m.name_len;
    }
    m.adapter
}

/// The `idx`-th record's name span alone — the inspect walker's
/// enumerator (r502, RFC 20260824-s2-5 刀 4 A8). Answers 1 on a hit
/// (`out_name` / `out_len` written), 0 past the end or for a NULL
/// layout. It deliberately does NOT hand out the adapter: the link
/// bakes the adapter column only while a finder that INVOKES it is
/// live, and a printer that read the slot for a null check would
/// have kept every class program's any world alive.
///
/// # Safety
/// `layout` as above; `out_name` / `out_len` point at writable slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_method_name_at(
    layout: *const StructLayoutEntry,
    idx: u32,
    out_name: *mut *const u8,
    out_len: *mut u32,
) -> u32 {
    if layout.is_null() {
        return 0;
    }
    // SAFETY: caller contract above.
    let Some(m) = (unsafe { &*layout }).method(idx) else {
        return 0;
    };
    // SAFETY: caller passes writable out-slots.
    unsafe {
        *out_name = m.name_ptr;
        *out_len = m.name_len;
    }
    1
}

/// The `idx`-th record's `__cmany_` twin adapter vaddr (blade 3) —
/// NULL when the method minted no twin, for a NULL layout, or an
/// out-of-range index.
///
/// # Safety
/// `layout` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_method_twin_at(
    layout: *const StructLayoutEntry,
    idx: u32,
) -> *const core::ffi::c_void {
    if layout.is_null() {
        return core::ptr::null();
    }
    // SAFETY: caller contract above.
    match (unsafe { &*layout }).method(idx) {
        Some(m) => m.twin,
        None => core::ptr::null(),
    }
}

/// The `idx`-th record's flags word (S2.38 — bit 0 =
/// [`METHOD_FLAG_THIS_FREE`]); `0` for a NULL layout or an
/// out-of-range index.
///
/// # Safety
/// `layout` as above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_method_flags_at(
    layout: *const StructLayoutEntry,
    idx: u32,
) -> u32 {
    if layout.is_null() {
        return 0;
    }
    // SAFETY: caller contract above.
    match (unsafe { &*layout }).method(idx) {
        Some(m) => m.flags,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A hand-built `.__class_methods_<i>` image: 8-byte header
    // (n_methods = 2) + two 24-byte MethodMeta records — walked
    // exactly like the link-emitted global.
    #[repr(C)]
    struct TwoMethodImage {
        n_methods: u32,
        _pad: u32,
        methods: [MethodMeta; 2],
    }

    const NAME_NEXT: &[u8] = b"next";
    const NAME_ADD: &[u8] = b"add";
    const NAME_GETTER_B: &[u8] = b"__getter_b";
    const NAME_SETTER_B: &[u8] = b"__setter_b";
    const ADAPTER_A: *const core::ffi::c_void = 0xA000usize as *const _;
    const ADAPTER_B: *const core::ffi::c_void = 0xB000usize as *const _;

    fn two_method_entry(image: &TwoMethodImage) -> StructLayoutEntry {
        StructLayoutEntry {
            n_children: 0,
            flags: 0,
            child_offsets: core::ptr::null(),
            field_metadata_ptr: core::ptr::null(),
            method_table_ptr: image as *const TwoMethodImage as *const u8,
        }
    }

    fn image() -> TwoMethodImage {
        TwoMethodImage {
            n_methods: 2,
            _pad: 0,
            methods: [
                MethodMeta {
                    name_ptr: NAME_NEXT.as_ptr(),
                    name_len: NAME_NEXT.len() as u32,
                    flags: 0,
                    adapter: ADAPTER_A,
                    twin: core::ptr::null(),
                },
                MethodMeta {
                    name_ptr: NAME_ADD.as_ptr(),
                    name_len: NAME_ADD.len() as u32,
                    flags: 0,
                    adapter: ADAPTER_B,
                    twin: core::ptr::null(),
                },
            ],
        }
    }

    #[test]
    fn find_hits_both_and_misses_absent() {
        let img = image();
        let e = two_method_entry(&img);
        let p = &e as *const StructLayoutEntry;
        unsafe {
            assert_eq!(
                __torajs_struct_method_find(p, NAME_NEXT.as_ptr(), 4),
                ADAPTER_A
            );
            assert_eq!(
                __torajs_struct_method_find(p, NAME_ADD.as_ptr(), 3),
                ADAPTER_B
            );
            assert!(__torajs_struct_method_find(p, b"nosuch".as_ptr(), 6).is_null());
        }
    }

    // A class-accessor table: the getter/setter adapters sit under the
    // synthetic slot spelling, exactly as the emit side registers them.
    fn accessor_image() -> TwoMethodImage {
        TwoMethodImage {
            n_methods: 2,
            _pad: 0,
            methods: [
                MethodMeta {
                    name_ptr: NAME_GETTER_B.as_ptr(),
                    name_len: NAME_GETTER_B.len() as u32,
                    flags: 0,
                    adapter: ADAPTER_A,
                    twin: core::ptr::null(),
                },
                MethodMeta {
                    name_ptr: NAME_SETTER_B.as_ptr(),
                    name_len: NAME_SETTER_B.len() as u32,
                    flags: 0,
                    adapter: ADAPTER_B,
                    twin: core::ptr::null(),
                },
            ],
        }
    }

    #[test]
    fn accessor_method_find_resolves_both_halves_from_the_plain_name() {
        let img = accessor_image();
        let e = two_method_entry(&img);
        let p = &e as *const StructLayoutEntry;
        unsafe {
            assert_eq!(
                __torajs_struct_accessor_method_find(p, b"b".as_ptr(), 1, 0),
                ADAPTER_A
            );
            assert_eq!(
                __torajs_struct_accessor_method_find(p, b"b".as_ptr(), 1, 1),
                ADAPTER_B
            );
            // Unknown kind byte is not an accessor request.
            assert!(__torajs_struct_accessor_method_find(p, b"b".as_ptr(), 1, 7).is_null());
            // The property is `b` — asking with the slot's own spelling
            // must miss, or the internal name leaks back in as a key.
            assert!(
                __torajs_struct_accessor_method_find(p, NAME_GETTER_B.as_ptr(), 10, 0).is_null()
            );
        }
    }

    #[test]
    fn plain_method_find_does_not_see_accessor_entries_under_the_plain_name() {
        // The method-call probe must miss `b` — a getter is not a
        // callable method, and `o.b()` on an accessor property has to
        // keep its honest TypeError.
        let img = accessor_image();
        let e = two_method_entry(&img);
        let p = &e as *const StructLayoutEntry;
        unsafe {
            assert!(__torajs_struct_method_find(p, b"b".as_ptr(), 1).is_null());
        }
    }

    #[test]
    fn null_table_and_null_args_answer_null() {
        let e = StructLayoutEntry {
            n_children: 0,
            flags: 0,
            child_offsets: core::ptr::null(),
            field_metadata_ptr: core::ptr::null(),
            method_table_ptr: core::ptr::null(),
        };
        let p = &e as *const StructLayoutEntry;
        unsafe {
            assert!(__torajs_struct_method_find(p, NAME_NEXT.as_ptr(), 4).is_null());
            assert!(
                __torajs_struct_method_find(core::ptr::null(), NAME_NEXT.as_ptr(), 4).is_null()
            );
            assert!(__torajs_struct_method_find(p, core::ptr::null(), 0).is_null());
        }
    }
}
