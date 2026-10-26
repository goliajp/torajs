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

/// The column tables a probe assumed (`None` = the caller's).
type Tables = (
    Option<Vec<UserClassLayoutEntry>>,
    Option<Vec<UserFnNameEntry>>,
);

/// The user fn list the probes of one judgment rewrite. It is copied
/// from the caller's once; each later probe puts back only the code
/// and relocs of the fns the previous one patched or stripped (the
/// only parts either touches), and drops its stubs.
pub(crate) struct Working {
    pub(crate) funcs: Vec<CompiledFunction>,
    /// indices into `funcs` whose code or relocs differ from the
    /// caller's list
    changed: Vec<usize>,
}

impl Working {
    pub(crate) fn new(cfg: &LinkConfig) -> Self {
        Working {
            funcs: cfg.funcs.clone(),
            changed: Vec::new(),
        }
    }

    /// `funcs` with the chosen shapes assumed: elided sites patched
    /// (bytes + relocs removed), fns the patched-away fn-address sites
    /// and the blanked columns orphaned stripped, applied stubs
    /// appended; answers the tables with the dropped columns blanked.
    pub(crate) fn assume(
        &mut self,
        cfg: &LinkConfig,
        elided: &[bool],
        applied: &[bool],
        dropped: Columns,
        diag: bool,
    ) -> Result<Tables, String> {
        let funcs = &mut self.funcs;
        funcs.truncate(cfg.funcs.len());
        // every site of one fn records it
        self.changed.sort_unstable();
        self.changed.dedup();
        for i in self.changed.drain(..) {
            funcs[i].bytes.clone_from(&cfg.funcs[i].bytes);
            funcs[i].relocs.clone_from(&cfg.funcs[i].relocs);
        }
        let mut user_ref_dropped = false;
        for (site, &e) in cfg.elidable_sites.iter().zip(elided) {
            if e {
                user_ref_dropped |= site_drops_user_ref(funcs, site);
                self.changed.push(patch_site(funcs, site)?);
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
            let live = user_gc::strip_with_roots(funcs, &cfg.entry, &roots, diag);
            // a declaration has no code to strip
            let stripped =
                (live.iter().enumerate()).filter(|&(i, l)| !*l && !cfg.funcs[i].bytes.is_empty());
            self.changed.extend(stripped.map(|l| l.0));
            let mut rows = cfg.fn_name_globals.clone();
            user_gc::drop_stripped_fn_name_rows(&mut rows, funcs);
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
        Ok((class_layouts, fn_name_globals))
    }

    /// The final shape: [`Self::assume`] without diag, the list
    /// handed over.
    pub(crate) fn finish(
        mut self,
        cfg: &LinkConfig,
        elided: &[bool],
        applied: &[bool],
        dropped: Columns,
    ) -> Result<Assumed, String> {
        let (class_layouts, fn_name_globals) = self.assume(cfg, elided, applied, dropped, false)?;
        Ok(Assumed {
            funcs: self.funcs,
            class_layouts,
            fn_name_globals,
        })
    }
}
