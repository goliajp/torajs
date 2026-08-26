//! r499 — link-judged conditional shapes in the user `.o`: a `bl`
//! the dead-strip pre-pass may replace with a fixed instruction
//! ([`ElidableCall`]), and a definition it may add to shadow a
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
//! Policy stays in the caller (`torajs-cli` knows which members feed
//! which hook); this file is mechanism only. `TORAJS_LINK_ELIDE=0`
//! disables the pass (A/B pricing); `TORAJS_LINK_ELIDE_DIAG=1`
//! prints each verdict to stderr.

use std::collections::BTreeSet;

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
use torajs_obj::{N_SECT, N_TYPE};

use crate::archives_merge::{MergedArchives, compute_required_members};
use crate::dead_strip_reach::{MemberReach, ReachResult, compute_reachability};
use crate::exec::LinkConfig;

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

/// One conditional `bl` site. `func` + `byte_offset` name the reloc
/// (a `CallSite { Extern }` at that offset must exist);
/// `replacement` is the instruction word written over the `bl`
/// when elided.
#[derive(Debug, Clone)]
pub struct ElidableCall {
    pub func: String,
    pub byte_offset: u32,
    pub guard: Guard,
    pub replacement: u32,
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
    pub relocs: Vec<Reloc>,
    pub guard: Guard,
}

/// Run the fix-point over `cfg.elidable_calls` + `cfg.guarded_stubs`;
/// answer the patched fn list (sites rewritten, surviving stubs
/// appended) when any shape survived, `None` when nothing changes.
pub(crate) fn judge_and_patch(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
    extra_defined_syms: &BTreeSet<String>,
) -> Result<Option<Vec<CompiledFunction>>, String> {
    if (cfg.elidable_calls.is_empty() && cfg.guarded_stubs.is_empty())
        || std::env::var_os("TORAJS_LINK_ELIDE").is_some_and(|v| v == "0")
    {
        return Ok(None);
    }
    let diag = std::env::var_os("TORAJS_LINK_ELIDE_DIAG").is_some();

    let mut elided: Vec<bool> = vec![true; cfg.elidable_calls.len()];
    let mut applied: Vec<bool> = vec![true; cfg.guarded_stubs.len()];
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
        for (i, site) in cfg.elidable_calls.iter().enumerate() {
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
            break;
        }
    }

    if diag {
        for (site, &e) in cfg.elidable_calls.iter().zip(&elided) {
            eprintln!(
                "elide: {}+{:#x} guard={} -> {}",
                site.func,
                site.byte_offset,
                site.guard,
                if e { "ELIDED" } else { "KEPT" }
            );
        }
        for (stub, &a) in cfg.guarded_stubs.iter().zip(&applied) {
            eprintln!(
                "stub: {} guard={} -> {}",
                stub.sym,
                stub.guard,
                if a { "APPLIED" } else { "DROPPED" }
            );
        }
    }
    if !elided.iter().any(|&e| e) && !applied.iter().any(|&a| a) {
        return Ok(None);
    }
    Ok(Some(assume(cfg, &elided, &applied)?))
}

