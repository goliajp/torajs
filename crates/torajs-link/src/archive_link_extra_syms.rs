//! Worklist `extra_defined_syms` collector — centralizes the 8
//! per-emit-module `*_extra_defined_syms` calls so
//! `compute_required_members` ignores those sym names instead of
//! flagging them `UnresolvedExterns`. Extracted from `archive_link.rs`
//! to keep that file under the 500-prod-LOC file-size hard limit
//! (`rules/common/file-size.md`); pure mechanical pull, no semantic
//! change.

use std::collections::{BTreeMap, BTreeSet};

use torajs_codegen::CompiledFunction;
use torajs_codegen::id_hash::HashSet;

use crate::class_name_table_layout::class_name_table_extra_defined_syms;
use crate::exec::LinkConfig;
use crate::fn_addr_syms::fn_addr_alias_index_bytes;
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
pub(crate) fn collect_extra_defined_syms(cfg: &LinkConfig) -> ExtraDefined {
    let mut extra = user_strings_extra_defined_syms(&cfg.strings);
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
    ExtraDefined {
        names: extra,
        fn_count: cfg.funcs.len(),
    }
}

/// The names the link defines itself.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExtraDefined {
    /// every one but the fn-address aliases
    pub names: BTreeSet<String>,
    /// both fn-address aliases of every user fn index below this; they
    /// are recognized rather than listed, since a list is two strings
    /// per fn, declarations included, built again on every walk
    pub fn_count: usize,
}

impl ExtraDefined {
    fn is_fn_alias(&self, name: &[u8]) -> bool {
        fn_addr_alias_index_bytes(name).is_some_and(|i| i < self.fn_count)
    }

    /// These names and the user fns' own, as one membership test.
    pub(crate) fn with_user_fns<'a>(&'a self, funcs: &'a [CompiledFunction]) -> UserDefined<'a> {
        let names = funcs
            .iter()
            .map(|f| f.name.as_bytes())
            .chain(self.names.iter().map(String::as_bytes))
            .collect();
        UserDefined { names, extra: self }
    }
}

/// A name a user fn or the link defines: a member's reference to it
/// never pulls the archive's copy.
pub(crate) struct UserDefined<'a> {
    names: HashSet<&'a [u8]>,
    extra: &'a ExtraDefined,
}

impl UserDefined<'_> {
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.contains_bytes(name.as_bytes())
    }

    /// [`Self::contains`] of a name read as bytes.
    pub(crate) fn contains_bytes(&self, name: &[u8]) -> bool {
        self.names.contains(name) || self.extra.is_fn_alias(name)
    }
}

/// The names the member closure meets that the image's symbol index
/// lacks: none of them pulls a member.
pub(crate) struct Foreign<'u, 'a> {
    pub(crate) defined_in_user: &'u UserDefined<'u>,
    pub(crate) visited: HashSet<&'a str>,
    pub(crate) unresolved: BTreeSet<String>,
    pub(crate) dyld_imports: BTreeMap<String, u8>,
}

impl<'a> Foreign<'_, 'a> {
    pub(crate) fn visit(&mut self, name: &'a str) {
        if !self.visited.insert(name) || self.defined_in_user.contains(name) {
            return;
        }
        // SD-1 / SD-4b: dyld-class externs (libSystem or libcurl) bind
        // through `LC_LOAD_DYLIB`, not through any archive's
        // defined-extern set. Classify them out of `unresolved` so the
        // link can proceed; record the resolving dylib's `lib_ordinal`
        // so downstream encoders know which LC_LOAD_DYLIB to bind
        // against.
        if let Some(lib) = crate::dyld_syms::dyld_lib_for(name) {
            self.dyld_imports.insert(name.to_string(), lib.ordinal());
        } else {
            self.unresolved.insert(name.to_string());
        }
    }
}
