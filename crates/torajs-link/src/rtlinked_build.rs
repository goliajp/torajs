//! Build the prelinked runtime image: every global a `torajs*` member
//! defines, plus everything those reach, laid out once through the
//! stock merge / closure / layout passes with no user code, and
//! relocated against a fixed origin. The result
//! ([`crate::rtlinked_format`]) is independent of any program: names the
//! runtime expects the program to define ([`USER_TABLE_NAMES`]) are
//! reached through GOT slots the loader fills, so text needs no
//! per-run rewrite.

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

use crate::archive_link::{ArchiveLayout, ArchiveLayoutError, compute_archive_layout_with_merged};
use crate::archives_merge::{MergedArchives, merge_archive_indexes};
use crate::defined_extern_resolve::insert_member_syms;
use crate::exec::LinkConfig;
use crate::exec_user_entries::UserDataGlobalEntry;
use crate::resolve::SymTable;
use crate::rtlinked_build_encode::{ImageParts, encode_image};
use crate::rtlinked_build_regions as regions;
use crate::rtlinked_format::SEGMENT_ALIGN;
use crate::tlv_descriptor_layout::apply_tlv_overrides;

/// Program-side tables the runtime reads. Each gets an 8-byte slot in
/// the image's data; every runtime reference must be a GOT load.
pub const USER_TABLE_NAMES: [&str; 6] = [
    "___torajs_class_layouts",
    "___torajs_n_class_layouts",
    "___torajs_fn_name_table",
    "___torajs_fn_name_table_count",
    "___torajs_class_name_table",
    "___torajs_n_class_names",
];

/// Placeholder entry the closure walk starts from; never emitted.
const ROOT_FN: &str = "_main";

#[derive(Debug)]
pub enum RtLinkedBuildError {
    /// Merge, closure (including names nobody defines), layout or reloc
    /// failure from the stock passes.
    Layout(ArchiveLayoutError),
    /// The runtime has a shape a fixed image cannot hold.
    Unsupported(String),
}

impl From<ArchiveLayoutError> for RtLinkedBuildError {
    fn from(e: ArchiveLayoutError) -> Self {
        RtLinkedBuildError::Layout(e)
    }
}

/// Link `runtime_image` (a [`torajs_rtimage`] blob) into a prelinked
/// image whose origin is meant to sit at `preferred_base`.
pub fn build_rtlinked_image(
    runtime_image: &[u8],
    preferred_base: u64,
) -> Result<Vec<u8>, RtLinkedBuildError> {
    if !preferred_base.is_multiple_of(u64::from(SEGMENT_ALIGN)) {
        return Err(RtLinkedBuildError::Unsupported(format!(
            "preferred base {preferred_base:#x} is not {SEGMENT_ALIGN:#x}-aligned"
        )));
    }
    let merged = merge_archive_indexes(runtime_image).map_err(ArchiveLayoutError::Merge)?;
    let cfg = link_cfg(runtime_image, root_fn(&roots(&merged)));
    let layout = compute_archive_layout_with_merged(&cfg, &merged, &Default::default())?;
    check_no_program_regions(&layout)?;

    let (sym_table, got_slots) = sym_tables(&layout, &merged)?;
    let origin = regions::origin(&layout);
    let mut parts = ImageParts {
        preferred_base,
        ..ImageParts::default()
    };
    regions::text(&layout, &merged, &sym_table, &got_slots, origin, &mut parts)?;
    regions::data(&layout, &merged, &sym_table, origin, &mut parts)?;
    crate::rtlinked_build_tlv::tlv(&layout, &merged, &sym_table, origin, &mut parts)?;
    regions::imports(&layout, &got_slots, origin, &mut parts)?;
    parts.exports = sym_table
        .iter()
        .map(|(n, &v)| Ok((n.clone(), regions::off(v, origin)?)))
        .collect::<Result<_, RtLinkedBuildError>>()?;
    Ok(encode_image(&parts))
}

