//! r499 — link-judged conditional shapes in the user `.o`: a `bl`
//! the dead-strip pre-pass may replace with a fixed instruction
//! ([`ElidableSite`]), and a definition it may add to shadow a
//! runtime member's ([`GuardedStub`]) — each decided by whether the
//! runtime member it guards on has live text once the shape itself
//! is assumed.
//!
//! The shapes exist for two kinds of runtime hook: the synthesized
//! `main` closes with drains (microtask queue, unhandled-rejection
//! sweep, cycle buffer) that are no-ops unless some other live code
//! can feed them, yet each call roots the whole draining machinery;
//! and rc-hit-zero notifies the weak-observer registry through a
//! symbol whose callee already returns at once when no observer was
//! ever registered — a runtime check the linker cannot see, so the
//! reference alone roots the registry and, through it, the generic
//! value-drop world. The caller names the site or symbol and the
//! member whose text is the evidence; this pass answers the question
//! the linker alone can: with the shape assumed, does any text of
//! that member (beyond the entry points listed as unable to matter)
//! still come out live? A feeder always crosses a crate boundary
//! through an extern symbol, so member-text liveness is inline-proof.
//!
//! Judgment is a fix-point: start with every site elided and every
//! stub applied, run reachability, and un-assume each shape whose
//! guard came back live; repeat until nothing changes. Un-assuming
//! only grows the world, so it converges, and every surviving
//! verdict was checked against a world holding everything the
//! artifact will hold. The strip pass then runs over the patched fns.
//!
//! Two guard shapes ([`Guard`]): member text liveness (the drains and
//! the weak hook — every feeder crosses a crate boundary into one
//! member), and named-symbol liveness (r500: a typed kernel's exotic
//! slow path is un-assumable exactly when one of a few `#[inline
//! (never)]` writer entries in its own crate is live — the member
//! itself is live regardless).
//!
//! Two site shapes ([`SiteShape`]): a `bl` replaced by a fixed word
//! (r499), and — r501, RFC 20260824-s2-5 刀 4 A1 — a closure mint's
//! `adrp/add` pair taking its `__boxed_` adapter's address, replaced
//! by `movz Xd, #0` (the no-adapter shape the runtime already answers
//! with a catchable TypeError). The adapter's per-parameter unbox
//! roots the whole any world; its only readers go through one
//! `#[inline(never)]` rc entry, so that symbol's text liveness is the
//! guard. When every mint of an adapter is assumed away the adapter
//! itself has no reference left, and `assume` re-runs the user-fn
//! dead-strip so its relocs stop seeding the member closure.
//!
//! One assumption is a table column rather than a site (r502, 刀 4
//! A8): a class-method row's `__cmany_` twin adapter is read by the
//! runtime through exactly one structmeta entry
//! (`__torajs_struct_method_twin_at`, called only by the register
//! kernel's prototype reification), and the twin's per-member
//! `any_member_get` / `accessor_get` relocs root the any world the
//! way the closure adapter's unbox did. With that entry dead the
//! column bakes 0 and the twins stop being user-gc roots. The reader
//! is a runtime-substrate fact, not per-program policy, so the
//! symbol lives here (as `force_emit_derive` names the table
//! globals).
//!
//! Policy stays in the caller (`torajs-cli` knows which members feed
//! which hook); this file is mechanism only. `TORAJS_LINK_ELIDE=0`
//! disables the pass (A/B pricing); `TORAJS_LINK_ELIDE_DIAG=1`
//! prints each verdict to stderr.

use std::collections::BTreeSet;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;

use crate::archives_merge::{MergedArchives, compute_required_members};
use crate::dead_strip_elide_columns::{Columns, columns_present, without_columns};
use crate::dead_strip_elide_patch::{patch_site, site_drops_user_ref};
use crate::dead_strip_reach::{MemberReach, ReachResult, compute_reachability};
use crate::exec::{LinkConfig, UserClassLayoutEntry, UserFnNameEntry};
use crate::user_gc;

