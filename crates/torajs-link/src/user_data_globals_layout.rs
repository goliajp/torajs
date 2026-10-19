//! SD-4c-prereq+e4 — layout pass for user-binary mutable top-level
//! globals. Each `UserDataGlobalEntry` becomes one zero-init slot in
//! the binary's `__DATA,__bss` section; the loader supplies zero
//! bytes at map time so no file payload is emitted.
//!
//! Layout discipline:
//!
//! - Entries pack in input order, each one padded up to its
//!   `align_log2` requirement before placement.
//! - The caller passes the first free vaddr after all file-storage
//!   `__DATA` sections and the runtime members' zerofill (Mach-O
//!   requires zerofill sections to come last within their segment).
//!   That address can stop at any byte, so the section start is
//!   rounded up to the section alignment (at least 8): entry offsets
//!   are only aligned relative to the section start, and codegen folds
//!   a global's page offset into the scaled immediate of an 8-byte
//!   load / store, which only reaches an 8-aligned address.
//! - `total_vmsize` is the section's `vmsize` — the `vmsize` of the
//!   enclosing `__DATA` segment is extended by this amount, while
//!   `filesize` is unchanged (zerofill occupies vmsize, not file
//!   bytes).

use crate::exec::UserDataGlobalEntry;
use crate::resolve::SymTable;

/// Per-entry placement — `vaddr` is what the SymTable learns so
/// codegen-emitted `RelocKind::Page21 / PageOff12 { target_sym: e.sym }`
/// ADRP+ADD pairs resolve to.
#[derive(Debug, Clone)]
pub struct UserDataGlobalEntryLayout {
    pub sym: String,
    pub vaddr: u64,
    /// Offset from the section's start (= `vaddr - vaddr_base`).
    pub section_offset: u32,
    pub size: u32,
}

/// Aggregated `__DATA,__bss` layout for one user-data-globals set.
#[derive(Debug, Clone, Default)]
pub struct UserDataGlobalsLayout {
    pub entries: Vec<UserDataGlobalEntryLayout>,
    /// Absolute vaddr of the section's first byte (`vaddr_base`
    /// rounded up to `1 << align_log2`).
    pub vaddr: u64,
    /// Section alignment — the largest entry alignment, and never
    /// below [`USER_BSS_MIN_ALIGN_LOG2`].
    pub align_log2: u32,
    /// Section vmsize — sum of entry sizes plus inter-entry alignment
    /// pad. 0 when input is empty.
    pub total_vmsize: u32,
}

/// Minimum `__DATA,__bss` section alignment: every 8-byte slot sits on
/// an 8-byte boundary in absolute terms, not only relative to the
/// section start.
pub const USER_BSS_MIN_ALIGN_LOG2: u32 = 3;

/// Compute per-entry placements for one user-data-globals set, in a
/// section starting at `vaddr_base` rounded up to the section
/// alignment. Pure function — no allocations beyond the returned
/// `entries` Vec.
pub fn compute_user_data_globals_layout(
    globals: &[UserDataGlobalEntry],
    vaddr_base: u64,
) -> UserDataGlobalsLayout {
    let align_log2 = globals
        .iter()
        .map(|g| u32::from(g.align_log2))
        .fold(USER_BSS_MIN_ALIGN_LOG2, u32::max);
    let section_mask = (1u64 << align_log2) - 1;
    let vaddr_base = (vaddr_base + section_mask) & !section_mask;
    let mut entries = Vec::with_capacity(globals.len());
    let mut running: u32 = 0;
    for g in globals {
        let align = 1u32 << g.align_log2;
        let mask = align - 1;
        running = (running + mask) & !mask;
        entries.push(UserDataGlobalEntryLayout {
            sym: g.sym.clone(),
            vaddr: vaddr_base + u64::from(running),
            section_offset: running,
            size: g.size,
        });
        running += g.size;
    }
    UserDataGlobalsLayout {
        entries,
        vaddr: vaddr_base,
        align_log2,
        total_vmsize: running,
    }
}

/// Register each entry's `sym → vaddr` into the effective sym table.
/// Caller invokes after `apply_user_string_overrides` so per-link
/// overrides land in a consistent order.
pub fn apply_user_data_global_overrides(layout: &UserDataGlobalsLayout, sym_table: &mut SymTable) {
    for entry in &layout.entries {
        sym_table.insert(entry.sym.clone(), entry.vaddr);
    }
}

/// Sym names to flag as link-defined in the worklist closure so a
/// user-fn reloc against the global's `sym` isn't surfaced as
/// `UnresolvedExterns` before emit. Mirrors
/// `user_strings_extra_defined_syms`.
pub fn user_data_globals_extra_defined_syms(
    globals: &[UserDataGlobalEntry],
) -> std::collections::BTreeSet<String> {
    globals.iter().map(|g| g.sym.clone()).collect()
}

