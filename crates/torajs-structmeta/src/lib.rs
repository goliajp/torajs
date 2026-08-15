//! Struct-layout reflection substrate — read-side helpers over the
//! toolchain-emitted `__torajs_class_layouts` table (W-J Phase A4,
//! RFC 20260614-w-j-struct-reflect §3 A4).
//!
//! ## What this reads
//!
//! `torajs-link` emits, into every `tr build` user binary:
//!
//! - `__torajs_class_layouts`: an array of [`StructLayoutEntry`]
//!   (24 bytes each), one per declared class + anonymous struct shape.
//! - `__torajs_n_class_layouts`: a `u32` table length.
//!
//! Each instance carries its `class_tag` as a `u32` at `+8` (right
//! after the 8-byte universal heap header). `class_tag == 0` means
//! "anonymous struct interned too late to get a layout" (graceful
//! degradation — reflection sees it as having no metadata, never a
//! crash). Otherwise the layout is `class_layouts[class_tag - 1]`.
//!
//! W-J Phase A3b populated each entry's `field_metadata_ptr` slot with
//! the vaddr of a per-class inner global (`.__class_fields_<i>`):
//!
//! ```text
//!   FieldMetaArrayHeader { u32 n_fields; u32 _pad }   // 8 bytes
//!   FieldMeta[n_fields] { *name; u32 name_len;        // 24 bytes
//!                         u32 field_byte_offset;       //   each
//!                         u8 type_tag; <7 pad> }
//! ```
//!
//! The cycle collector (`torajs-cycle`) already reads `n_children` +
//! `child_offsets` from the same table; this crate adds the field
//! name / offset / type-tag read path the reflection ops need.
//!
//! ## ABI lock
//!
//! Every `#[repr(C)]` struct below mirrors
//! `torajs-link/src/user_class_layouts_layout/types.rs` byte for byte.
//! The `#[cfg(test)]` `abi_*` tests assert `size_of` / `align_of` /
//! `offset_of` against the same numeric constants the link layer emits,
//! so a drift on either side trips a unit-test failure rather than
//! silently mis-reading the on-disk bytes.
//!
//! ## Two-layer API
//!
//! Per the C→Rust port playbook: an idiomatic inner layer (typed
//! references, `Option`, slices) does the real work, and a thin
//! `extern "C"` shell does only the raw↔safe boundary conversion so
//! the reflection consumers (Phase B+) can call across the FFI.

// ---------------------------------------------------------------------------
// ABI constants — mirror `user_class_layouts_layout/types.rs`. Kept as
// named consts (not inline literals) so the `abi_*` tests read as a
// cross-reference table against the link layer's emit-side consts.
// ---------------------------------------------------------------------------

/// `OUTER_ENTRY_SIZE` — on-disk size of one [`StructLayoutEntry`]
/// (刀 4 extended 24 → 32 with the `method_table_ptr` slot).
const OUTER_ENTRY_SIZE: usize = 32;
/// `OUTER_CHILD_OFFSETS_PTR_OFFSET_IN_ENTRY`.
const OUTER_CHILD_OFFSETS_PTR_OFFSET: usize = 8;
/// `OUTER_FIELD_META_PTR_OFFSET_IN_ENTRY`.
const OUTER_FIELD_META_PTR_OFFSET: usize = 16;
/// `INNER_FIELD_META_HEADER_SIZE` — `{ u32 n_fields, u32 _pad }`.
const INNER_FIELD_META_HEADER_SIZE: usize = 8;
/// `INNER_FIELD_META_ELEM_SIZE` — one [`FieldMeta`].
const INNER_FIELD_META_ELEM_SIZE: usize = 24;
/// `FIELD_META_NAME_PTR_OFFSET_IN_ELEM`.
const FIELD_META_NAME_PTR_OFFSET: usize = 0;
/// `FIELD_META_NAME_LEN_OFFSET_IN_ELEM`.
const FIELD_META_NAME_LEN_OFFSET: usize = 8;
/// `FIELD_META_FIELD_OFFSET_OFFSET_IN_ELEM`.
const FIELD_META_FIELD_OFFSET_OFFSET: usize = 12;
/// `FIELD_META_TYPE_TAG_OFFSET_IN_ELEM`.
const FIELD_META_TYPE_TAG_OFFSET: usize = 16;

