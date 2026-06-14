//! W-J A3b prep — types + ABI consts for the class-layouts region.
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
/// 16 → 24 to carry a reserved `field_metadata` ptr slot (NULL until
/// Phase A3 fills it). Must stay in lockstep with
/// `torajs_cycle::layout::ClassLayout`'s `size_of` (24B, asserted in
/// that crate's `class_layout_struct` test).
///   `u32 n_children` (4) + 4-byte pad + `ptr child_offsets` (8) +
///   `ptr field_metadata` (8) = 24
pub(super) const OUTER_ENTRY_SIZE: u32 = 24;
/// Outer-table entry alignment — derived from the `ptr` field's
/// 8-byte alignment requirement. Matches LLVM's struct layout for
/// `{ i32, ptr }` packed=false.
pub(super) const OUTER_ENTRY_ALIGN: u32 = 8;
/// Inner-array element size — `i32`.
pub(super) const INNER_ELEM_SIZE: u32 = 4;
/// Inner-array element alignment — `i32`.
pub(super) const INNER_ELEM_ALIGN: u32 = 4;
/// Count global on-disk size — `u32`.
pub(super) const COUNT_SIZE: u32 = 4;
/// Count global alignment — `u32`.
pub(super) const COUNT_ALIGN: u32 = 4;
/// Outer-entry byte offset where the inner-ptr field lives
/// (4 bytes `n_children` + 4 bytes padding).
pub(super) const OUTER_PTR_OFFSET_IN_ENTRY: u32 = 8;

/// Per-entry placement — both the inner `.__class_offsets_<i>` global
/// (if `child_offsets` is non-empty) and the outer-table slot.
#[derive(Debug, Clone)]
pub struct UserClassLayoutEntryLayout {
    /// Number of refcounted-pointer fields the entry advertises.
    /// Mirrors `child_offsets.len()`; cached for the build pass.
    pub n_children: u32,
    /// Cloned offsets for the build pass — written little-endian
    /// into the inner global.
    pub child_offsets: Vec<u32>,
    /// `None` when `child_offsets` is empty: no inner global is
    /// allocated and the outer slot packs `(0, NULL)`. `Some(vaddr)`
    /// is the absolute vaddr of `.__class_offsets_<i>`.
    pub inner_vaddr: Option<u64>,
    /// File offset of the inner global (or `0` when no inner is
    /// emitted — caller never indexes it in that case).
    pub inner_file_offset: u32,
    /// Outer-table entry vaddr (= `outer_vaddr + i * 16`). Inner ptr
    /// slot lives at `entry_vaddr + OUTER_PTR_OFFSET_IN_ENTRY`.
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
    /// Region's first byte vaddr (matches `file_offset` after the
    /// caller-supplied `vaddr_base` translation).
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
    /// Total cumulative byte size = leading pad + inner globals +
    /// outer/count alignment pads + outer table + count u32. Caller
    /// adds this to the segment cursor.
    pub total_size: u32,
}
