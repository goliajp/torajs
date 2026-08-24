//! S2 dead-strip blade 2b — archive repack driver (RFC
//! 20260824-s2-dead-strip, plan C).
//!
//! Runs the full pre-pass: merge → member closure → atom
//! reachability → per-member `.o` rewrite → `.a` repack. The caller
//! (`link_to_exec_with_archives`, gated on `TORAJS_LINK_DEADSTRIP`)
//! swaps the stripped archives into a cloned `LinkConfig` and runs
//! the unchanged pipeline against them.
//!
//! Repack notes: the writer emits BSD-style `#1/N` long-name
//! members (NUL-padded names, 2-byte member alignment) — the same
//! dialect `parse_archive` consumes. `__.SYMDEF` index members were
//! already dropped at parse time and are not re-emitted; nothing in
//! this linker reads them.

use std::borrow::Cow;
use std::collections::BTreeMap;

use torajs_obj::{
    N_EXT, N_TYPE, N_UNDF, NLIST_64_SIZE, RELOCATION_INFO_SIZE, S_4BYTE_LITERALS, S_8BYTE_LITERALS,
    S_16BYTE_LITERALS, S_CSTRING_LITERALS, S_REGULAR, SECTION_TYPE,
};

use crate::archive::ArMember;
use crate::archive_link_extra_syms::collect_extra_defined_syms;
use crate::archives_merge::{compute_required_members, merge_archive_indexes};
use crate::dead_strip_reach::{MemberReach, SectAtoms, compute_reachability};
use crate::dead_strip_rewrite::{scan_load_commands, strip_member_section};
use crate::exec::LinkConfig;

/// Run the dead-strip pre-pass over `cfg.archives`. Returns the
/// replacement archive list, or `None` when nothing was stripped
/// (the caller then links the originals untouched).
pub(crate) fn strip_archives(cfg: &LinkConfig) -> Result<Option<Vec<Cow<'static, [u8]>>>, String> {
    let merged = merge_archive_indexes(&cfg.archives).map_err(|e| format!("merge: {e:?}"))?;
    let extra = collect_extra_defined_syms(cfg);
    let required = compute_required_members(&cfg.funcs, &merged, &extra)
        .map_err(|e| format!("member closure: {e:?}"))?;
    // cuts=None always: the CUT what-if is a diag-only pricing
    // probe and must never shape emitted bytes.
    let reach = compute_reachability(cfg, &merged, &required, &extra, false, None)?;

    let mut out: Vec<Cow<'static, [u8]>> = Vec::with_capacity(cfg.archives.len());
    let mut any = false;
    for (a_idx, members) in merged.per_archive_members.iter().enumerate() {
        let mut replaced: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
        for (m_idx, m) in members.iter().enumerate() {
            if let Some(r) = reach.members.get(&(a_idx, m_idx))
                && let Some(new_bytes) = strip_member(m.data, r)?
            {
                replaced.insert(m_idx, new_bytes);
            }
        }
        if replaced.is_empty() {
            out.push(cfg.archives[a_idx].clone());
        } else {
            any = true;
            out.push(Cow::Owned(write_archive(members, &replaced)));
        }
    }
    Ok(any.then_some(out))
}

/// Which sections the rewriter may subset (S2 blade 3): regular
/// file-storage payloads plus the cstring / literal pools. Zerofill
/// has no bytes to drop; the TLV family flows through dedicated
/// descriptor / thunk layout passes (and is rooted by the closure
/// anyway); flag-rooted sections come back `all_live`.
fn strippable(sa: &SectAtoms) -> bool {
    if sa.no_file_storage || sa.all_live || sa.size == 0 {
        return false;
    }
    matches!(
        sa.flags & SECTION_TYPE,
        S_REGULAR | S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS
    ) && sa.live.iter().any(|&l| !l)
}

/// Apply the per-section rewriter once for every eligible section
/// of one member, threading each pass's output into the next (the
/// rewriter re-scans load commands, so it always sees the previous
/// pass's shifted headers). Atom ranges are handed over
/// section-relative — relative offsets survive earlier passes;
/// absolute reach addresses do not.
fn strip_member(bytes: &[u8], reach: &MemberReach<'_>) -> Result<Option<Vec<u8>>, String> {
    let mut cur: Option<Vec<u8>> = None;
    for (i, sa) in reach.sects.iter().enumerate() {
        if !strippable(sa) {
            continue;
        }
        let atoms_rel: Vec<(u64, u64)> = sa
            .atoms
            .iter()
            .map(|&(s, e)| (s - sa.addr, e - sa.addr))
            .collect();
        let b = cur.as_deref().unwrap_or(bytes);
        if let Some(next) = strip_member_section(b, (i + 1) as u8, &atoms_rel, &sa.live)? {
            cur = Some(next);
        }
    }
    // S2 blade 3 dead-undef cleanup: with the dead reloc sites gone,
    // clear N_EXT on undef nlist rows nothing references any more.
    let stripped = cur.is_some();
    let mut out = match cur {
        Some(v) => v,
        None => bytes.to_vec(),
    };
    let neutered = neuter_dead_undefs(&mut out)?;
    if !stripped && neutered == 0 {
        return Ok(None);
    }
    Ok(Some(out))
}

