//! The config the link lays out once the dead-strip pre-pass has run
//! over the caller's: the user fns and tables it rewrote in place of
//! the caller's.

use std::borrow::Cow;

use torajs_codegen::CompiledFunction;

use crate::archives_merge::{MergedArchives, RequiredMembers};
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_repack::{DeadRows, StripOutput, StrippedMembers, install_cuts};
use crate::exec::{LinkConfig, UserClassLayoutEntry, UserClassNameEntry, UserFnNameEntry};
use crate::layout_types::ArchiveLayoutError;
use crate::phase_timer::PhaseTimer;

/// S2 dead-strip blade 2b (RFC 20260824-s2-dead-strip) — opt-in
/// input-normalization pre-pass: rewrite the archives with dead
/// `__text` atoms dropped (and, r499, the user fns with elided drain
/// sites patched), then link the rewritten inputs through the
/// unchanged pipeline, plus the defined externs it dropped (for
/// `LC_SYMTAB`) and the rewritten member bodies, which the caller
/// swaps into `merged` — the returned config's `archives` stay the
/// unstripped inputs. `None` = link the caller's config as is.
fn dead_strip_prepass<'a>(
    cfg: &LinkConfig,
    merged: &MergedArchives<'a>,
    cache: &mut ReachCache<'a>,
) -> Result<Option<StripOutput>, ArchiveLayoutError> {
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
    let unchanged = out.members.is_empty()
        && out.funcs.is_none()
        && out.class_layouts.is_none()
        && out.fn_name_globals.is_none();
    Ok((!unchanged).then_some(out))
}

/// The config the link lays out, with the dead rows the pre-pass left
/// and the member closure when it is already known.
pub(crate) struct Stripped<'c> {
    pub(crate) cfg: Cow<'c, LinkConfig>,
    pub(crate) dead_rows: DeadRows,
    pub(crate) required: Option<RequiredMembers>,
}

/// Run the dead-strip pre-pass over `cfg`, install its cuts in
/// `merged`, and narrow the forced tables to what the cut archives
/// still read; the parts taken out of an owned config go to `retired`.
pub(crate) fn strip_inputs<'a, 'c>(
    cfg: Cow<'c, LinkConfig>,
    merged: &MergedArchives<'a>,
    cache: &mut ReachCache<'a>,
    retired: &mut Retired,
    t: &mut PhaseTimer,
) -> Result<Stripped<'c>, ArchiveLayoutError> {
    let Some(out) = dead_strip_prepass(&cfg, merged, cache)? else {
        t.tick("dead_strip_prepass");
        let dead_rows = DeadRows::new();
        return Ok(Stripped {
            cfg,
            dead_rows,
            required: None,
        });
    };
    let (mut cfg, dead_rows, members) = rewritten_config(cfg, out, retired);
    t.tick("dead_strip_prepass");
    install_cuts(merged, members)?;
    t.tick("install_cuts");
    // r499 — with the archives stripped, a forced table global whose
    // readers all died drops, and an empty `__DATA_CONST` with it
    // (module doc in force_emit_derive).
    let required = crate::force_emit_derive::derive_force_emit(&mut cfg, merged, cache, retired)?;
    t.tick("derive_force_emit");
    let cfg = Cow::Owned(cfg);
    Ok(Stripped {
        cfg,
        dead_rows,
        required,
    })
}

/// The runtime image, held apart from `cfg`, which the pre-pass may
/// rewrite; nothing past the merge reads `cfg.image`.
pub(crate) fn detach_image<'c>(cfg: &mut Cow<'c, LinkConfig>) -> Cow<'c, [u8]> {
    match cfg {
        Cow::Borrowed(c) => {
            let c: &'c LinkConfig = c;
            Cow::Borrowed(&c.image)
        }
        Cow::Owned(c) => std::mem::take(&mut c.image),
    }
}

/// What rewriting a config in place took out of it, held for the
/// caller to free, or to leave to the exit of a process that is about
/// to end (a copied config was never freed there either).
#[derive(Default)]
pub(crate) struct Retired {
    pub(crate) funcs: Vec<Vec<CompiledFunction>>,
    pub(crate) class_layouts: Vec<Vec<UserClassLayoutEntry>>,
    pub(crate) fn_name_globals: Vec<Vec<UserFnNameEntry>>,
    pub(crate) class_names: Vec<Vec<UserClassNameEntry>>,
}

/// `cfg` with the user fns and tables the pre-pass rewrote, plus the
/// pre-pass's dead rows and member cuts. An owned config is rewritten
/// in place, the parts it replaces going to `retired`; a borrowed one
/// is copied field by field, each rewritten field taken from `out`
/// rather than copied only to be replaced.
fn rewritten_config(
    cfg: Cow<'_, LinkConfig>,
    out: StripOutput,
    retired: &mut Retired,
) -> (LinkConfig, DeadRows, StrippedMembers) {
    let StripOutput {
        members,
        funcs,
        class_layouts,
        fn_name_globals,
        dead_rows,
    } = out;
    let cfg = match cfg {
        Cow::Owned(mut c) => {
            if let Some(f) = funcs {
                retired.funcs.push(std::mem::replace(&mut c.funcs, f));
            }
            if let Some(l) = class_layouts {
                (retired.class_layouts).push(std::mem::replace(&mut c.class_layouts, l));
            }
            if let Some(g) = fn_name_globals {
                (retired.fn_name_globals).push(std::mem::replace(&mut c.fn_name_globals, g));
            }
            c
        }
        Cow::Borrowed(c) => copied_config(c, funcs, class_layouts, fn_name_globals),
    };
    (cfg, dead_rows, members)
}

/// A copy of `cfg` with the given fields in place of its own.
fn copied_config(
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