// ---------------------------------------------------------------------------
// On-disk struct mirrors
// ---------------------------------------------------------------------------

mod accessor_table;
mod class_name;
mod method_table;
pub use accessor_table::{
    __torajs_accessor_name_kind, __torajs_struct_accessor_find, AccessorKind,
};
pub use class_name::{__torajs_struct_class_name, ClassNameTableEntry};
pub use method_table::{
    __torajs_struct_accessor_method_find, __torajs_struct_method_find, METHOD_FLAG_THIS_FREE,
    MethodMeta,
};

/// One outer-table entry — the same 24-byte record `torajs-cycle`'s
/// `ClassLayout` mirrors. `n_children` + `child_offsets` belong to the
/// cycle collector; this crate reads `field_metadata_ptr`.
///
/// Layout: `{ u32 n_children; u32 flags; *const u32 child_offsets;
/// *const u8 field_metadata_ptr }` — 24 bytes, 8-aligned (LLVM struct
/// rule for `{ i32, i32, ptr, ptr }`). L3b #4 turned the former
/// implicit pad at +4 into a flags word (bit 0 = named class); this
/// crate doesn't read it, but the field is declared so the mirror
/// stays byte-exact with the emit side.
#[repr(C)]
pub struct StructLayoutEntry {
    /// Refcounted-pointer field count (cycle collector consumer).
    pub n_children: u32,
    /// L3b #4 — bit 0 = declared class (vs anonymous struct shape).
    /// Consumers: runtime Obj drop's cycle-root gate (torajs-cycle).
    pub flags: u32,
    /// Byte offsets of refcounted-pointer fields (cycle collector
    /// consumer). `NULL` when the class has none.
    pub child_offsets: *const u32,
    /// Base of the per-class `.__class_fields_<i>` inner global
    /// (header + body). `NULL` when the class has no field metadata.
    pub field_metadata_ptr: *const u8,
    /// 刀 4 (RFC 20260714-t262-top-clusters) — base of the per-class
    /// `.__class_methods_<i>` inner global (header + body). `NULL`
    /// when the class has no runtime-dispatchable methods.
    pub method_table_ptr: *const u8,
}

// SAFETY: `StructLayoutEntry` carries raw pointers so the auto `Sync`
// check fails. In the `tr build` path the table lives in the binary's
// read-only data segment (immutable, single producer at link time) and
// the reflection helpers only ever read through it; at cargo-test time
// the extern stand-ins are empty stubs. No code mutates through these
// pointers.
unsafe impl Sync for StructLayoutEntry {}

/// Header at the top of every non-empty `.__class_fields_<i>` inner
/// global: the field count, then 4 bytes of padding so the first
/// [`FieldMeta`] body element lands 8-aligned.
#[repr(C)]
struct FieldMetaArrayHeader {
    n_fields: u32,
    _pad: u32,
}

/// One field-metadata record. Trailing padding (bytes 17..24) is the
/// implicit `repr(C)` alignment pad — present on disk, never read.
///
/// Layout: `{ *const u8 name; u32 name_len; u32 field_byte_offset;
/// u8 type_tag; <7 pad> }` — 24 bytes, 8-aligned.
#[repr(C)]
pub struct FieldMeta {
    /// UTF-8 field-name bytes (no NUL terminator). Rebased at load
    /// time to the `.__class_field_name_<i>_<j>` rodata.
    pub name_ptr: *const u8,
    /// Length of the field name in bytes.
    pub name_len: u32,
    /// Byte offset of the field within the instance
    /// (`OBJ_HEADER_SIZE + idx * 8`).
    pub field_byte_offset: u32,
    /// Coarse 8-bit `Type` discriminator (`ssa::field_type_tag_of`).
    pub type_tag: u8,
}