/// Which runtime text is the evidence.
#[derive(Debug, Clone)]
pub enum Guard {
    /// Archive members whose name starts with `prefix`
    /// (`torajs_cycle-` covers every cgu of that crate), ignoring
    /// atoms anchored by a symbol in `ignore` — the member's entry
    /// points that can never matter (a drop, a printer, the hook
    /// being shadowed). An unlisted entry keeps the shape
    /// un-assumed, the safe direction.
    Member { prefix: String, ignore: Vec<String> },
    /// Any live text atom, in any member, anchored by one of these
    /// defined symbols (exact Mach-O names). For a slow path whose
    /// only enablers are a few named entries of an otherwise-live
    /// member.
    Symbols(Vec<String>),
}

impl core::fmt::Display for Guard {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Guard::Member { prefix, .. } => write!(f, "member:{prefix}"),
            Guard::Symbols(syms) => write!(f, "syms:{}", syms.join("|")),
        }
    }
}

/// What a conditional site looks like in the fn's bytes.
#[derive(Debug, Clone)]
pub enum SiteShape {
    /// A `bl` (a `CallSite { Extern }` reloc at `byte_offset` must
    /// exist); `replacement` is the word written over it when elided.
    Call { byte_offset: u32, replacement: u32 },
    /// An `adrp Xd; add Xd, Xd, #lo12` pair at `adrp_offset` whose
    /// `Page21` / `PageOff12` relocs name `target` (a
    /// `__torajs_boxed_<i>` alias). Elided: `movz Xd, #0; nop`, both
    /// relocs dropped, and the alias's fn stripped if nothing else
    /// references it.
    FnAddr { adrp_offset: u32, target: String },
}

impl core::fmt::Display for SiteShape {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SiteShape::Call { byte_offset, .. } => write!(f, "+{byte_offset:#x}"),
            SiteShape::FnAddr {
                adrp_offset,
                target,
            } => write!(f, "+{adrp_offset:#x} fnaddr={target}"),
        }
    }
}

/// One conditional site in user fn `func`.
#[derive(Debug, Clone)]
pub struct ElidableSite {
    pub func: String,
    pub guard: Guard,
    pub shape: SiteShape,
}

/// One conditional definition: a leaf fn named `sym` with body
/// `bytes` (plus `relocs` — a loud-reject stub tail-branches into
/// the landing pad) that, when applied, shadows the archive's
/// definition — user-defined names win symbol resolution, so the
/// member atom loses its in-edges.
#[derive(Debug, Clone)]
pub struct GuardedStub {
    pub sym: String,
    pub bytes: Vec<u8>,
    pub relocs: Vec<torajs_codegen::reloc::Reloc>,
    pub guard: Guard,
}

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

