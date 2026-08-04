//! SD-4c-prereq+e8 — layout + emit for the cycle collector's
//! per-class `child_offsets` tables.
//!
//! Byte ABI from the LLVM-era `ssa_inkwell::class_globals::
//! emit_class_layouts` (canonical here since the swap). The
//! cycle collector indexes by `class_tag - 1`; for each entry it
//! reads `n_children` then walks `offsets[]` to enumerate the
//! refcounted-pointer fields. Three rodata blocks per `class_layouts`
//! input, all sharing `__DATA_CONST` with the vtables:
//!
//! 1. `N` inner `.__class_offsets_<i>` arrays — `[K x i32]` (private
//!    naming; never enters the public sym table). Empty entry → no
//!    inner global allocated, the outer slot packs `(0, NULL)`.
//! 2. `__torajs_class_layouts` outer table — `[N x { u32 n; ptr;
//!    ptr field_metadata }]` laid out as 24-byte entries (`n` +
//!    4-byte pad + 8-byte child_offsets ptr + 8-byte field_metadata
//!    ptr). W-J Phase A2: the field_metadata slot is reserved + NULL;
//!    A3 will fill it with a pointer to per-class field-name/offset/
//!    type-tag metadata so the reflection consumers (Phase B `gOPD`
//!    struct arm / Phase C `Object.keys`/`values`/`entries` / Phase D
//!    `inspect.rs` Tag::Obj walker) can do struct cell reflection.
//! 3. `__torajs_n_class_layouts` count — `u32 = N`.
//!
//! Each non-empty entry's outer ptr slot needs an `LC_DYLD_CHAINED_FIXUPS`
//! rebase entry so dyld stamps the inner global's runtime vaddr after
//! the ASLR slide (same e7b path the vtable slots ride on).
//!
//! Module shape (W-J A3b prep — file-size hard 500): split into
//! `types` (consts + layout structs), `compute` (placement math),
//! `payload` (byte-emit), `sym` (sym-table apply + rebase target).
//! All public API stays accessible via `crate::user_class_layouts_layout::*`
//! re-exports for unchanged external import sites.

mod compute;
mod payload;
mod sym;
mod types;

pub use compute::{build_user_class_layouts_region, compute_user_class_layouts_layout};
pub use payload::build_user_class_layouts_payload;
pub use sym::{
    apply_user_class_layouts_overrides, compute_class_layouts_rebase_targets,
    user_class_layouts_extra_defined_syms,
};
pub use types::{
    CLASS_LAYOUTS_SYM, ENTRY_FLAG_NAMED_CLASS, N_CLASS_LAYOUTS_SYM, UserClassLayoutEntryLayout,
    UserClassLayoutsLayout, UserFieldMetaPlacement, UserMethodMetaPlacement,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_twin;