/// `(ptr, len)` pair returned by [`__torajs_struct_field_name`]. An
/// empty slice (`ptr == NULL`, `len == 0`) signals an out-of-range
/// index or a class with no field metadata.
#[repr(C)]
pub struct StrSlice {
    pub ptr: *const u8,
    pub len: usize,
}

impl StrSlice {
    const EMPTY: StrSlice = StrSlice {
        ptr: core::ptr::null(),
        len: 0,
    };
}

/// Field byte-offset + type-tag returned by
/// [`__torajs_struct_field_info`]. A zeroed value (`field_byte_offset
/// == 0`) signals an out-of-range index — a real field offset is
/// always `>= OBJ_HEADER_SIZE` (32), never 0.
#[repr(C)]
pub struct FieldInfo {
    pub field_byte_offset: u32,
    pub type_tag: u8,
}

impl FieldInfo {
    const ZEROED: FieldInfo = FieldInfo {
        field_byte_offset: 0,
        type_tag: 0,
    };
}

// Compile-time ABI lock against `user_class_layouts_layout/types.rs`.
// `size_of` / `align_of` / `offset_of` are const-evaluable, so a drift
// on either the emit (link layer) or read (this crate) side fails the
// build outright — stronger than a runtime assert. Each line names the
// matching emit-side const so the two files stay in lockstep by review.
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    // OUTER_ENTRY_SIZE / OUTER_ENTRY_ALIGN.
    assert!(size_of::<StructLayoutEntry>() == OUTER_ENTRY_SIZE);
    assert!(align_of::<StructLayoutEntry>() == 8);
    // OUTER_CHILD_OFFSETS_PTR_OFFSET_IN_ENTRY / OUTER_FIELD_META_PTR_OFFSET_IN_ENTRY.
    assert!(offset_of!(StructLayoutEntry, child_offsets) == OUTER_CHILD_OFFSETS_PTR_OFFSET);
    assert!(offset_of!(StructLayoutEntry, field_metadata_ptr) == OUTER_FIELD_META_PTR_OFFSET);
    // INNER_FIELD_META_ELEM_SIZE + FIELD_META_*_OFFSET_IN_ELEM.
    assert!(size_of::<FieldMeta>() == INNER_FIELD_META_ELEM_SIZE);
    assert!(align_of::<FieldMeta>() == 8);
    assert!(offset_of!(FieldMeta, name_ptr) == FIELD_META_NAME_PTR_OFFSET);
    assert!(offset_of!(FieldMeta, name_len) == FIELD_META_NAME_LEN_OFFSET);
    assert!(offset_of!(FieldMeta, field_byte_offset) == FIELD_META_FIELD_OFFSET_OFFSET);
    assert!(offset_of!(FieldMeta, type_tag) == FIELD_META_TYPE_TAG_OFFSET);
    // INNER_FIELD_META_HEADER_SIZE.
    assert!(size_of::<FieldMetaArrayHeader>() == INNER_FIELD_META_HEADER_SIZE);
    assert!(offset_of!(FieldMetaArrayHeader, n_fields) == 0);
};

// ---------------------------------------------------------------------------
// Global table — emitted by torajs-link at `tr build`, stubbed at test
// (mirrors `torajs-cycle/src/layout.rs`).
// ---------------------------------------------------------------------------

#[cfg(not(test))]
unsafe extern "C" {
    static __torajs_class_layouts: StructLayoutEntry;
    static __torajs_n_class_layouts: u32;
}

#[cfg(test)]
#[unsafe(no_mangle)]
static __torajs_n_class_layouts: u32 = 0;
#[cfg(test)]
#[unsafe(no_mangle)]
static __torajs_class_layouts: StructLayoutEntry = StructLayoutEntry {
    n_children: 0,
    flags: 0,
    child_offsets: core::ptr::null(),
    field_metadata_ptr: core::ptr::null(),
    method_table_ptr: core::ptr::null(),
};

/// Read the global outer-table base + length. `&raw const` keeps this
/// safe to take across the extern (`cfg(not(test))`) and stub
/// (`cfg(test)`) definitions alike; only the count deref is unsafe.
#[inline]
fn class_layouts_table() -> (*const StructLayoutEntry, u32) {
    let table: *const StructLayoutEntry = &raw const __torajs_class_layouts;
    // SAFETY: the count global is a plain `u32` in both build paths.
    let n = unsafe { *(&raw const __torajs_n_class_layouts) };
    (table, n)
}