/// Run the fix-point over `cfg.elidable_sites` + `cfg.guarded_stubs`
/// + the twin column; answer the assumed shape (sites rewritten,
/// surviving stubs appended, column dropped) when any of it
/// survived, `None` when nothing changes.
pub(crate) fn judge_and_patch(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
    extra_defined_syms: &BTreeSet<String>,
) -> Result<Option<Assumed>, String> {
    let present = columns_present(&cfg.class_layouts);
    if (cfg.elidable_sites.is_empty() && cfg.guarded_stubs.is_empty() && !present.any())
        || std::env::var_os("TORAJS_LINK_ELIDE").is_some_and(|v| v == "0")
    {
        return Ok(None);
    }
    let diag = std::env::var_os("TORAJS_LINK_ELIDE_DIAG").is_some();
    let twin_guard = Columns::twin_guard();
    let adapter_guard = Columns::adapter_guard();

    let mut elided: Vec<bool> = vec![true; cfg.elidable_sites.len()];
    let mut applied: Vec<bool> = vec![true; cfg.guarded_stubs.len()];
    let mut dropped = present;
    // An applied stub no live atom imports is dead weight (it would
    // only ride along because `___torajs_`-named user fns are always
    // rooted) — and a page-line away from costing a whole page.
    let mut referenced: Vec<bool> = vec![true; cfg.guarded_stubs.len()];
    loop {
        let assumed = assume(cfg, &elided, &applied, dropped, diag)?;
        let required = compute_required_members(&assumed.funcs, merged, extra_defined_syms)
            .map_err(|e| format!("member closure (elide probe): {e:?}"))?;
        let probe_cfg = LinkConfig {
            funcs: assumed.funcs,
            class_layouts: assumed
                .class_layouts
                .unwrap_or_else(|| cfg.class_layouts.clone()),
            fn_name_globals: assumed
                .fn_name_globals
                .unwrap_or_else(|| cfg.fn_name_globals.clone()),
            ..cfg.clone()
        };
        // preds only under diag — `explain_live` walks them.
        let reach = compute_reachability(
            &probe_cfg,
            merged,
            &required,
            extra_defined_syms,
            diag,
            None,
        )?;
        let mut changed = false;
        for (i, site) in cfg.elidable_sites.iter().enumerate() {
            if elided[i] && guard_live(&reach, &site.guard) {
                elided[i] = false;
                changed = true;
                if diag {
                    eprint!("{}", explain_live(&reach, &site.guard));
                }
            }
        }
        for (i, stub) in cfg.guarded_stubs.iter().enumerate() {
            if applied[i] && guard_live(&reach, &stub.guard) {
                applied[i] = false;
                changed = true;
                if diag {
                    eprint!("{}", explain_live(&reach, &stub.guard));
                }
            }
        }
        if dropped.twin && guard_live(&reach, &twin_guard) {
            dropped.twin = false;
            changed = true;
            if diag {
                eprint!("{}", explain_live(&reach, &twin_guard));
            }
        }
        if dropped.adapter && guard_live(&reach, &adapter_guard) {
            dropped.adapter = false;
            changed = true;
            if diag {
                eprint!("{}", explain_live(&reach, &adapter_guard));
            }
        }
        if !changed {
            // A seam is imported either by a live member atom
            // (`user_refs`) or by a user fn's own reloc — the closure
            // env-drop legs (A5) are called from synthesized user fns
            // and never appear in a member's undef table.
            let fn_named = user_fn_reloc_names(&probe_cfg.funcs);
            for (i, stub) in cfg.guarded_stubs.iter().enumerate() {
                referenced[i] = !applied[i]
                    || reach.user_refs.contains(&stub.sym)
                    || fn_named.contains(stub.sym.as_str());
            }
            break;
        }
    }
    for (a, r) in applied.iter_mut().zip(&referenced) {
        *a = *a && *r;
    }

    if diag {
        for (site, &e) in cfg.elidable_sites.iter().zip(&elided) {
            eprintln!(
                "elide: {}{} guard={} -> {}",
                site.func,
                site.shape,
                site.guard,
                if e { "ELIDED" } else { "KEPT" }
            );
        }
        for ((stub, &a), &r) in cfg.guarded_stubs.iter().zip(&applied).zip(&referenced) {
            eprintln!(
                "stub: {} guard={} -> {}",
                stub.sym,
                stub.guard,
                match (a, r) {
                    (true, _) => "APPLIED",
                    (false, false) => "UNREFERENCED",
                    (false, true) => "DROPPED",
                }
            );
        }
        if present.twin {
            eprintln!(
                "twin-column: guard={twin_guard} -> {}",
                if dropped.twin { "DROPPED" } else { "KEPT" }
            );
        }
        if present.adapter {
            eprintln!(
                "adapter-column: guard={adapter_guard} -> {}",
                if dropped.adapter { "DROPPED" } else { "KEPT" }
            );
        }
    }
    if !elided.iter().any(|&e| e) && !applied.iter().any(|&a| a) && !dropped.any() {
        return Ok(None);
    }
    Ok(Some(assume(cfg, &elided, &applied, dropped, false)?))
}

