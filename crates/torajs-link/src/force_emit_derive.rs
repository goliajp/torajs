//! r499 — derive the `force_emit_*` table globals from what the
//! stripped archives still reference.
//!
//! `tr build` sets the three `force_emit_*` flags unconditionally:
//! `libtorajs_cycle.a` / `libtorajs_fnname.a` / `libtorajs_structmeta.a`
//! reference the link-emitted class-layout / fn-name / class-name
//! tables from their own text, so a program with no classes and no
//! named fns still had to define the zero-count globals for the
//! archives to resolve. Each forced table is a few zero bytes — but
//! any of them turns `__DATA_CONST` on, and an otherwise empty
//! `__DATA_CONST` is a whole page of the artifact (16 KiB of a 51 KiB
//! empty program, r499).
//!
//! Once the dead-strip pre-pass has run, the question has an exact
//! answer: a member's dead atoms had their undef rows neutered
//! (`N_EXT` cleared), so "does any required member still carry an
//! `N_EXT` undef for the table's symbols" is "does live runtime code
//! read the table". A flag whose symbols nobody references drops;
//! the layout's existing `has_data_const` gate then omits the segment
//! (the pre-e8 shape every probe exercises). Members the pass left
//! untouched keep every undef they came with, which only ever keeps a
//! flag — the safe direction.

use std::collections::BTreeSet;

use crate::archive_link_extra_syms::collect_extra_defined_syms;
use crate::archives_merge::{MergedArchives, compute_required_members, parse_member_undef_externs};
use crate::exec::LinkConfig;
use crate::layout_types::ArchiveLayoutError;

/// Mach-O names of each forced table's globals.
const CLASS_LAYOUTS_SYMS: [&str; 2] = ["___torajs_class_layouts", "___torajs_n_class_layouts"];
const FN_NAME_SYMS: [&str; 2] = ["___torajs_fn_name_table", "___torajs_fn_name_table_count"];
const CLASS_NAMES_SYMS: [&str; 2] = ["___torajs_class_name_table", "___torajs_n_class_names"];

/// Drop every forced flag whose symbols no required member
/// references. `None` when nothing changes (no flag set, or every
/// set flag is still referenced).
pub(crate) fn derive_force_emit(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
) -> Result<Option<LinkConfig>, ArchiveLayoutError> {
    if !(cfg.force_emit_class_layouts_globals
        || cfg.force_emit_fn_name_globals
        || cfg.force_emit_class_names_globals)
    {
        return Ok(None);
    }
    let extra = collect_extra_defined_syms(cfg);
    let required =
        compute_required_members(&cfg.funcs, merged, &extra).map_err(ArchiveLayoutError::Link)?;
    let mut undefs: BTreeSet<String> = BTreeSet::new();
    for &(a, m) in &required.members {
        let member = &merged.per_archive_members[a][m];
        let names =
            parse_member_undef_externs(member).map_err(|err| ArchiveLayoutError::MemberSymtab {
                archive_idx: a,
                member_idx: m,
                err,
            })?;
        undefs.extend(names);
    }
    let flags = derive_flags(
        (
            cfg.force_emit_class_layouts_globals,
            cfg.force_emit_fn_name_globals,
            cfg.force_emit_class_names_globals,
        ),
        &undefs,
    );
    if flags
        == (
            cfg.force_emit_class_layouts_globals,
            cfg.force_emit_fn_name_globals,
            cfg.force_emit_class_names_globals,
        )
    {
        return Ok(None);
    }
    Ok(Some(LinkConfig {
        force_emit_class_layouts_globals: flags.0,
        force_emit_fn_name_globals: flags.1,
        force_emit_class_names_globals: flags.2,
        ..cfg.clone()
    }))
}

/// A set flag survives iff one of its table's symbols is among the
/// undefs the required members still carry.
fn derive_flags(set: (bool, bool, bool), undefs: &BTreeSet<String>) -> (bool, bool, bool) {
    let referenced = |syms: &[&str]| syms.iter().any(|s| undefs.contains(*s));
    (
        set.0 && referenced(&CLASS_LAYOUTS_SYMS),
        set.1 && referenced(&FN_NAME_SYMS),
        set.2 && referenced(&CLASS_NAMES_SYMS),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn unreferenced_flags_drop_referenced_flags_stay() {
        let undefs = set(&["___torajs_n_class_layouts", "___torajs_print_i64"]);
        assert_eq!(
            derive_flags((true, true, true), &undefs),
            (true, false, false)
        );
    }

    #[test]
    fn either_symbol_of_a_table_keeps_its_flag() {
        assert_eq!(
            derive_flags((true, true, true), &set(&["___torajs_fn_name_table"])),
            (false, true, false)
        );
        assert_eq!(
            derive_flags((true, true, true), &set(&["___torajs_class_name_table"])),
            (false, false, true)
        );
    }

    #[test]
    fn a_clear_flag_never_turns_on() {
        let undefs = set(&["___torajs_n_class_layouts"]);
        assert_eq!(
            derive_flags((false, false, false), &undefs),
            (false, false, false)
        );
    }
}