// ---------------------------------------------------------------------------
// Inner idiomatic layer
// ---------------------------------------------------------------------------

/// Resolve a `class_tag` to its layout entry. `None` for `tag == 0`
/// (no layout) or `tag` past the table length.
#[inline]
fn lookup_layout(class_tag: u32) -> Option<&'static StructLayoutEntry> {
    if class_tag == 0 {
        return None;
    }
    let (table, n) = class_layouts_table();
    if class_tag > n {
        return None;
    }
    // SAFETY: `table` is the rodata outer-table base and
    // `class_tag - 1 < n`, so the indexed entry is in bounds.
    Some(unsafe { &*table.add((class_tag - 1) as usize) })
}

impl StructLayoutEntry {
    /// Number of field-metadata records, or `0` when the class has no
    /// metadata (`field_metadata_ptr` is NULL).
    #[inline]
    fn n_fields(&self) -> u32 {
        if self.field_metadata_ptr.is_null() {
            return 0;
        }
        // SAFETY: a non-NULL `field_metadata_ptr` points at a
        // `FieldMetaArrayHeader` (8-aligned, 8 bytes) in rodata.
        unsafe { (*(self.field_metadata_ptr as *const FieldMetaArrayHeader)).n_fields }
    }

    /// The `idx`-th field-metadata record, or `None` if `idx` is out
    /// of range (covers the no-metadata case via `n_fields() == 0`).
    #[inline]
    fn field(&self, idx: u32) -> Option<&'static FieldMeta> {
        if idx >= self.n_fields() {
            return None;
        }
        let body = INNER_FIELD_META_HEADER_SIZE + (idx as usize) * INNER_FIELD_META_ELEM_SIZE;
        // SAFETY: `field_metadata_ptr` is non-NULL (n_fields() > 0) and
        // `idx < n_fields`, so the element at `header + idx*elem` lies
        // within the `.__class_fields_<i>` global.
        let p = unsafe { self.field_metadata_ptr.add(body) } as *const FieldMeta;
        Some(unsafe { &*p })
    }

    /// Linear scan for a field by name. `None` if absent. Field counts
    /// are small, so a linear scan beats hashing here (a perfect hash
    /// is a deferred follow-up if a workload ever needs it).
    #[inline]
    fn find_field(&self, name: &[u8]) -> Option<u32> {
        let n = self.n_fields();
        let mut i = 0;
        while i < n {
            if let Some(f) = self.field(i) {
                if f.name_bytes() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

impl FieldMeta {
    /// The field name as a byte slice. Empty when the name pointer is
    /// NULL or the length is 0.
    #[inline]
    fn name_bytes(&self) -> &'static [u8] {
        if self.name_ptr.is_null() || self.name_len == 0 {
            return &[];
        }
        // SAFETY: a non-NULL `name_ptr` points at `name_len` UTF-8
        // bytes in the `.__class_field_name_<i>_<j>` rodata.
        unsafe { core::slice::from_raw_parts(self.name_ptr, self.name_len as usize) }
    }
}

// ---------------------------------------------------------------------------
// Outer extern "C" shells — thin raw↔safe boundary, no logic.
// ---------------------------------------------------------------------------

/// Look up the struct layout for a `class_tag`. Returns NULL when the
/// tag is `0` (no layout) or past the table length.
///
/// # Safety
/// Reads the link-emitted `__torajs_class_layouts` table; the returned
/// pointer borrows rodata and must not be mutated through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_layout_lookup(class_tag: u32) -> *const StructLayoutEntry {
    match lookup_layout(class_tag) {
        Some(entry) => entry as *const StructLayoutEntry,
        None => core::ptr::null(),
    }
}

/// 405-04 knife 2 fix — whether `class_tag`'s row is a GENERIC
/// specialization row (outer-entry flags bit 1, mirrored from
/// torajs-link `ENTRY_FLAG_GENERIC_ROW`). The proto/class registry
/// alias (`torajs-meta::classmeta::generic_alias`) triggers only on
/// these rows: a non-generic tag with an empty registry slot must
/// keep the null answer — consumer termination logic depends on it
/// (the rotation-408 Iterator-helper hang).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_row_is_generic(class_tag: u32) -> bool {
    match lookup_layout(class_tag) {
        Some(entry) => entry.flags & 2 != 0,
        None => false,
    }
}

