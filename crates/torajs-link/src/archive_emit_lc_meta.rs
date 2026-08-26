//! Mach-O load-command meta — per-cfg flag set + extra LC count/size
//! + `MH_*` flag bits. Derived purely from `ArchiveLayout`; lifted out
//! of `emit_binary` so the prelude block stays one self-contained
//! computation step.

use torajs_obj::{SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE};

use crate::archive_link::ArchiveLayout;
use crate::lc::{
    LINKEDIT_DATA_CMDSIZE, LOAD_DYLIB_LIBCURL_CMDSIZE, LOAD_DYLIB_LIBSYSTEM_CMDSIZE, MH_DYLDLINK,
    MH_NOUNDEFS, MH_PIE, MH_TWOLEVEL,
};

/// Flag set + LC sizing + MH flag bits that the prelude block of
/// `emit_binary` needs to size the header and pick segment/load-cmd
/// emit branches. All `pub(crate)`; the struct is internal.
pub(crate) struct EmitLcMeta {
    pub has_dyld: bool,
    pub has_chained_fixups: bool,
    pub has_libcurl_lc: bool,
    pub has_libsystem_lc: bool,
    pub extra_lc_count: u32,
    pub extra_lc_size: u32,
    pub mh_flags: u32,
}

/// dyld imports → LC_LOAD_DYLIB + __stubs/__la_symbol_ptr;
/// e8 text rebase → LC_DYLD_CHAINED_FIXUPS even without la-ptr.
pub(crate) fn compute_emit_lc_meta(layout: &ArchiveLayout) -> EmitLcMeta {
    let has_dyld = layout.has_dyld;
    let has_data_const = layout.data_const_layout.has_data_const;
    let has_text_rebase = !layout.text_rebase_link_values.is_empty();
    let has_chained_fixups = has_dyld || has_text_rebase;
    let ordinals_used: std::collections::BTreeSet<u8> =
        layout.dyld_imports.values().copied().collect();
    // dyld assigns ordinals by LC_LOAD_DYLIB position; chain encoder
    // hard-codes ord 1 = libSystem, so emit libSystem first.
    let has_libcurl_lc = ordinals_used.contains(&2);
    let has_libsystem_lc = has_dyld;
    let load_dylib_count = u32::from(has_libsystem_lc) + u32::from(has_libcurl_lc);
    let load_dylib_size = (if has_libsystem_lc {
        LOAD_DYLIB_LIBSYSTEM_CMDSIZE
    } else {
        0
    }) + (if has_libcurl_lc {
        LOAD_DYLIB_LIBCURL_CMDSIZE
    } else {
        0
    });
    // ncmds delta: LC_LOAD_DYLIBs + (__DATA when has_dyld) +
    // (__DATA_CONST when has_data_const) + LC_DYLD_CHAINED_FIXUPS.
    let extra_lc_count = if has_dyld { load_dylib_count + 1 } else { 0 }
        + u32::from(has_data_const)
        + u32::from(has_chained_fixups);
    // c-fix-c4 — each member `__DATA,*` section + 1 section_64 entry.
    let data_non_text_section_count: u32 = layout.data_merged_sections.len() as u32
        + u32::from(layout.user_data_globals_layout.total_vmsize > 0);
    let extra_lc_size = (if has_dyld {
        // load_dylibs + __TEXT __stubs section + __DATA seg + its
        // __la_symbol_ptr section + member __DATA,* sections.
        load_dylib_size
            + SECTION_64_SIZE
            + SEGMENT_COMMAND_64_SIZE
            + SECTION_64_SIZE
            + (SECTION_64_SIZE * data_non_text_section_count)
    } else {
        0
    }) + (if has_data_const {
        SEGMENT_COMMAND_64_SIZE
    } else {
        0
    }) + (if has_chained_fixups {
        LINKEDIT_DATA_CMDSIZE
    } else {
        0
    });
    // has_dyld clears MH_NOUNDEFS (link-time unresolved syms allowed).
    let mh_flags = if has_dyld {
        MH_DYLDLINK | MH_TWOLEVEL | MH_PIE
    } else {
        MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE
    };
    EmitLcMeta {
        has_dyld,
        has_chained_fixups,
        has_libcurl_lc,
        has_libsystem_lc,
        extra_lc_count,
        extra_lc_size,
        mh_flags,
    }
}
