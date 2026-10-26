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
//! answer — and the same answer retires a POPULATED table whose only
//! readers died (r501: the rows are runtime-read, never user-read):
//! a member's dead atoms had their undef rows neutered (`N_EXT`
//! cleared), so "does any required member still carry an
//! `N_EXT` undef for the table's symbols" is "does live runtime code
//! read the table". A flag whose symbols nobody references drops;
//! the layout's existing `has_data_const` gate then omits the segment
//! (the pre-e8 shape every probe exercises). Members the pass left
//! untouched keep every undef they came with, which only ever keeps a
//! flag — the safe direction.

use std::collections::BTreeSet;

use crate::archive_link_extra_syms::collect_extra_defined_syms;
use crate::archives_merge::{MergedArchives, RequiredMembers, compute_required_members_cached};
use crate::dead_strip_config::Retired;
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_reach_table::ImportRef;
use crate::exec::LinkConfig;
use crate::layout_types::ArchiveLayoutError;

/// Mach-O names of each forced table's globals.
const CLASS_LAYOUTS_SYMS: [&str; 2] = ["___torajs_class_layouts", "___torajs_n_class_layouts"];
const FN_NAME_SYMS: [&str; 2] = ["___torajs_fn_name_table", "___torajs_fn_name_table_count"];
const CLASS_NAMES_SYMS: [&str; 2] = ["___torajs_class_name_table", "___torajs_n_class_names"];

/// Drop every table whose symbols no required member references —
/// a forced flag turns off, and (r501, RFC 20260824-s2-5 刀 4 A7) a
/// populated registry empties: a fn-name / class-name / class-layout
/// table is read only by runtime text (`__torajs_fn_print_inline`,
/// structmeta, the cycle collector), so with every reader stripped
/// the rows are a `__DATA_CONST` page nobody will open (a program with
/// one named closure carried a one-row fn-name table and, through it,
/// a 16 KiB segment). `cfg` is narrowed in place, the emptied tables
/// going to `retired`. Answers the member
/// closure it walked when that is also the closure of the narrowed
/// `cfg`.
pub(crate) fn derive_force_emit<'a>(
    cfg: &mut LinkConfig,
    merged: &MergedArchives<'a>,
    cache: &mut ReachCache<'a>,
    retired: &mut Retired,
) -> Result<Option<RequiredMembers>, ArchiveLayoutError> {
    let set = table_state(cfg);
    if set == (false, false, false) {
        return Ok(None);
    }
    let extra = collect_extra_defined_syms(cfg);
    let required = compute_required_members_cached(&cfg.funcs, merged, &extra, cache)
        .map_err(ArchiveLayoutError::Link)?;
    // the table symbols the required members import, each looked for
    // as the member tables name it: by its row in the image's symbol
    // index when a runtime member defines it, else by name
    let image = &merged.image;
    let table_syms: Vec<&str> = [CLASS_LAYOUTS_SYMS, FN_NAME_SYMS, CLASS_NAMES_SYMS].concat();
    let rows: Vec<(u32, &str)> = table_syms
        .iter()
        .filter_map(|&n| Some((image.find_symbol(n)? as u32, n)))
        .collect();
    let mut undefs: BTreeSet<&str> = BTreeSet::new();
    for &(a, m) in &required.members {
        let imports = cache.member_imports(merged, (a, m)).map_err(|err| {
            ArchiveLayoutError::MemberSymtab {
                archive_idx: a,
                member_idx: m,
                err,
            }
        })?;
        for import in imports.iter() {
            match import.to {
                ImportRef::Foreign(name) => {
                    if table_syms.contains(&name) {
                        undefs.insert(name);
                    }
                }
                ImportRef::Sym(i) => {
                    if let Some(&(_, n)) = rows.iter().find(|r| r.0 == i) {
                        undefs.insert(n);
                    }
                }
            }
        }
    }
    let keep = derive_flags(set, &undefs);
    if keep == set {
        return Ok(Some(required));
    }
    if !keep.0 {
        cfg.force_emit_class_layouts_globals = false;
        (retired.class_layouts).push(std::mem::take(&mut cfg.class_layouts));
    }
    if !keep.1 {
        cfg.force_emit_fn_name_globals = false;
        (retired.fn_name_globals).push(std::mem::take(&mut cfg.fn_name_globals));
    }
    if !keep.2 {
        cfg.force_emit_class_names_globals = false;
        (retired.class_names).push(std::mem::take(&mut cfg.class_names));
    }
    // the fns are the same; the closure is too unless the dropped
    // tables changed the link-defined names it skips
    Ok((collect_extra_defined_syms(cfg) == extra).then_some(required))
}

/// Which of the three tables the config would emit at all (forced or
/// populated) — the set the derivation may narrow.
fn table_state(cfg: &LinkConfig) -> (bool, bool, bool) {
    (
        cfg.force_emit_class_layouts_globals || !cfg.class_layouts.is_empty(),
        cfg.force_emit_fn_name_globals || !cfg.fn_name_globals.is_empty(),
        cfg.force_emit_class_names_globals || !cfg.class_names.is_empty(),
    )
}

/// A table survives iff one of its symbols is among the undefs the
/// required members still carry.
fn derive_flags(set: (bool, bool, bool), undefs: &BTreeSet<&str>) -> (bool, bool, bool) {
    let referenced = |syms: &[&str]| syms.iter().any(|s| undefs.contains(s));
    (
        set.0 && referenced(&CLASS_LAYOUTS_SYMS),
        set.1 && referenced(&FN_NAME_SYMS),
        set.2 && referenced(&CLASS_NAMES_SYMS),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set<'a>(names: &[&'a str]) -> BTreeSet<&'a str> {
        names.iter().copied().collect()
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
