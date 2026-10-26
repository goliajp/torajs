//! The shapes one elide probe assumes: the user fn list with the
//! chosen sites patched, columns dropped and stubs appended.

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;

use crate::dead_strip_elide_columns::{Columns, without_columns};
use crate::dead_strip_elide_patch::{patch_site, site_drops_user_ref};
use crate::exec::{LinkConfig, UserClassLayoutEntry, UserFnNameEntry};
use crate::user_gc;

/// What the judgment assumed: the user fn list with the chosen
/// shapes applied, and the class-layout table with the twin column
/// dropped when that survived (`None` = the caller's table).
pub(crate) struct Assumed {
    pub(crate) funcs: Vec<CompiledFunction>,
    pub(crate) class_layouts: Option<Vec<UserClassLayoutEntry>>,
    /// The fn-name registry with the rows of fns the re-rooting
    /// stripped dropped (`None` = the caller's).
    pub(crate) fn_name_globals: Option<Vec<UserFnNameEntry>>,
}

/// The user fn list with the chosen shapes assumed: elided sites
/// patched (bytes + relocs removed), the dropped columns blanked, fns
/// the patched-away fn-address sites and the blanked columns
/// orphaned stripped, applied stubs appended.
pub(crate) fn assume(
    cfg: &LinkConfig,
    elided: &[bool],
    applied: &[bool],
    dropped: Columns,
    diag: bool,
) -> Result<Assumed, String> {
    let mut funcs = cfg.funcs.clone();
    let mut user_ref_dropped = false;
    for (site, &e) in cfg.elidable_sites.iter().zip(elided) {
        if e {
            user_ref_dropped |= site_drops_user_ref(&funcs, site);
            patch_site(&mut funcs, site)?;
        }
    }
    let class_layouts = dropped
        .any()
        .then(|| without_columns(&cfg.class_layouts, dropped));
    let mut fn_name_globals = None;
    if user_ref_dropped || dropped.any() {
        // The caller ran user-gc over the unpatched list; an adapter
        // whose mints are all gone (or a twin whose row no longer
        // names it, or the class prologue whose one call was assumed
        // away — r505) is dead only now, and its relocs are exactly
        // the seeds the guard must not see.
        let roots = user_gc::table_root_fids_with(
            &cfg.vtable_globals,
            class_layouts.as_deref().unwrap_or(&cfg.class_layouts),
        );
        if diag {
            let named: Vec<&str> = roots
                .iter()
                .filter_map(|&i| funcs.get(i).map(|f| f.name.as_str()))
                .collect();
            eprintln!(
                "elide-probe: dropped twin={} adapter={} table roots: {}",
                dropped.twin,
                dropped.adapter,
                named.join(" ")
            );
        }
        user_gc::strip_with_roots(&mut funcs, &cfg.entry, &roots, diag);
        let mut rows = cfg.fn_name_globals.clone();
        user_gc::drop_stripped_fn_name_rows(&mut rows, &funcs);
        fn_name_globals = Some(rows);
    }
    for (stub, &a) in cfg.guarded_stubs.iter().zip(applied) {
        if a {
            funcs.push(CompiledFunction {
                name: stub.sym.clone(),
                bytes: stub.bytes.clone(),
                relocs: stub.relocs.clone(),
                frame: FrameLayout::leaf_no_spill(),
            });
        }
    }
    Ok(Assumed {
        funcs,
        class_layouts,
        fn_name_globals,
    })
}