/// The user fn list with the chosen shapes assumed: elided sites
/// patched (bytes + reloc removed), applied stubs appended.
fn assume(
    cfg: &LinkConfig,
    elided: &[bool],
    applied: &[bool],
) -> Result<Vec<CompiledFunction>, String> {
    let mut funcs = cfg.funcs.clone();
    for (site, &e) in cfg.elidable_calls.iter().zip(elided) {
        if e {
            let (f, idx) = locate(&mut funcs, site)?;
            patch_site(f, idx, site)?;
        }
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
    m.nlist
        .iter()
        .filter(|n| n.n_type & N_TYPE == N_SECT && n.n_sect == n_sect && n.n_value <= start)
        .max_by_key(|n| n.n_value)
        .is_some_and(|n| names.iter().any(|i| i == n.name))
}

/// Find the site's fn and the index of its reloc. A missing fn or
/// reloc is a caller contract violation — loud, never skipped.
fn locate<'f>(
    funcs: &'f mut [CompiledFunction],
    site: &ElidableCall,
) -> Result<(&'f mut CompiledFunction, usize), String> {
    let f = funcs
        .iter_mut()
        .find(|f| f.name == site.func)
        .ok_or_else(|| format!("elidable call: no fn named {}", site.func))?;
    let idx = f
        .relocs
        .iter()
        .position(|r| {
            r.byte_offset == site.byte_offset
                && matches!(
                    &r.kind,
                    RelocKind::CallSite {
                        target: CallTarget::Extern(_)
                    }
                )
        })
        .ok_or_else(|| {
            format!(
                "elidable call: {}+{:#x} carries no extern CallSite reloc",
                site.func, site.byte_offset
            )
        })?;
    Ok((f, idx))
}

/// Overwrite the `bl` word with the replacement and drop its reloc.
fn patch_site(
    f: &mut CompiledFunction,
    reloc_idx: usize,
    site: &ElidableCall,
) -> Result<(), String> {
    let off = site.byte_offset as usize;
    let word = f
        .bytes
        .get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("elidable call: {}+{:#x} past fn end", site.func, off))?;
    // BL: 100101 imm26.
    if word & 0xFC00_0000 != 0x9400_0000 {
        return Err(format!(
            "elidable call: {}+{:#x} is {word:#010x}, not a bl",
            site.func, off
        ));
    }
    f.bytes[off..off + 4].copy_from_slice(&site.replacement.to_le_bytes());
    f.relocs.remove(reloc_idx);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::reloc::Reloc;

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

    fn guard() -> Guard {
        Guard::Member {
            prefix: "torajs_x-".into(),
            ignore: Vec::new(),
        }
    }

    fn site(func: &str, off: u32) -> ElidableCall {
        ElidableCall {
            func: func.into(),
            byte_offset: off,
            guard: guard(),
            replacement: NOP,
        }
    }

    #[test]
    fn patch_rewrites_bl_and_drops_reloc() {
        let mut f = fn_with_bl("_main_user", "___torajs_drain");
        patch_site(&mut f, 0, &site("_main_user", 4)).unwrap();
        assert_eq!(&f.bytes[4..8], &NOP.to_le_bytes());
        assert!(f.relocs.is_empty());
    }

    #[test]
    fn patch_refuses_non_bl_word() {
        let mut f = fn_with_bl("_main_user", "___torajs_drain");
        f.relocs[0].byte_offset = 0;
        let err = patch_site(&mut f, 0, &site("_main_user", 0)).unwrap_err();
        assert!(err.contains("not a bl"), "{err}");
    }

    #[test]
    fn locate_rejects_unknown_site() {
        let mut funcs = vec![fn_with_bl("_main_user", "___torajs_drain")];
        assert!(locate(&mut funcs, &site("_main_user", 8)).is_err());
        assert!(locate(&mut funcs, &site("_other", 4)).is_err());
        assert_eq!(locate(&mut funcs, &site("_main_user", 4)).unwrap().1, 0);
    }

    #[test]
    fn assume_patches_chosen_sites_and_appends_applied_stubs() {
        let cfg = LinkConfig {
            funcs: vec![fn_with_bl("_main_user", "___torajs_drain")],
            entry: "_main_user".into(),
            sym_table: crate::resolve::SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: true,
            strip_member_symbols: false,
            elidable_calls: vec![site("_main_user", 4)],
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
        };
        let kept = assume(&cfg, &[false], &[false]).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].relocs.len(), 1);
        let taken = assume(&cfg, &[true], &[true]).unwrap();
        assert_eq!(taken.len(), 2);
        assert!(taken[0].relocs.is_empty());
        assert_eq!(taken[1].name, "___torajs_hook");
        assert!(taken[1].relocs.is_empty());
    }
}
