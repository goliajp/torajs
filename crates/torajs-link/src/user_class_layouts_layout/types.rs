//! W-J A3b — types + ABI consts for the class-layouts region.
//! See parent module doc (`user_class_layouts_layout.rs`) for the
//! per-block layout diagram.

/// Public sym names the link layer registers into the effective sym
/// table after this layout's vaddr placement is known. Apple `_`-prefix
/// form (the actual Mach-O on-disk name) so staticlib members that
/// reference these via `extern "C"` resolve at the link layer; codegen
/// callers (probe + tr build) emit `Page21`/`PageOff12` relocs against
/// the same name.
pub const CLASS_LAYOUTS_SYM: &str = "___torajs_class_layouts";
pub const N_CLASS_LAYOUTS_SYM: &str = "___torajs_n_class_layouts";

/// On-disk size of one outer-table entry. W-J Phase A2 extended
/// 16 → 24 to carry a reserved `field_metadata_ptr` slot. W-J Phase
/// A3b populates that slot with the vaddr of the per-class
/// `.__class_fields_<i>` inner global (or NULL when the class has
/// no field metadata). L3b #4 turns the former 4-byte pad at +4
/// into a `u32 flags` word (bit 0 = named class).
///   `u32 n_children` (4) + `u32 flags` (4) + `ptr child_offsets` (8) +
///   `ptr field_metadata` (8) + `ptr method_table` (8) = 32
/// (刀 4 extended 24 → 32 for the runtime class-method dispatch
/// table; runtime mirrors in torajs-cycle + torajs-structmeta moved
/// in the same commit.)
pub(super) const OUTER_ENTRY_SIZE: u32 = 32;
/// L3b #4 — outer-entry flags bit 0: entry describes a declared
/// class (vs an anonymous struct shape). The runtime Obj drop reads
/// this to restrict cycle-root buffering to named-class instances,
/// mirroring the typed drop path's policy. Runtime mirror:
/// `torajs-cycle::layout::CLASS_LAYOUT_FLAG_NAMED`.
pub const ENTRY_FLAG_NAMED_CLASS: u32 = 1;
/// 405-04 knife 2 fix — outer-entry flags bit 1: a GENERIC
/// specialization row. The runtime registry alias
/// (`torajs-meta::classmeta::generic_alias`) resolves proto/class
/// reads through the main tag ONLY for rows carrying this bit.
pub const ENTRY_FLAG_GENERIC_ROW: u32 = 2;
/// Outer-table entry alignment.
pub(super) const OUTER_ENTRY_ALIGN: u32 = 8;
/// Inner child_offsets array element size — `i32`.
pub(super) const INNER_ELEM_SIZE: u32 = 4;
/// Inner child_offsets array element alignment.
pub(super) const INNER_ELEM_ALIGN: u32 = 4;
/// W-J A3b — inner FieldMeta array header size (`u32 n_fields` +
/// `u32 _pad`). Sits at the top of every non-empty
/// `.__class_fields_<i>` global so the runtime helper can read N
/// before walking the body.
pub(super) const INNER_FIELD_META_HEADER_SIZE: u32 = 8;
/// W-J A3b — on-disk size of one FieldMeta entry:
///   `ptr name (8)` + `u32 name_len (4)` + `u32 field_byte_offset (4)`
///   + `u8 type_tag (1)` + `u8[7] pad (7)` = 24 bytes.
pub(super) const INNER_FIELD_META_ELEM_SIZE: u32 = 24;
/// W-J A3b — alignment of the inner FieldMeta global (header + body).
pub(super) const INNER_FIELD_META_ALIGN: u32 = 8;
/// W-J A3b — byte offset of `name_ptr` inside one FieldMeta entry.
pub(super) const FIELD_META_NAME_PTR_OFFSET_IN_ELEM: u32 = 0;
/// W-J A3b — byte offset of `name_len` inside one FieldMeta entry.
pub(super) const FIELD_META_NAME_LEN_OFFSET_IN_ELEM: u32 = 8;
/// W-J A3b — byte offset of `field_byte_offset` inside one FieldMeta entry.
pub(super) const FIELD_META_FIELD_OFFSET_OFFSET_IN_ELEM: u32 = 12;
/// W-J A3b — byte offset of `type_tag` inside one FieldMeta entry.
pub(super) const FIELD_META_TYPE_TAG_OFFSET_IN_ELEM: u32 = 16;
/// Count global on-disk size — `u32`.
pub(super) const COUNT_SIZE: u32 = 4;
/// Count global alignment — `u32`.
pub(super) const COUNT_ALIGN: u32 = 4;
/// Outer-entry byte offset where the `child_offsets_ptr` field lives
/// (4 bytes `n_children` + 4 bytes padding).
pub(super) const OUTER_CHILD_OFFSETS_PTR_OFFSET_IN_ENTRY: u32 = 8;
/// W-J A3b — outer-entry byte offset where the `field_metadata_ptr`
/// field lives (after child_offsets_ptr).
pub(super) const OUTER_FIELD_META_PTR_OFFSET_IN_ENTRY: u32 = 16;
/// 刀 4 (RFC 20260714-t262-top-clusters) — outer-entry byte offset of
/// the `method_table_ptr` slot (NULL when the class has no
/// runtime-dispatchable methods). Runtime mirrors:
/// `torajs-cycle::layout::ClassLayout.method_table_ptr` /
/// `torajs-structmeta`'s `OUTER_METHOD_TABLE_PTR_OFFSET`.
pub(super) const OUTER_METHOD_TABLE_PTR_OFFSET_IN_ENTRY: u32 = 24;
/// 刀 4 — inner MethodMeta array header size (`u32 n_methods` +
/// `u32 _pad`), mirroring the FieldMeta header shape.
pub(super) const INNER_METHOD_META_HEADER_SIZE: u32 = 8;
/// 刀 4 — on-disk size of one MethodMeta entry:
///   `ptr name (8)` + `u32 name_len (4)` + `u32 flags (4)`
///   + `ptr adapter (8)` + `ptr twin (8)` = 32 bytes (blade 3
///   appended the `__cmany_` twin adapter slot; 0 = no twin).
pub(super) const INNER_METHOD_META_ELEM_SIZE: u32 = 32;
/// 刀 4 — alignment of the inner MethodMeta global.
pub(super) const INNER_METHOD_META_ALIGN: u32 = 8;
/// 刀 4 — byte offset of `name_ptr` inside one MethodMeta entry.
pub(super) const METHOD_META_NAME_PTR_OFFSET_IN_ELEM: u32 = 0;
/// 刀 4 — byte offset of `name_len` inside one MethodMeta entry.
pub(super) const METHOD_META_NAME_LEN_OFFSET_IN_ELEM: u32 = 8;
/// 刀 4 — byte offset of `adapter_ptr` inside one MethodMeta entry.
pub(super) const METHOD_META_ADAPTER_PTR_OFFSET_IN_ELEM: u32 = 16;
/// Blade 3 — byte offset of the `__cmany_` twin adapter ptr inside
/// one MethodMeta entry (0-baked when the method has no twin).
pub(super) const METHOD_META_TWIN_PTR_OFFSET_IN_ELEM: u32 = 24;