/// Every global a `torajs*` member defines.
fn roots<'a>(merged: &MergedArchives<'a>) -> Vec<&'a str> {
    merged
        .symbols()
        .filter(|(_, s)| {
            merged
                .member(s.archive_idx, s.member_idx)
                .name
                .starts_with("torajs")
        })
        .map(|(n, _)| n)
        .collect()
}

/// One function whose relocs name every root, so the stock closure
/// walk pulls the whole runtime.
pub(crate) fn root_fn(roots: &[&str]) -> CompiledFunction {
    let relocs = roots
        .iter()
        .map(|r| Reloc {
            byte_offset: 0,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern((*r).to_string()),
            },
        })
        .collect();
    CompiledFunction {
        name: ROOT_FN.into(),
        bytes: vec![0xC0, 0x03, 0x5F, 0xD6],
        relocs,
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// The user-table names become 8-byte data globals: the closure treats
/// them as defined, and their storage is the GOT slot.
pub(crate) fn link_cfg(runtime_image: &[u8], root: CompiledFunction) -> LinkConfig {
    let data_globals = USER_TABLE_NAMES
        .iter()
        .map(|s| UserDataGlobalEntry {
            sym: (*s).to_string(),
            size: 8,
            align_log2: 3,
        })
        .collect();
    LinkConfig {
        user_loops: Default::default(),
        funcs: vec![root],
        entry: ROOT_FN.into(),
        sym_table: SymTable::new(),
        codesign_ident: "rtlinked".into(),
        dead_strip: false,
        strip_member_symbols: true,
        elidable_sites: Vec::new(),
        guarded_stubs: Vec::new(),
        image: runtime_image.to_vec().into(),
        strings: Vec::new(),
        data_globals,
        vtable_globals: Vec::new(),
        class_layouts: Vec::new(),
        force_emit_class_layouts_globals: false,
        fn_name_globals: Vec::new(),
        force_emit_fn_name_globals: false,
        class_names: Vec::new(),
        force_emit_class_names_globals: false,
        baked_regex_entries: Vec::new(),
    }
}

/// With no program, every per-program region must come out empty.
fn check_no_program_regions(layout: &ArchiveLayout) -> Result<(), RtLinkedBuildError> {
    let present = [
        ("user strings", layout.user_strings_layout.total_size != 0),
        (
            "user vtables",
            !layout.user_vtables_layout.entries.is_empty(),
        ),
        ("__DATA_CONST", layout.data_const_layout.has_data_const),
        ("fn name table", layout.fn_name_table_layout.total_size != 0),
        (
            "class name table",
            layout.class_name_table_layout.total_size != 0,
        ),
    ];
    match present.iter().find(|(_, p)| *p) {
        Some((what, _)) => Err(RtLinkedBuildError::Unsupported(format!(
            "{what} present in a program-free layout"
        ))),
        None => Ok(()),
    }
}

/// The runtime's vaddr table (member defined externs, import stubs,
/// TLV variables) and the user-table GOT slots, kept apart so any
/// direct reference to a user table fails as unresolved.
fn sym_tables(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
) -> Result<(SymTable, SymTable), RtLinkedBuildError> {
    let mut table = SymTable::new();
    insert_member_syms(layout, &mut table);
    for (name, stub) in &layout.stub_vaddrs {
        table.insert(name.clone(), *stub);
    }
    apply_tlv_overrides(layout, merged, &mut table).map_err(ArchiveLayoutError::TlvOverride)?;
    let got_slots: SymTable = layout
        .user_data_globals_layout
        .entries
        .iter()
        .map(|e| (e.sym.clone(), e.vaddr))
        .collect();
    if let Some(name) = got_slots.keys().find(|n| table.contains_key(*n)) {
        return Err(RtLinkedBuildError::Unsupported(format!(
            "user table {name} is also defined by the runtime"
        )));
    }
    Ok((table, got_slots))
}
