//! The config the link lays out once the dead-strip pre-pass has run
//! over the caller's: the user fns and tables it rewrote in place of
//! the caller's.

use torajs_codegen::CompiledFunction;

use crate::archives_merge::MergedArchives;
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_repack::{DeadRows, StrippedMembers};
use crate::exec::{LinkConfig, UserClassLayoutEntry, UserFnNameEntry};
use crate::layout_types::ArchiveLayoutError;

/// S2 dead-strip blade 2b (RFC 20260824-s2-dead-strip) — opt-in
/// input-normalization pre-pass: rewrite the archives with dead
/// `__text` atoms dropped (and, r499, the user fns with elided drain
/// sites patched), then link the rewritten inputs through the
/// unchanged pipeline, plus the defined externs it dropped (for
/// `LC_SYMTAB`) and the rewritten member bodies, which the caller
/// swaps into `merged` — the returned config's `archives` stay the
/// unstripped inputs. `None` = link the caller's config as is.
pub(crate) fn dead_strip_prepass<'a>(
    cfg: &LinkConfig,
    merged: &MergedArchives<'a>,
    cache: &mut ReachCache<'a>,
) -> Result<Option<(LinkConfig, DeadRows, StrippedMembers)>, ArchiveLayoutError> {
    let enabled = match std::env::var_os("TORAJS_LINK_DEADSTRIP") {
        Some(v) if v == "0" => false,
        Some(_) => true,
        None => cfg.dead_strip,
    };
    if !enabled {
        return Ok(None);
    }
    let out = crate::dead_strip_repack::strip_archives(cfg, merged, cache)
        .map_err(ArchiveLayoutError::DeadStrip)?;
    if out.members.is_empty()
        && out.funcs.is_none()
        && out.class_layouts.is_none()
        && out.fn_name_globals.is_none()
    {
        return Ok(None);
    }
    let stripped = rewritten_config(cfg, out.funcs, out.class_layouts, out.fn_name_globals);
    Ok(Some((stripped, out.dead_rows, out.members)))
}

/// `cfg` with the user fns and tables the pre-pass rewrote (`None`:
/// unchanged). The other fields are cloned one by one: cloning the
/// whole config would copy the fns only to drop them.
pub(crate) fn rewritten_config(
    cfg: &LinkConfig,
    funcs: Option<Vec<CompiledFunction>>,
    class_layouts: Option<Vec<UserClassLayoutEntry>>,
    fn_name_globals: Option<Vec<UserFnNameEntry>>,
) -> LinkConfig {
    let LinkConfig {
        funcs: cfg_funcs,
        user_loops,
        entry,
        sym_table,
        codesign_ident,
        dead_strip,
        strip_member_symbols,
        elidable_sites,
        guarded_stubs,
        image,
        strings,
        data_globals,
        vtable_globals,
        class_layouts: cfg_class_layouts,
        force_emit_class_layouts_globals,
        force_emit_fn_name_globals,
        fn_name_globals: cfg_fn_name_globals,
        class_names,
        force_emit_class_names_globals,
        baked_regex_entries,
    } = cfg;
    LinkConfig {
        funcs: funcs.unwrap_or_else(|| cfg_funcs.clone()),
        user_loops: user_loops.clone(),
        entry: entry.clone(),
        sym_table: sym_table.clone(),
        codesign_ident: codesign_ident.clone(),
        dead_strip: *dead_strip,
        strip_member_symbols: *strip_member_symbols,
        elidable_sites: elidable_sites.clone(),
        guarded_stubs: guarded_stubs.clone(),
        image: image.clone(),
        strings: strings.clone(),
        data_globals: data_globals.clone(),
        vtable_globals: vtable_globals.clone(),
        class_layouts: class_layouts.unwrap_or_else(|| cfg_class_layouts.clone()),
        force_emit_class_layouts_globals: *force_emit_class_layouts_globals,
        force_emit_fn_name_globals: *force_emit_fn_name_globals,
        fn_name_globals: fn_name_globals.unwrap_or_else(|| cfg_fn_name_globals.clone()),
        class_names: class_names.clone(),
        force_emit_class_names_globals: *force_emit_class_names_globals,
        baked_regex_entries: baked_regex_entries.clone(),
    }
}