/// W-J A3b — placement record for one per-field name byte string and
/// its FieldMeta descriptor. Both live in the inner globals region; the
/// FieldMeta's `name_ptr` slot points at `name_vaddr` (rebased at load
/// time via a chained-fixup rebase target).
#[derive(Debug, Clone)]
pub struct UserFieldMetaPlacement {
    /// UTF-8 bytes of the field name (no NUL terminator) — copied at
    /// layout time so `build_user_class_layouts_payload` can splice
    /// them into the inner globals region without re-reading the
    /// caller's `UserClassLayoutEntry`.
    pub name_bytes: Vec<u8>,
    /// Absolute vaddr of `.__class_field_name_<i>_<j>` (used as the
    /// chained-fixup rebase target for the FieldMeta's `name_ptr` slot).
    pub name_vaddr: u64,
    /// File offset of `.__class_field_name_<i>_<j>`.
    pub name_file_offset: u32,
    /// Byte offset within the instance (= `OBJ_HEADER_SIZE + i*8`).
    pub field_byte_offset: u32,
    /// Coarse 8-bit Type discriminator (`ssa::field_type_tag_of`).
    pub type_tag: u8,
}

/// 刀 4 — placement record for one per-method name byte string and
/// its MethodMeta descriptor (mirrors [`UserFieldMetaPlacement`]).
#[derive(Debug, Clone)]
pub struct UserMethodMetaPlacement {
    /// UTF-8 bytes of the method name (no NUL terminator).
    pub name_bytes: Vec<u8>,
    /// Absolute vaddr of `.__class_method_name_<i>_<j>` (chained-
    /// fixup rebase target for the MethodMeta's `name_ptr` slot).
    pub name_vaddr: u64,
    /// File offset of `.__class_method_name_<i>_<j>`.
    pub name_file_offset: u32,
    /// The boxed adapter's fn id — indexes the link layer's
    /// `fn_vaddrs` slice at rebase-assembly time; `None` bakes a 0
    /// adapter_ptr and enumerates no rebase target for it (r502).
    pub adapter_fn_id: Option<u32>,
    /// Blade 3 — the `__cmany_` twin's boxed adapter fn id; `None`
    /// bakes a 0 twin_ptr and enumerates no rebase target for it.
    pub twin_fn_id: Option<u32>,
    /// S2.38 — MethodMeta flags word (bit 0 = this-free body),
    /// written into the elem's `+12` u32 (formerly pad).
    pub flags: u32,
}

