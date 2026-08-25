//! r499 — conditional call sites: a `bl` in a user fn that the
//! dead-strip pre-pass replaces with a fixed instruction when the
//! runtime member it guards on has no live text once the site's own
//! edge is set aside.
//!
//! The shape it exists for: the synthesized `main` ends in three
//! unconditional calls — drain the microtask queue, sweep unhandled
//! rejections for the exit code, drain the cycle-collector buffer.
//! Each is a no-op unless some other live code can feed it (an
//! enqueue, a promise rejection, a buffered candidate), yet the call
//! itself roots the whole draining machinery into every artifact.
//! The caller names the site and the member whose text is the
//! evidence; this pass answers the question the linker alone can:
//! with these edges removed from the seeds, does any text of that
//! member still come out live? A feeder always crosses a crate
//! boundary through an extern symbol, so member-text liveness is
//! inline-proof — a feeder inlined into its caller still lives in
//! the feeder's own member.
//!
//! Policy stays in the caller (`torajs-cli` knows which runtime
//! members feed which drain); this file is mechanism only. The
//! judgment runs a reachability pass with the sites' edges dropped,
//! patches the elided sites (bytes + reloc removed), and the normal
//! strip pass then runs over the patched fns — an un-elided site's
//! callee closure comes back live through its restored edge.
//!
//! `TORAJS_LINK_ELIDE=0` disables the pass (A/B pricing);
//! `TORAJS_LINK_ELIDE_DIAG=1` prints each verdict to stderr.

use std::collections::BTreeSet;

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};

use crate::archives_merge::{MergedArchives, compute_required_members};
use torajs_obj::{N_SECT, N_TYPE};

use crate::dead_strip_reach::{MemberReach, ReachResult, compute_reachability};
use crate::exec::LinkConfig;

/// One conditional `bl` site. `func` + `byte_offset` name the reloc
/// (a `CallSite { Extern }` at that offset must exist);
/// `guard_member_prefix` is matched against archive member names
/// (`torajs_cycle-` covers every cgu of that crate);
/// `guard_ignore` lists the member's extern entry points that can
/// never feed the drain (a drop, a printer) so their liveness alone
/// does not keep the site — an unlisted entry keeps it, the safe
/// direction; `replacement` is the instruction word written over
/// the `bl` when elided.
#[derive(Debug, Clone)]
pub struct ElidableCall {
    pub func: String,
    pub byte_offset: u32,
    pub guard_member_prefix: String,
    pub guard_ignore: Vec<String>,
    pub replacement: u32,
}

/// Decide every site in `cfg.elidable_calls`; answer the patched fn
/// list when at least one site was elided, `None` when nothing
/// changes (no sites, pass disabled, or every guard came back live).
pub(crate) fn judge_and_patch(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
    extra_defined_syms: &BTreeSet<String>,
) -> Result<Option<Vec<CompiledFunction>>, String> {
    if cfg.elidable_calls.is_empty()
        || std::env::var_os("TORAJS_LINK_ELIDE").is_some_and(|v| v == "0")
    {
        return Ok(None);
    }
    let diag = std::env::var_os("TORAJS_LINK_ELIDE_DIAG").is_some();

    // Pass 1 — seeds without the conditional edges.
    let mut probe_funcs = cfg.funcs.clone();
    for site in &cfg.elidable_calls {
        let (f, idx) = locate(&mut probe_funcs, site)?;
        f.relocs.remove(idx);
    }
    let required = compute_required_members(&probe_funcs, merged, extra_defined_syms)
        .map_err(|e| format!("member closure (elide probe): {e:?}"))?;
    let probe_cfg = LinkConfig {
        funcs: probe_funcs,
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

    let mut patched = cfg.funcs.clone();
    let mut any = false;
    for site in &cfg.elidable_calls {
        let live = member_text_live(&reach, &site.guard_member_prefix, &site.guard_ignore);
        if diag {
            eprintln!(
                "elide: {}+{:#x} guard={} -> {}",
                site.func,
                site.byte_offset,
                site.guard_member_prefix,
                if live { "KEPT" } else { "ELIDED" }
            );
        }
        if live {
            continue;
        }
        let (f, idx) = locate(&mut patched, site)?;
        patch_site(f, idx, site)?;
        any = true;
    }
    Ok(any.then_some(patched))
}

/// Does any text atom of a member whose name starts with `prefix`
/// come out live, other than the atoms anchored by an `ignore`d
/// symbol? Members outside the closure are absent from the map, so
/// they answer `false`. A flag-rooted (`all_live`) text section
/// answers `true` without looking at names.
fn member_text_live(reach: &ReachResult<'_>, prefix: &str, ignore: &[String]) -> bool {
    reach
        .members
        .values()
        .filter(|m| m.member_name.starts_with(prefix))
        .any(|m| {
            m.sects.iter().enumerate().any(|(si, s)| {
                s.is_text
                    && (s.all_live
                        || s.atoms.iter().zip(&s.live).any(|(&(start, _), &l)| {
                            l && !anchor_ignored(m, (si + 1) as u8, start, ignore)
                        }))
            })
        })
}

/// Is the defined symbol anchoring the atom at `start` (the highest
/// `N_SECT` nlist row at or below it in that section) on the ignore
/// list? An unanchored atom (leading gap) is never ignored.
fn anchor_ignored(m: &MemberReach<'_>, n_sect: u8, start: u64, ignore: &[String]) -> bool {
    m.nlist
        .iter()
        .filter(|n| n.n_type & N_TYPE == N_SECT && n.n_sect == n_sect && n.n_value <= start)
        .max_by_key(|n| n.n_value)
        .is_some_and(|n| ignore.iter().any(|i| i == n.name))
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
    use torajs_codegen::frame::FrameLayout;
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

    fn site(func: &str, off: u32) -> ElidableCall {
        ElidableCall {
            func: func.into(),
            byte_offset: off,
            guard_member_prefix: "torajs_x-".into(),
            guard_ignore: Vec::new(),
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
}