/// Read the `idx`-th field name as a `(ptr, len)` slice. Returns an
/// empty slice for a NULL layout, an out-of-range index, or a class
/// with no field metadata.
///
/// # Safety
/// `layout` must be NULL or a pointer returned by
/// [`__torajs_struct_layout_lookup`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_field_name(
    layout: *const StructLayoutEntry,
    idx: u32,
) -> StrSlice {
    if layout.is_null() {
        return StrSlice::EMPTY;
    }
    // SAFETY: caller contract — `layout` is a live lookup result.
    let entry = unsafe { &*layout };
    match entry.field(idx) {
        Some(f) => {
            let b = f.name_bytes();
            StrSlice {
                ptr: b.as_ptr(),
                len: b.len(),
            }
        }
        None => StrSlice::EMPTY,
    }
}

/// Read the `idx`-th field's byte offset + type tag. Returns a zeroed
/// [`FieldInfo`] for a NULL layout or an out-of-range index (a real
/// offset is never 0).
///
/// # Safety
/// `layout` must be NULL or a pointer returned by
/// [`__torajs_struct_layout_lookup`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_field_info(
    layout: *const StructLayoutEntry,
    idx: u32,
) -> FieldInfo {
    if layout.is_null() {
        return FieldInfo::ZEROED;
    }
    // SAFETY: caller contract — `layout` is a live lookup result.
    let entry = unsafe { &*layout };
    match entry.field(idx) {
        Some(f) => FieldInfo {
            field_byte_offset: f.field_byte_offset,
            type_tag: f.type_tag,
        },
        None => FieldInfo::ZEROED,
    }
}

/// Number of field-metadata records in a layout. Returns `0` for a
/// NULL layout or a class with no field metadata. W-J Phase C's
/// `Object.keys`/`values`/`entries` struct arms call this to size the
/// declaration-order walk (`__torajs_struct_field_name` /
/// `__torajs_struct_field_info` for each `idx` in `0..count`).
///
/// # Safety
/// `layout` must be NULL or a pointer returned by
/// [`__torajs_struct_layout_lookup`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_field_count(layout: *const StructLayoutEntry) -> u32 {
    if layout.is_null() {
        return 0;
    }
    // SAFETY: caller contract — `layout` is a live lookup result.
    unsafe { &*layout }.n_fields()
}