/// Pipeline helper validating the e4 has_dyld precondition + layout
/// build in one call. Keeps `archive_link.rs`'s call-site at one
/// line per the ≥soft LOC floor convention. Caller passes
/// `vaddr_base` only when `has_dyld` is true; the empty path skips
/// computation entirely and returns the zero-sized default.
pub fn build_user_data_globals_region(
    globals: &[UserDataGlobalEntry],
    has_dyld: bool,
    vaddr_base: u64,
) -> Result<UserDataGlobalsLayout, usize> {
    if globals.is_empty() {
        return Ok(UserDataGlobalsLayout::default());
    }
    if !has_dyld {
        return Err(globals.len());
    }
    Ok(compute_user_data_globals_layout(globals, vaddr_base))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sym: &str, size: u32, align_log2: u8) -> UserDataGlobalEntry {
        UserDataGlobalEntry {
            sym: sym.into(),
            size,
            align_log2,
        }
    }

    #[test]
    fn empty_input_zero_vmsize() {
        let layout = compute_user_data_globals_layout(&[], 0x1_0000_8000);
        assert!(layout.entries.is_empty());
        assert_eq!(layout.total_vmsize, 0);
        assert_eq!(layout.vaddr, 0x1_0000_8000);
    }

    #[test]
    fn unaligned_base_rounds_section_start_to_8() {
        // the runtime members' zerofill can end on any byte; every
        // 8-byte slot must still land on an absolute 8-byte boundary
        let globals = [
            entry("a_bool", 1, 0),
            entry("b_i64", 8, 3),
            entry("c_i32", 4, 2),
            entry("d_f64", 8, 3),
        ];
        for base in 0x1_0000_8000u64..0x1_0000_8010 {
            let layout = compute_user_data_globals_layout(&globals, base);
            assert_eq!(layout.vaddr % 8, 0, "base 0x{base:X}");
            assert!(layout.vaddr >= base && layout.vaddr - base < 8);
            assert_eq!(layout.align_log2, 3);
            for e in &layout.entries {
                if e.size == 8 {
                    assert_eq!(e.vaddr % 8, 0, "{} at base 0x{base:X}", e.sym);
                }
            }
            assert_eq!(layout.entries[0].vaddr, layout.vaddr);
        }
    }

    #[test]
    fn single_i64_entry_8_byte_size() {
        let globals = [entry("global_x", 8, 3)];
        let layout = compute_user_data_globals_layout(&globals, 0x1_0000_8000);
        assert_eq!(layout.entries.len(), 1);
        assert_eq!(layout.entries[0].vaddr, 0x1_0000_8000);
        assert_eq!(layout.entries[0].section_offset, 0);
        assert_eq!(layout.entries[0].size, 8);
        assert_eq!(layout.total_vmsize, 8);
    }

    #[test]
    fn mixed_alignment_pads_correctly() {
        // I32 (4, align 4) + Bool (1, align 1) + I64 (8, align 8)
        //   offset 0  → I32 → end at 4
        //   offset 4  → Bool → end at 5
        //   offset 5  → I64 needs align 8, pad to 8 → end at 16
        let globals = [
            entry("a_i32", 4, 2),
            entry("b_bool", 1, 0),
            entry("c_i64", 8, 3),
        ];
        let layout = compute_user_data_globals_layout(&globals, 0x1_0000_8000);
        assert_eq!(layout.entries[0].section_offset, 0);
        assert_eq!(layout.entries[1].section_offset, 4);
        assert_eq!(layout.entries[2].section_offset, 8);
        assert_eq!(layout.total_vmsize, 16);
    }

    #[test]
    fn apply_overrides_inserts_each_sym() {
        let globals = [entry("g0", 8, 3), entry("g1", 4, 2)];
        let layout = compute_user_data_globals_layout(&globals, 0x1_0000_8000);
        let mut sym_table = SymTable::new();
        apply_user_data_global_overrides(&layout, &mut sym_table);
        assert_eq!(sym_table.get("g0"), Some(&0x1_0000_8000));
        assert_eq!(sym_table.get("g1"), Some(&0x1_0000_8008));
    }

    #[test]
    fn extra_defined_syms_collects_names() {
        let globals = [entry("foo", 8, 3), entry("bar", 4, 2)];
        let names = user_data_globals_extra_defined_syms(&globals);
        assert!(names.contains("foo"));
        assert!(names.contains("bar"));
        assert_eq!(names.len(), 2);
    }
}