/// Per-entry placement — inner child_offsets global, per-field name
/// strings + FieldMeta inner global, and the outer-table slot.
#[derive(Debug, Clone)]
pub struct UserClassLayoutEntryLayout {
    /// Number of refcounted-pointer fields the entry advertises
    /// (= `child_offsets.len()` — cycle collector consumer).
    pub n_children: u32,
    /// L3b #4 — outer-entry flags word written at +4 (bit 0 =
    /// [`ENTRY_FLAG_NAMED_CLASS`]).
    pub flags: u32,
    /// Cloned offsets for the build pass — written little-endian
    /// into `.__class_offsets_<i>`.
    pub child_offsets: Vec<u32>,
    /// `None` when `child_offsets` is empty: no `.__class_offsets_<i>`
    /// global allocated and the outer slot packs `child_offsets_ptr = 0`.
    /// `Some(vaddr)` is the absolute vaddr of `.__class_offsets_<i>`.
    pub inner_vaddr: Option<u64>,
    /// File offset of `.__class_offsets_<i>` (or `0` when no inner is
    /// emitted — caller never indexes it in that case).
    pub inner_file_offset: u32,
    /// W-J A3b — per-field name + offset + type_tag placement. Empty
    /// Vec means the class has no field metadata; no
    /// `.__class_fields_<i>` global is emitted and the outer entry's
    /// `field_metadata_ptr` slot stays NULL.
    pub field_metadata: Vec<UserFieldMetaPlacement>,
    /// W-J A3b — vaddr of the per-class FieldMeta array
    /// `.__class_fields_<i>` (header + body). `None` ↔ `field_metadata.is_empty()`.
    pub field_meta_array_vaddr: Option<u64>,
    /// W-J A3b — file offset of `.__class_fields_<i>`.
    pub field_meta_array_file_offset: u32,
    /// 刀 4 — per-method name-string + adapter placement. Empty Vec
    /// means no `.__class_methods_<i>` global; the outer entry's
    /// `method_table_ptr` slot stays NULL.
    pub methods: Vec<UserMethodMetaPlacement>,
    /// 刀 4 — vaddr of the per-class MethodMeta array
    /// `.__class_methods_<i>`. `None` ↔ `methods.is_empty()`.
    pub method_meta_array_vaddr: Option<u64>,
    /// 刀 4 — file offset of `.__class_methods_<i>`.
    pub method_meta_array_file_offset: u32,
    /// Outer-table entry vaddr (= `outer_vaddr + i * OUTER_ENTRY_SIZE`).
    pub entry_vaddr: u64,
    /// Outer-table entry file offset.
    pub entry_file_offset: u32,
}

/// Aggregated layout for one full class-layouts region.
#[derive(Debug, Clone, Default)]
pub struct UserClassLayoutsLayout {
    pub entries: Vec<UserClassLayoutEntryLayout>,
    /// Region's first byte file offset (= leading-pad start, before
    /// any inner globals).
    pub file_offset: u32,
    /// Region's first byte vaddr.
    pub vaddr: u64,
    /// `__torajs_class_layouts` outer-table vaddr (= sym vaddr for
    /// `apply_user_class_layouts_overrides`).
    pub outer_vaddr: u64,
    /// `__torajs_class_layouts` outer-table file offset.
    pub outer_file_offset: u32,
    /// `__torajs_n_class_layouts` count global vaddr.
    pub count_vaddr: u64,
    /// `__torajs_n_class_layouts` count global file offset.
    pub count_file_offset: u32,
    /// Total cumulative byte size = leading pad + inner globals
    /// (child_offsets + field name strings + field meta arrays) +
    /// outer/count alignment pads + outer table + count u32.
    pub total_size: u32,
}