/// Find a field index by name. Returns `u32::MAX` when the layout is
/// NULL, the name pointer is NULL, or no field matches.
///
/// # Safety
/// `layout` must be NULL or a pointer returned by
/// [`__torajs_struct_layout_lookup`]; `name` must be NULL or point at
/// `name_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_field_find(
    layout: *const StructLayoutEntry,
    name: *const u8,
    name_len: u32,
) -> u32 {
    if layout.is_null() || name.is_null() {
        return u32::MAX;
    }
    // SAFETY: caller contract — `layout` is a live lookup result and
    // `name` points at `name_len` readable bytes.
    let entry = unsafe { &*layout };
    let needle = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    entry.find_field(needle).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ABI size / offset locks live in the production `const _: () { ... }`
    // block above (compile-time, stronger than a runtime assert). These
    // tests cover the runtime walk behaviour.

    // A hand-built `.__class_fields_<i>` image: 8-byte header (n_fields=2)
    // followed by two 24-byte FieldMeta records. Mirrors the on-disk
    // layout so the helpers walk it exactly as they would the real
    // link-emitted global.
    #[repr(C)]
    struct TwoFieldImage {
        n_fields: u32,
        _pad: u32,
        fields: [FieldMeta; 2],
    }

    const NAME_ALPHA: &[u8] = b"alpha";
    const NAME_BETA: &[u8] = b"beta";

    // RFC 20260714-objlit-accessor — an object literal's accessor lives
    // in the layout under a synthetic slot name; `find_accessor`
    // resolves it from the plain property name the reflection
    // consumers ask with.
    const NAME_GETTER_V: &[u8] = b"__getter_v";
    const NAME_SETTER_V: &[u8] = b"__setter_v";

    fn accessor_image() -> TwoFieldImage {
        TwoFieldImage {
            n_fields: 2,
            _pad: 0,
            fields: [
                FieldMeta {
                    name_ptr: NAME_GETTER_V.as_ptr(),
                    name_len: NAME_GETTER_V.len() as u32,
                    field_byte_offset: 8,
                    type_tag: 4,
                },
                FieldMeta {
                    name_ptr: NAME_SETTER_V.as_ptr(),
                    name_len: NAME_SETTER_V.len() as u32,
                    field_byte_offset: 16,
                    type_tag: 4,
                },
            ],
        }
    }

    #[test]
    fn find_accessor_resolves_both_halves_from_the_plain_name() {
        let image = accessor_image();
        let entry = two_field_entry(&image);

        assert_eq!(entry.find_accessor(b"v", AccessorKind::Getter), Some(0));
        assert_eq!(entry.find_accessor(b"v", AccessorKind::Setter), Some(1));
    }

    #[test]
    fn find_accessor_does_not_match_a_data_field_or_a_wrong_name() {
        let data = make_image();
        let data_entry = two_field_entry(&data);
        // A plain data field is never an accessor, under either half.
        assert_eq!(
            data_entry.find_accessor(b"alpha", AccessorKind::Getter),
            None
        );
        assert_eq!(
            data_entry.find_accessor(b"alpha", AccessorKind::Setter),
            None
        );

        let acc = accessor_image();
        let acc_entry = two_field_entry(&acc);
        // The property is `v`, not the slot's own spelling — asking with
        // the mangled name must not resolve (that would let the internal
        // name leak back in as a user-visible key).
        assert_eq!(
            acc_entry.find_accessor(b"__getter_v", AccessorKind::Getter),
            None
        );
        // A prefix of the real property is not the property.
        assert_eq!(acc_entry.find_accessor(b"", AccessorKind::Getter), None);
        assert_eq!(acc_entry.find_accessor(b"vv", AccessorKind::Getter), None);
    }

    #[test]
    fn find_field_does_not_see_accessor_slots_under_the_plain_name() {
        // The data lookup must miss `v` — the gOPD arm relies on that
        // miss to route into the accessor descriptor instead of reading
        // the closure slot as if it were a value.
        let image = accessor_image();
        let entry = two_field_entry(&image);
        assert_eq!(entry.find_field(b"v"), None);
        assert_eq!(entry.find_field(b"__getter_v"), Some(0));
    }

    fn two_field_entry(image: &TwoFieldImage) -> StructLayoutEntry {
        StructLayoutEntry {
            n_children: 0,
            flags: 0,
            child_offsets: core::ptr::null(),
            field_metadata_ptr: image as *const TwoFieldImage as *const u8,
            method_table_ptr: core::ptr::null(),
        }
    }

    fn make_image() -> TwoFieldImage {
        TwoFieldImage {
            n_fields: 2,
            _pad: 0,
            fields: [
                FieldMeta {
                    name_ptr: NAME_ALPHA.as_ptr(),
                    name_len: NAME_ALPHA.len() as u32,
                    field_byte_offset: 8,
                    type_tag: 1,
                },
                FieldMeta {
                    name_ptr: NAME_BETA.as_ptr(),
                    name_len: NAME_BETA.len() as u32,
                    field_byte_offset: 16,
                    type_tag: 4,
                },
            ],
        }
    }

    #[test]
    fn field_find_round_trip() {
        let image = make_image();
        let entry = two_field_entry(&image);
        let p = &entry as *const StructLayoutEntry;
        unsafe {
            assert_eq!(__torajs_struct_field_find(p, NAME_ALPHA.as_ptr(), 5), 0);
            assert_eq!(__torajs_struct_field_find(p, NAME_BETA.as_ptr(), 4), 1);
            // Absent name → u32::MAX.
            assert_eq!(
                __torajs_struct_field_find(p, b"gamma".as_ptr(), 5),
                u32::MAX
            );
            // Prefix of an existing name must not match (length-aware).
            assert_eq!(
                __torajs_struct_field_find(p, NAME_ALPHA.as_ptr(), 3),
                u32::MAX
            );
        }
    }

    #[test]
    fn field_count_round_trip() {
        let image = make_image();
        let entry = two_field_entry(&image);
        let p = &entry as *const StructLayoutEntry;
        unsafe {
            assert_eq!(__torajs_struct_field_count(p), 2);
            // NULL layout → 0.
            assert_eq!(__torajs_struct_field_count(core::ptr::null()), 0);
            // Entry with NULL field_metadata_ptr → 0.
            let empty = StructLayoutEntry {
                n_children: 0,
                flags: 0,
                child_offsets: core::ptr::null(),
                field_metadata_ptr: core::ptr::null(),
                method_table_ptr: core::ptr::null(),
            };
            assert_eq!(
                __torajs_struct_field_count(&empty as *const StructLayoutEntry),
                0
            );
        }
    }

    #[test]
    fn field_name_and_info_round_trip() {
        let image = make_image();
        let entry = two_field_entry(&image);
        let p = &entry as *const StructLayoutEntry;
        unsafe {
            let n0 = __torajs_struct_field_name(p, 0);
            let s0 = core::slice::from_raw_parts(n0.ptr, n0.len);
            assert_eq!(s0, NAME_ALPHA);

            let n1 = __torajs_struct_field_name(p, 1);
            let s1 = core::slice::from_raw_parts(n1.ptr, n1.len);
            assert_eq!(s1, NAME_BETA);

            let i1 = __torajs_struct_field_info(p, 1);
            assert_eq!(i1.field_byte_offset, 16);
            assert_eq!(i1.type_tag, 4);
        }
    }

    #[test]
    fn out_of_range_idx_is_sentinel() {
        let image = make_image();
        let entry = two_field_entry(&image);
        let p = &entry as *const StructLayoutEntry;
        unsafe {
            let name = __torajs_struct_field_name(p, 2);
            assert!(name.ptr.is_null());
            assert_eq!(name.len, 0);

            let info = __torajs_struct_field_info(p, 9);
            assert_eq!(info.field_byte_offset, 0);
            assert_eq!(info.type_tag, 0);
        }
    }

    #[test]
    fn null_layout_and_empty_metadata() {
        unsafe {
            // NULL layout → sentinels.
            assert_eq!(
                __torajs_struct_field_find(core::ptr::null(), NAME_ALPHA.as_ptr(), 5),
                u32::MAX
            );
            let n = __torajs_struct_field_name(core::ptr::null(), 0);
            assert!(n.ptr.is_null());

            // Entry with NULL field_metadata_ptr → no fields.
            let empty = StructLayoutEntry {
                n_children: 0,
                flags: 0,
                child_offsets: core::ptr::null(),
                field_metadata_ptr: core::ptr::null(),
                method_table_ptr: core::ptr::null(),
            };
            let p = &empty as *const StructLayoutEntry;
            assert_eq!(
                __torajs_struct_field_find(p, NAME_ALPHA.as_ptr(), 5),
                u32::MAX
            );
            let name = __torajs_struct_field_name(p, 0);
            assert!(name.ptr.is_null());
        }
    }

    #[test]
    fn lookup_bounds_against_stub_table() {
        // The cfg(test) stub table has __torajs_n_class_layouts == 0, so
        // every tag (including 0) resolves to NULL — exercises the
        // tag==0 and tag>n bound guards.
        unsafe {
            assert!(__torajs_struct_layout_lookup(0).is_null());
            assert!(__torajs_struct_layout_lookup(1).is_null());
            assert!(__torajs_struct_layout_lookup(u32::MAX).is_null());
        }
    }
}
