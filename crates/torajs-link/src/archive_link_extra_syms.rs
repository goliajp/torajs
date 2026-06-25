//! Worklist `extra_defined_syms` collector — centralizes the 8
//! per-emit-module `*_extra_defined_syms` calls so
//! `compute_required_members` ignores those sym names instead of
//! flagging them `UnresolvedExterns`. Extracted from `archive_link.rs`
//! to keep that file under the 500-prod-LOC file-size hard limit
//! (`rules/common/file-size.md`); pure mechanical pull, no semantic
//! change.

use std::collections::BTreeSet;

use crate::class_name_table_layout::class_name_table_extra_defined_syms;
use crate::exec::LinkConfig;
use crate::fn_addr_syms::fn_addr_extra_defined_syms;
use crate::fn_name_table_layout::fn_name_table_extra_defined_syms;
use crate::user_class_layouts_layout::user_class_layouts_extra_defined_syms;
use crate::user_data_globals_layout::user_data_globals_extra_defined_syms;
use crate::user_regex_baked_layout::user_regex_baked_extra_defined_syms;
use crate::user_strings_layout::user_strings_extra_defined_syms;
use crate::user_vtables_layout::user_vtables_extra_defined_syms;

/// Build the `extra_defined_syms` set fed into
/// `compute_required_members`. Each user-data emit module exports its
/// own `*_extra_defined_syms` (string-keyed sym names that the worklist
/// closure should treat as defined-by-emit instead of unresolved).
pub(crate) fn collect_extra_defined_syms(cfg: &LinkConfig) -> BTreeSet<String> {
    let mut extra = user_strings_extra_defined_syms(&cfg.strings);
    extra.extend(fn_addr_extra_defined_syms(&cfg.funcs));
    extra.extend(user_data_globals_extra_defined_syms(&cfg.data_globals));
    extra.extend(user_vtables_extra_defined_syms(&cfg.vtable_globals));
    // e8-2c — `tr build` sets force_emit so the libtorajs_cycle.a
    // extern resolves even on class-free programs (probes stay false).
    extra.extend(user_class_layouts_extra_defined_syms(
        &cfg.class_layouts,
        cfg.force_emit_class_layouts_globals,
    ));
    extra.extend(fn_name_table_extra_defined_syms(
        &cfg.fn_name_globals,
        cfg.force_emit_fn_name_globals,
    ));
    extra.extend(class_name_table_extra_defined_syms(
        &cfg.class_names,
        cfg.force_emit_class_names_globals,
    ));
    // C-5c.2d — outer `___torajs_baked_regex_<i>` syms close the worklist
    // for `RelocKind::Page21/PageOff12 { target_sym }` against AOT-baked
    // DFA meta blocks. Inner `states_sym` stays internal (never reloc'd
    // from user code; reader goes through chain-LC rebased
    // `BakedDfaMeta::states_ptr`).
    extra.extend(user_regex_baked_extra_defined_syms(
        &cfg.baked_regex_entries,
    ));
    extra
}
