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
//! Policy stays in the caller (`torajs-cli` knows which members feed
//! which hook); this file is mechanism only. `TORAJS_LINK_ELIDE=0`
//! disables the pass (A/B pricing); `TORAJS_LINK_ELIDE_DIAG=1`
//! prints each verdict to stderr.

use std::collections::BTreeSet;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;

use crate::archives_merge::{MergedArchives, compute_required_members};
use crate::dead_strip_elide_patch::patch_site;
use crate::dead_strip_reach::{MemberReach, ReachResult, compute_reachability};
use crate::exec::LinkConfig;
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

/// Run the fix-point over `cfg.elidable_sites` + `cfg.guarded_stubs`;
/// answer the patched fn list (sites rewritten, surviving stubs
/// appended) when any shape survived, `None` when nothing changes.
pub(crate) fn judge_and_patch(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
    extra_defined_syms: &BTreeSet<String>,
) -> Result<Option<Vec<CompiledFunction>>, String> {
    if (cfg.elidable_sites.is_empty() && cfg.guarded_stubs.is_empty())
        || std::env::var_os("TORAJS_LINK_ELIDE").is_some_and(|v| v == "0")
    {
        return Ok(None);
    }
    let diag = std::env::var_os("TORAJS_LINK_ELIDE_DIAG").is_some();

    let mut elided: Vec<bool> = vec![true; cfg.elidable_sites.len()];
    let mut applied: Vec<bool> = vec![true; cfg.guarded_stubs.len()];
    // An applied stub no live atom imports is dead weight (it would
    // only ride along because `___torajs_`-named user fns are always
    // rooted) — and a page-line away from costing a whole page.
    let mut referenced: Vec<bool> = vec![true; cfg.guarded_stubs.len()];
    loop {
        let probe = assume(cfg, &elided, &applied)?;
        let required = compute_required_members(&probe, merged, extra_defined_syms)
            .map_err(|e| format!("member closure (elide probe): {e:?}"))?;
        let probe_cfg = LinkConfig {
            funcs: probe,
            ..cfg.clone()
        };
        let reach = compute_reachability(
            &probe_cfg,
            merged,
            &required,
            extra_defined_syms,
            false,
            None,
        )?;
        let mut changed = false;
        for (i, site) in cfg.elidable_sites.iter().enumerate() {
            if elided[i] && guard_live(&reach, &site.guard) {
                elided[i] = false;
                changed = true;
            }
        }
        for (i, stub) in cfg.guarded_stubs.iter().enumerate() {
            if applied[i] && guard_live(&reach, &stub.guard) {
                applied[i] = false;
                changed = true;
            }
        }
        if !changed {
            for (i, stub) in cfg.guarded_stubs.iter().enumerate() {
                referenced[i] = !applied[i] || reach.user_refs.contains(&stub.sym);
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
    }
    if !elided.iter().any(|&e| e) && !applied.iter().any(|&a| a) {
        return Ok(None);
    }
    Ok(Some(assume(cfg, &elided, &applied)?))
}

/// The user fn list with the chosen shapes assumed: elided sites
/// patched (bytes + relocs removed), fns the patched-away fn-address
/// sites orphaned stripped, applied stubs appended.
fn assume(
    cfg: &LinkConfig,
    elided: &[bool],
    applied: &[bool],
) -> Result<Vec<CompiledFunction>, String> {
    let mut funcs = cfg.funcs.clone();
    let mut fn_addr_taken = false;
    for (site, &e) in cfg.elidable_sites.iter().zip(elided) {
        if e {
            patch_site(&mut funcs, site)?;
            fn_addr_taken |= matches!(site.shape, SiteShape::FnAddr { .. });
        }
    }
    if fn_addr_taken {
        // The caller ran user-gc over the unpatched list; an adapter
        // whose mints are all gone is dead only now, and its relocs
        // are exactly the seeds the guard must not see.
        let roots = user_gc::table_root_fids(cfg);
        user_gc::strip_with_roots(&mut funcs, &cfg.entry, &roots, false);
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
    Ok(funcs)
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
        let kept = assume(&cfg, &[false], &[false]).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].relocs.len(), 1);
        let taken = assume(&cfg, &[true], &[true]).unwrap();
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
        let kept = assume(&cfg, &[false], &[false]).unwrap();
        assert!(
            !kept[1].bytes.is_empty(),
            "adapter stays while its mint does"
        );
        let taken = assume(&cfg, &[true], &[false]).unwrap();
        // movz x9, #0 ; nop — relocs gone, adapter emptied.
        assert_eq!(&taken[0].bytes[..4], &(0xD280_0000u32 | 9).to_le_bytes());
        assert_eq!(&taken[0].bytes[4..], &NOP.to_le_bytes());
        assert!(taken[0].relocs.is_empty());
        assert!(taken[1].bytes.is_empty(), "no mint left → adapter stripped");
        assert!(taken[1].relocs.is_empty());
    }
}