/// The user fn list with the chosen shapes assumed: elided sites
/// patched (bytes + relocs removed), the dropped columns blanked, fns
/// the patched-away fn-address sites and the blanked columns
/// orphaned stripped, applied stubs appended.
fn assume(
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
            cfg,
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

/// Diag — why the guard came back live in THIS probe (the final
/// world no longer shows it: the un-assumed shape has changed what
/// is live). `Symbols`: the why-chain of each; `Member`: the member's
/// live atoms, the reader picks one and re-runs with
/// `TORAJS_LINK_DEADSTRIP_WHY`.
fn explain_live(reach: &ReachResult<'_>, guard: &Guard) -> String {
    match guard {
        Guard::Symbols(syms) => crate::dead_strip_diag::render_why(reach, &syms.join(",")),
        Guard::Member { prefix, .. } => {
            crate::dead_strip_diag::render_live_dump(reach, std::slice::from_ref(prefix))
        }
    }
}

/// Every symbol name a non-empty user fn's relocs carry.
fn user_fn_reloc_names(funcs: &[CompiledFunction]) -> BTreeSet<&str> {
    use torajs_codegen::reloc::{CallTarget, RelocKind};
    funcs
        .iter()
        .filter(|f| !f.bytes.is_empty())
        .flat_map(|f| f.relocs.iter())
        .filter_map(|r| match &r.kind {
            RelocKind::CallSite {
                target: CallTarget::Extern(name),
            } => Some(name.as_str()),
            RelocKind::CallSite { .. } => None,
            RelocKind::Page21 { target_sym }
            | RelocKind::PageOff12 { target_sym }
            | RelocKind::AbsPtr64 { target_sym } => Some(target_sym.as_str()),
        })
        .collect()
}

/// Is the guard's evidence live? `Member`: any text atom of a
/// matching member, other than the atoms anchored by an ignored
/// symbol (members outside the closure are absent from the map, so
/// they answer `false`; a flag-rooted `all_live` text section
/// answers `true` without looking at names). `Symbols`: any live
/// text atom, in any member, anchored by one of the names.
fn guard_live(reach: &ReachResult<'_>, guard: &Guard) -> bool {
    match guard {
        Guard::Member { prefix, ignore } => reach
            .members
            .values()
            .filter(|m| m.member_name.starts_with(prefix))
            .any(|m| {
                m.sects.iter().enumerate().any(|(si, s)| {
                    s.is_text
                        && (s.all_live
                            || s.atoms.iter().zip(&s.live).any(|(&(start, _), &l)| {
                                l && !anchor_named(m, (si + 1) as u8, start, ignore)
                            }))
                })
            }),
        Guard::Symbols(syms) => reach.members.values().any(|m| {
            m.sects.iter().enumerate().any(|(si, s)| {
                s.is_text
                    && s.atoms
                        .iter()
                        .zip(&s.live)
                        .any(|(&(start, _), &l)| l && anchor_named(m, (si + 1) as u8, start, syms))
            })
        }),
    }
}

/// Is the defined symbol anchoring the atom at `start` (the highest
/// `N_SECT` nlist row at or below it in that section) one of
/// `names`? An unanchored atom (leading gap) never is.
fn anchor_named(m: &MemberReach<'_>, n_sect: u8, start: u64, names: &[String]) -> bool {
    use torajs_obj::{N_SECT, N_TYPE};
    m.nlist
        .iter()
        .filter(|n| n.n_type & N_TYPE == N_SECT && n.n_sect == n_sect && n.n_value <= start)
        .max_by_key(|n| n.n_value)
        .is_some_and(|n| names.iter().any(|i| i == n.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

    const NOP: u32 = 0xD503_201F;

    fn fn_with_bl(name: &str, callee: &str) -> CompiledFunction {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xD503_201Fu32.to_le_bytes());
        bytes.extend_from_slice(&0x9400_0000u32.to_le_bytes());
        CompiledFunction {
            name: name.into(),
            bytes,
            relocs: vec![Reloc {
                byte_offset: 4,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern(callee.into()),
                },
            }],
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// `adrp x9; add x9, x9, #0` at 0 against `__torajs_boxed_<i>`.
    fn fn_with_mint(name: &str, i: usize) -> CompiledFunction {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(0x9000_0000u32 | 9).to_le_bytes());
        bytes.extend_from_slice(&(0x9100_0000u32 | (9 << 5) | 9).to_le_bytes());
        let target = format!("__torajs_boxed_{i}");
        CompiledFunction {
            name: name.into(),
            bytes,
            relocs: vec![
                Reloc {
                    byte_offset: 0,
                    kind: RelocKind::Page21 {
                        target_sym: target.clone(),
                    },
                },
                Reloc {
                    byte_offset: 4,
                    kind: RelocKind::PageOff12 { target_sym: target },
                },
            ],
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    fn guard() -> Guard {
        Guard::Member {
            prefix: "torajs_x-".into(),
            ignore: Vec::new(),
        }
    }

    fn site(func: &str, off: u32) -> ElidableSite {
        ElidableSite {
            func: func.into(),
            guard: guard(),
            shape: SiteShape::Call {
                byte_offset: off,
                replacement: NOP,
            },
        }
    }

    fn cfg_with(funcs: Vec<CompiledFunction>, sites: Vec<ElidableSite>) -> LinkConfig {
        LinkConfig {
            funcs,
            entry: "_main_user".into(),
            sym_table: crate::resolve::SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: true,
            strip_member_symbols: false,
            elidable_sites: sites,
            guarded_stubs: vec![GuardedStub {
                sym: "___torajs_hook".into(),
                bytes: 0xD65F_03C0u32.to_le_bytes().to_vec(),
                relocs: Vec::new(),
                guard: guard(),
            }],
            archives: Vec::new(),
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        }
    }

    #[test]
    fn assume_patches_chosen_sites_and_appends_applied_stubs() {
        let cfg = cfg_with(
            vec![fn_with_bl("_main_user", "___torajs_drain")],
            vec![site("_main_user", 4)],
        );
        let kept = assume(&cfg, &[false], &[false], Columns::NONE, false)
            .unwrap()
            .funcs;
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].relocs.len(), 1);
        let taken = assume(&cfg, &[true], &[true], Columns::NONE, false)
            .unwrap()
            .funcs;
        assert_eq!(taken.len(), 2);
        assert!(taken[0].relocs.is_empty());
        assert_eq!(taken[1].name, "___torajs_hook");
        assert!(taken[1].relocs.is_empty());
    }

    #[test]
    fn assumed_mint_strips_the_orphaned_adapter() {
        // main mints adapter 1 (which calls into the any world); a
        // second fn takes the adapter's address the plain way and is
        // itself dead (unreferenced), so it must not keep it.
        let mut adapter = fn_with_bl("__boxed_f", "___torajs_anyv_unbox");
        adapter.name = "__boxed_f".into();
        let funcs = vec![fn_with_mint("_main_user", 1), adapter];
        let mint = ElidableSite {
            func: "_main_user".into(),
            guard: guard(),
            shape: SiteShape::FnAddr {
                adrp_offset: 0,
                target: "__torajs_boxed_1".into(),
            },
        };
        let cfg = cfg_with(funcs, vec![mint]);
        let kept = assume(&cfg, &[false], &[false], Columns::NONE, false)
            .unwrap()
            .funcs;
        assert!(
            !kept[1].bytes.is_empty(),
            "adapter stays while its mint does"
        );
        let taken = assume(&cfg, &[true], &[false], Columns::NONE, false)
            .unwrap()
            .funcs;
        // movz x9, #0 ; nop — relocs gone, adapter emptied.
        assert_eq!(&taken[0].bytes[..4], &(0xD280_0000u32 | 9).to_le_bytes());
        assert_eq!(&taken[0].bytes[4..], &NOP.to_le_bytes());
        assert!(taken[0].relocs.is_empty());
        assert!(taken[1].bytes.is_empty(), "no mint left → adapter stripped");
        assert!(taken[1].relocs.is_empty());
    }

    #[test]
    fn assumed_prologue_call_strips_the_orphaned_callee() {
        // r505 (A12) — main's one `bl` to the class prologue (a user
        // fn, `CallTarget::Func`) is a site; assumed away, the callee
        // has no reference left and the user-gc re-run strips it,
        // relocs and all.
        let mut main = fn_with_bl("_main_user", "unused");
        main.relocs[0].kind = RelocKind::CallSite {
            target: CallTarget::Func(torajs_core::ssa::FuncId(1)),
        };
        let prologue = fn_with_bl("__cprologue", "___torajs_dynobj_alloc");
        let cfg = cfg_with(vec![main, prologue], vec![site("_main_user", 4)]);
        let kept = assume(&cfg, &[false], &[false], Columns::NONE, false)
            .unwrap()
            .funcs;
        assert!(
            !kept[1].bytes.is_empty(),
            "the prologue stays while its call does"
        );
        let taken = assume(&cfg, &[true], &[false], Columns::NONE, false)
            .unwrap()
            .funcs;
        assert_eq!(&taken[0].bytes[4..], &NOP.to_le_bytes());
        assert!(taken[0].relocs.is_empty());
        assert!(
            taken[1].bytes.is_empty(),
            "no call left → prologue stripped"
        );
        assert!(taken[1].relocs.is_empty());
    }

    #[test]
    fn dropped_twin_column_strips_the_twin_it_rooted() {
        use crate::exec::{UserClassLayoutEntry, UserMethodMetaEntry};
        // main; the mono adapter (row 1); the twin adapter (row 2)
        // whose reloc is the any-world seed.
        let mut twin = fn_with_bl("__boxed___cmany_A__m", "___torajs_any_member_get_tag");
        twin.name = "__boxed___cmany_A__m".into();
        let funcs = vec![
            fn_with_bl("_main_user", "___torajs_print_i64"),
            fn_with_bl("__boxed___cm_A__m", "___torajs_anyv_box_from_pair"),
            twin,
        ];
        let mut cfg = cfg_with(funcs, Vec::new());
        cfg.guarded_stubs.clear();
        cfg.class_layouts = vec![UserClassLayoutEntry {
            child_offsets: Vec::new(),
            fields: Vec::new(),
            is_named: true,
            is_generic: false,
            methods: vec![UserMethodMetaEntry {
                name: "m".into(),
                adapter_fn_id: Some(1),
                flags: 0,
                twin_fn_id: Some(2),
            }],
        }];
        assert_eq!(
            columns_present(&cfg.class_layouts),
            Columns {
                twin: true,
                adapter: true
            }
        );
        let kept = assume(&cfg, &[], &[], Columns::NONE, false).unwrap();
        assert!(kept.class_layouts.is_none());
        assert!(!kept.funcs[2].bytes.is_empty(), "the row roots the twin");
        let dropped = assume(
            &cfg,
            &[],
            &[],
            Columns {
                twin: true,
                adapter: false,
            },
            false,
        )
        .unwrap();
        let layouts = dropped.class_layouts.expect("column rewritten");
        assert_eq!(layouts[0].methods[0].twin_fn_id, None);
        assert_eq!(layouts[0].methods[0].adapter_fn_id, Some(1));
        assert!(
            !dropped.funcs[1].bytes.is_empty(),
            "the adapter column stays"
        );
        assert!(
            dropped.funcs[2].bytes.is_empty(),
            "no row names the twin → stripped"
        );
        assert!(
            dropped.funcs[2].relocs.is_empty(),
            "its any-world seed goes with it"
        );
        let both = assume(
            &cfg,
            &[],
            &[],
            Columns {
                twin: true,
                adapter: true,
            },
            false,
        )
        .unwrap();
        let layouts = both.class_layouts.expect("columns rewritten");
        assert_eq!(layouts[0].methods[0].adapter_fn_id, None);
        assert_eq!(layouts[0].methods[0].name, "m", "the name column stays");
        assert!(
            both.funcs[1].bytes.is_empty(),
            "no row names the adapter → stripped"
        );
        assert!(both.funcs[2].bytes.is_empty());
    }
}
