//! What a repeat visit of an already-walked path remembers — split
//! from `resolve_imports` when the 426 knives pushed it past the
//! 200-line limit. The parent drives the BFS; this file answers how
//! a request aligns itself to the walks that came before it (the
//! dyn-namespace field shortcut, the mangle-memory realignment) and
//! what a namespace walk leaves behind for the next one.

use crate::ast::{Ast, Stmt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::resolve_helpers::NsAccum;
use super::{NamedImport, default_binding};

/// The pre-filter half: a dyn-import namespace revisiting a path
/// whose fields are already known inlines them without re-walking,
/// and a repeat default request binds by reference (see
/// `default_binding::shortcut_repeat_default`).
#[allow(clippy::too_many_arguments)]
pub(super) fn align_repeat_request(
    ast: &mut Ast,
    target_path: &Path,
    default_alias: &mut Option<String>,
    namespace_alias: &mut Option<String>,
    ns_fields_by_path: &HashMap<PathBuf, Vec<(String, String)>>,
    dyn_ns_inline: &mut Vec<(String, Vec<(String, String)>)>,
    default_bound_as: &HashMap<PathBuf, String>,
    injections_by_path: &mut HashMap<PathBuf, Vec<Stmt>>,
) {
    if let Some(ns) = namespace_alias
        .as_ref()
        .filter(|n| n.starts_with("__dyn_ns_"))
        && let Some(fields) = ns_fields_by_path.get(target_path)
    {
        dyn_ns_inline.push((ns.clone(), fields.clone()));
        *namespace_alias = None;
    }
    default_binding::shortcut_repeat_default(
        ast,
        target_path,
        default_alias,
        default_bound_as,
        injections_by_path,
    );
}

/// A named request that ALIASES its binding follows this path's
/// mangle memory: the source-side decl an earlier walk renamed
/// re-mangles to the same spelling, and the alias is what the
/// importer sees anyway (the `__reex_` fetches ride this). A
/// plain-spelling request keeps the orig — the importer wants the
/// very name the mangle took away, and the census's loud reject
/// stays right for it.
pub(super) fn realign_named_to_mangles(
    named: &mut [NamedImport],
    prior: Option<&HashMap<String, String>>,
) {
    let Some(prior) = prior else { return };
    for (orig, alias) in named.iter_mut() {
        if let Some(m) = prior.get(orig)
            && alias.as_ref().is_some_and(|a| a != orig)
        {
            *orig = m.clone();
        }
    }
}

/// For P13-S2 namespace import: make sure the alias owns an
/// accumulator (the collector of every value-export injected) and
/// answer where THIS lib's own contribution starts, so the dyn-ns
/// revisit shortcut still records per-path fields.
pub(super) fn open_ns_walk(
    namespace_alias: &Option<String>,
    ns_accums: &mut HashMap<String, NsAccum>,
    ns_order: &mut Vec<String>,
) -> usize {
    let Some(alias) = namespace_alias else {
        return 0;
    };
    if !ns_accums.contains_key(alias) {
        ns_order.push(alias.clone());
        ns_accums.insert(alias.clone(), NsAccum::new(alias.clone()));
    }
    ns_accums[alias].fields().len()
}

/// The post-walk half: settle the repeat side of the ns `default`
/// field (the binding already exists, only the claim is owed) and
/// snapshot this walk's fields so a dyn revisit can inline them —
/// claim before snapshot, so the snapshot carries the field.
pub(super) fn record_ns_walk(
    namespace_alias: &Option<String>,
    repeat_default_field: Option<String>,
    ns_accums: &mut HashMap<String, NsAccum>,
    first_field: usize,
    target_path: &Path,
    ns_fields_by_path: &mut HashMap<PathBuf, Vec<(String, String)>>,
) {
    let Some(alias) = namespace_alias else { return };
    if let Some(binding) = repeat_default_field
        && let Some(accum) = ns_accums.get_mut(alias.as_str())
    {
        accum.claim("default", &binding);
    }
    let fields = ns_accums[alias].fields()[first_field..].to_vec();
    ns_fields_by_path.insert(target_path.to_path_buf(), fields);
}
