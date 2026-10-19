//! The table half of one multi-section rewrite
//! (`dead_strip_rewrite_sections`): each cut section's reloc table
//! filtered to its surviving sites, every section-keyed address addend
//! and every `N_SECT` row's value pushed through the passes in order.

use torajs_obj::{ARM64_RELOC_UNSIGNED, N_SECT, N_TYPE, NLIST_64_SIZE, RELOCATION_INFO_SIZE};

use crate::dead_strip_rewrite::{DeadRowPlacement, SectHdr, addend_group_len, dead_row_addr};
use crate::dead_strip_rewrite_sections::{Pass, rd32, rd64, shift_off};

const ARM64_RELOC_SUBTRACTOR: u8 = 1;

/// Keep the relocs of `pass`'s section whose site survives, at their
/// new `r_address`, in place in the table at `tbl` (its tail zeroed);
/// returns how many were kept.
pub(crate) fn filter_relocs(out: &mut [u8], pass: &Pass, tbl: u32, nreloc: u32) -> u32 {
    let (tbl, n) = (tbl as usize, nreloc as usize);
    let esz = RELOCATION_INFO_SIZE as usize;
    let entries: Vec<[u8; 8]> = out[tbl..tbl + n * esz].as_chunks::<8>().0.to_vec();
    let mut kept = 0usize;
    let mut i = 0;
    while i < n {
        // ARM64_RELOC_ADDEND precedes its mate with the same
        // r_address — keep/drop the pair together
        let group = addend_group_len(&entries, i);
        let r_address = rd32(&entries[i + group - 1], 0);
        // a site in a dead atom remaps to None and is dropped
        if let Some(new_abs) = pass.remap(pass.addr + u64::from(r_address)) {
            let new_rel = (new_abs - pass.addr) as u32;
            for e in &entries[i..i + group] {
                let at = tbl + kept * esz;
                out[at..at + 4].copy_from_slice(&new_rel.to_le_bytes());
                out[at + 4..at + 8].copy_from_slice(&e[4..8]);
                kept += 1;
            }
        }
        i += group;
    }
    out[tbl + kept * esz..tbl + n * esz].fill(0);
    kept as u32
}

/// Rewrite the 8-byte addends of section-keyed (`r_extern=0`)
/// UNSIGNED relocs, which are object-file addresses: a target in a
/// cut section remaps through its runs (a dead one lands on the
/// section's base), later ones shift down. SUBTRACTOR pairs are
/// skipped (their stored value is a difference, not an address).
pub(crate) fn patch_unsigned_addends(
    out: &mut [u8],
    sects: &[SectHdr],
    passes: &[Pass],
) -> Result<(), String> {
    let esz = RELOCATION_INFO_SIZE as usize;
    for s in sects {
        if s.reloff == 0 || s.nreloc == 0 || s.offset == 0 {
            continue;
        }
        let tbl = shift_off(passes, s.reloff) as usize;
        let payload = shift_off(passes, s.offset) as usize;
        let n = rd32(out, s.hdr_pos + 60) as usize;
        let mut i = 0;
        while i < n {
            let info = rd32(out, tbl + i * esz + 4);
            let r_type = ((info >> 28) & 0xF) as u8;
            if r_type == ARM64_RELOC_SUBTRACTOR {
                i += 2; // the stored value is a difference
                continue;
            }
            let r_extern = (info >> 27) & 0x1;
            let r_length = (info >> 25) & 0x3;
            let r_pcrel = (info >> 24) & 0x1;
            let target_sect = (info & 0x00FF_FFFF) as u8;
            if r_extern == 0 && r_type == ARM64_RELOC_UNSIGNED && r_length == 3 && r_pcrel == 0 {
                let site = payload + rd32(out, tbl + i * esz) as usize;
                if out.len() < site + 8 {
                    return Err(format!("sect {} addend site out of range", s.ord));
                }
                let mut v = rd64(out, site);
                for p in passes {
                    v = if target_sect == p.ord {
                        p.remap(v).unwrap_or(p.addr)
                    } else if v >= p.old_end() {
                        v - p.shift
                    } else {
                        v
                    };
                }
                out[site..site + 8].copy_from_slice(&v.to_le_bytes());
            }
            i += 1;
        }
    }
    Ok(())
}

/// Move every `N_SECT` row's value: live rows of a cut section remap,
/// dead ones go where `dead_at` puts them, later ones shift. Returns
/// the dead rows, pass by pass, ascending within a pass.
pub(crate) fn move_rows(
    out: &mut [u8],
    passes: &[Pass],
    symoff: usize,
    nsyms: usize,
    dead_at: DeadRowPlacement,
) -> Vec<u32> {
    let mut dead: Vec<Vec<u32>> = vec![Vec::new(); passes.len()];
    for i in 0..nsyms {
        let p = symoff + i * NLIST_64_SIZE as usize;
        let (n_type, n_sect) = (out[p + 4], out[p + 5]);
        if n_type & N_TYPE != N_SECT {
            continue;
        }
        let mut v = rd64(out, p + 8);
        for (k, pass) in passes.iter().enumerate() {
            v = if n_sect == pass.ord {
                pass.remap(v).unwrap_or_else(|| {
                    dead[k].push(i as u32);
                    dead_row_addr(&pass.runs, dead_at, pass.addr, v)
                })
            } else if v >= pass.old_end() {
                v - pass.shift
            } else {
                v
            };
        }
        out[p + 8..p + 16].copy_from_slice(&v.to_le_bytes());
    }
    dead.concat()
}