/// Clear `N_EXT` on undefined nlist rows that no surviving reloc
/// references. `parse_member_undef_externs` then stops reporting
/// them, so the downstream member closure over the stripped
/// archives pulls fewer members and derives fewer dyld binds.
/// Physical row layout is untouched — extern reloc indices stay
/// valid. Returns the number of rows neutered.
fn neuter_dead_undefs(bytes: &mut [u8]) -> Result<u32, String> {
    let (_, symtab_pos, _, sects) = scan_load_commands(bytes)?;
    let Some(sp) = symtab_pos else {
        return Ok(0);
    };
    let symoff = u32::from_le_bytes(bytes[sp + 8..sp + 12].try_into().unwrap()) as usize;
    let nsyms = u32::from_le_bytes(bytes[sp + 12..sp + 16].try_into().unwrap()) as usize;
    let mut referenced = vec![false; nsyms];
    let esz = RELOCATION_INFO_SIZE as usize;
    for s in &sects {
        if s.reloff == 0 {
            continue;
        }
        for i in 0..s.nreloc as usize {
            let e = s.reloff as usize + i * esz;
            let info = u32::from_le_bytes(bytes[e + 4..e + 8].try_into().unwrap());
            if (info >> 27) & 1 == 1 {
                let idx = (info & 0x00FF_FFFF) as usize;
                if idx < nsyms {
                    referenced[idx] = true;
                }
            }
        }
    }
    let mut n = 0;
    for (i, r) in referenced.iter().enumerate() {
        let p = symoff + i * NLIST_64_SIZE as usize;
        let t = bytes[p + 4];
        if t & N_TYPE == N_UNDF && t & N_EXT != 0 && !r {
            bytes[p + 4] = t & !N_EXT;
            n += 1;
        }
    }
    Ok(n)
}

/// Serialize an archive: every member in order, swapping in the
/// rewritten bodies. BSD `#1/N` long names throughout.
fn write_archive(members: &[ArMember<'_>], replaced: &BTreeMap<usize, Vec<u8>>) -> Vec<u8> {
    let mut out: Vec<u8> = b"!<arch>\n".to_vec();
    for (i, m) in members.iter().enumerate() {
        let data: &[u8] = replaced.get(&i).map(|v| v.as_slice()).unwrap_or(m.data);
        let name = m.name.as_bytes();
        let name_padded = name.len().next_multiple_of(8);
        let total = name_padded + data.len();

        let mut hdr = [b' '; 60];
        let n1 = format!("#1/{name_padded}");
        hdr[..n1.len()].copy_from_slice(n1.as_bytes());
        hdr[16] = b'0'; // mtime
        hdr[28] = b'0'; // uid
        hdr[34] = b'0'; // gid
        hdr[40..46].copy_from_slice(b"100644");
        let sz = total.to_string();
        hdr[48..48 + sz.len()].copy_from_slice(sz.as_bytes());
        hdr[58] = 0x60; // '`'
        hdr[59] = b'\n';
        out.extend_from_slice(&hdr);
        out.extend_from_slice(name);
        out.resize(out.len() + (name_padded - name.len()), 0);
        out.extend_from_slice(data);
        if total % 2 != 0 {
            out.push(b'\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::write_archive;
    use crate::archive::{ArMember, parse_archive};
    use std::collections::BTreeMap;

    #[test]
    fn roundtrip_through_parse_archive() {
        let members = [
            ArMember {
                name: "alpha_with_a_rather_long_member_name.o",
                data: &[1, 2, 3, 4, 5],
            },
            ArMember {
                name: "beta.o",
                data: &[9, 8],
            },
        ];
        let mut replaced = BTreeMap::new();
        replaced.insert(1usize, vec![7, 7, 7, 7]);
        let bytes = write_archive(&members, &replaced);
        let parsed = parse_archive(&bytes).expect("roundtrip parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, members[0].name);
        assert_eq!(parsed[0].data, members[0].data);
        assert_eq!(parsed[1].name, "beta.o");
        assert_eq!(parsed[1].data, &[7, 7, 7, 7]);
    }
}
