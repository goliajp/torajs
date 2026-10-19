//! The table half of one multi-section cut
//! (`dead_strip_rewrite_sections`): each cut section's reloc table
//! filtered to its surviving sites, the section-keyed address addends
//! and every `N_SECT` row's value pushed through the passes in order,
//! and the undef rows no surviving reloc names.

use torajs_obj::{ARM64_RELOC_UNSIGNED, N_EXT, N_TYPE, N_UNDF, NLIST_64_SIZE};

use crate::dead_strip_rewrite::{DeadRowPlacement, SectHdr, addend_group_len, dead_row_addr};
use crate::dead_strip_rewrite_sections::{Pass, rd32, rd64};

const ARM64_RELOC_SUBTRACTOR: u8 = 1;

/// The relocs of `pass`'s section whose site survives, at their new
/// `r_address`; `input` is the section's reloc table as it was read.
pub(crate) fn filter_relocs(input: &[u8], pass: &Pass) -> Vec<[u8; 8]> {
    let entries = input.as_chunks::<8>().0;
    let n = entries.len();
    let mut kept = Vec::with_capacity(n);
    let (mut i, mut hint) = (0usize, 0usize);
    while i < n {
        // ARM64_RELOC_ADDEND precedes its mate with the same
        // r_address — keep/drop the pair together
        let group = addend_group_len(entries, i);
        let r_address = rd32(&entries[i + group - 1], 0);
        // a site in a dead atom remaps to None and is dropped
        if let Some(new_rel) = pass.remap_rel(u64::from(r_address), &mut hint) {
            for e in &entries[i..i + group] {
                let mut k = *e;
                k[..4].copy_from_slice(&(new_rel as u32).to_le_bytes());
                kept.push(k);
            }
        }
        i += group;
    }
    kept
}

/// Every section's reloc table as the cut leaves it: the kept relocs
/// of a cut section, the table as read otherwise.
pub(crate) fn tables_after<'t>(
    bytes: &'t [u8],
    s: &SectHdr,
    kept: &'t [(u8, Vec<[u8; 8]>)],
) -> &'t [[u8; 8]] {
    match kept.iter().find(|(ord, _)| *ord == s.ord) {
        Some((_, k)) => k,
        None if s.reloff == 0 => &[],
        None => {
            bytes[s.reloff as usize..][..s.nreloc as usize * 8]
                .as_chunks()
                .0
        }
    }
}

/// An addend to rewrite: `(ordinal, offset in the section as cut, value)`.
pub(crate) type Patch = (u8, u32, u64);

/// One walk over every reloc the cut leaves. Returns the new values of
/// the 8-byte addends of section-keyed (`r_extern=0`) UNSIGNED relocs,
/// which are object-file addresses (a target in a cut section remaps
/// through its runs, a dead one landing on the section's base, later
/// ones shift down; SUBTRACTOR pairs are skipped, their stored value is
/// a difference), as `(ordinal, offset in the section as cut, value)`;
/// and which of the first `nsyms` rows some reloc names.
pub(crate) fn walk_relocs(
    bytes: &[u8],
    sects: &[SectHdr],
    passes: &[Pass],
    kept: &[(u8, Vec<[u8; 8]>)],
    nsyms: usize,
) -> Result<(Vec<Patch>, Vec<bool>), String> {
    let (mut patches, mut referenced) = (Vec::new(), vec![false; nsyms]);
    for s in sects {
        let patch = !passes.is_empty() && s.reloff != 0 && s.nreloc != 0 && s.offset != 0;
        let own = passes.iter().find(|p| p.ord == s.ord);
        let mut skip = false;
        for e in tables_after(bytes, s, kept) {
            let info = rd32(e, 4);
            let (r_extern, symbolnum) = ((info >> 27) & 0x1, (info & 0x00FF_FFFF) as usize);
            if r_extern == 1 && symbolnum < nsyms {
                referenced[symbolnum] = true;
            }
            if !patch || std::mem::take(&mut skip) {
                continue;
            }
            let r_type = ((info >> 28) & 0xF) as u8;
            if r_type == ARM64_RELOC_SUBTRACTOR {
                skip = true; // its mate's stored value is a difference
                continue;
            }
            let (r_length, r_pcrel) = ((info >> 25) & 0x3, (info >> 24) & 0x1);
            if r_extern == 0 && r_type == ARM64_RELOC_UNSIGNED && r_length == 3 && r_pcrel == 0 {
                let site = rd32(e, 0);
                let old_site = match own {
                    Some(p) => p
                        .unmap_rel(u64::from(site))
                        .ok_or("kept reloc off its runs")?,
                    None => u64::from(site),
                };
                let src = s.offset as usize + old_site as usize;
                if bytes.len() < src + 8 {
                    return Err(format!("sect {} addend site out of range", s.ord));
                }
                let v = moved_address(passes, symbolnum as u8, rd64(bytes, src));
                patches.push((s.ord, site, v));
            }
        }
    }
    Ok((patches, referenced))
}

/// An address in section `sect` after every pass (a dead one lands on
/// its section's base).
fn moved_address(passes: &[Pass], sect: u8, mut v: u64) -> u64 {
    for p in passes {
        v = if sect == p.ord {
            p.remap(v).unwrap_or(p.addr)
        } else if v >= p.old_end() {
            v - p.shift
        } else {
            v
        };
    }
    v
}

/// One bit per nlist row of `(symoff, nsyms)`: an `N_UNDF | N_EXT`
/// row no reloc names (`referenced`, from [`walk_relocs`]). Returns the
/// bits and how many are set.
pub(crate) fn unreferenced_undefs(
    bytes: &[u8],
    (symoff, nsyms): (usize, usize),
    referenced: &[bool],
) -> (Vec<u64>, u32) {
    let mut bits = vec![0u64; nsyms.div_ceil(64)];
    let mut n = 0;
    for (i, r) in referenced.iter().enumerate().take(nsyms) {
        let t = bytes[symoff + i * NLIST_64_SIZE as usize + 4];
        if t & N_TYPE == N_UNDF && t & N_EXT != 0 && !r {
            bits[i / 64] |= 1 << (i % 64);
            n += 1;
        }
    }
    (bits, n)
}

#[cfg(test)]
thread_local! {
    /// Rows [`walk_row`] moved on this thread, for tests that must
    /// reach it.
    pub(crate) static WALKED_ROWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Where an `N_SECT` row of section `n_sect` at `v` lands after every
/// pass, and the pass that dropped its atom, if one did.
pub(crate) fn move_value(
    passes: &[Pass],
    plans: &[Option<RowPlan>],
    n_sect: u8,
    v: u64,
    dead_at: DeadRowPlacement,
) -> (u64, Option<usize>) {
    let plan = usize::from(n_sect)
        .checked_sub(1)
        .and_then(|k| plans.get(k)?.as_ref())
        .filter(|pl| (pl.lo..=pl.hi).contains(&v));
    match plan {
        Some(pl) => pl.apply(passes, v, dead_at),
        None => walk_row(passes, n_sect, v, dead_at),
    }
}

/// One row's value through every pass in order: the definition
/// [`RowPlan`] shortcuts.
fn walk_row(
    passes: &[Pass],
    n_sect: u8,
    mut v: u64,
    dead_at: DeadRowPlacement,
) -> (u64, Option<usize>) {
    #[cfg(test)]
    WALKED_ROWS.with(|w| w.set(w.get() + 1));
    let mut dead = None;
    for (k, pass) in passes.iter().enumerate() {
        v = if n_sect == pass.ord {
            pass.remap(v).unwrap_or_else(|| {
                dead = Some(k);
                dead_row_addr(&pass.runs, dead_at, pass.addr, v)
            })
        } else if v >= pass.old_end() {
            v - pass.shift
        } else {
            v
        };
    }
    (v, dead)
}

/// How every value inside one section's address range `[lo, hi]`
/// moves: each pass other than the section's own one either shifts
/// all of them or none, so the walk is a shift, the section's own
/// remap, and a shift.
#[derive(Debug)]
pub(crate) struct RowPlan {
    lo: u64,
    hi: u64,
    before: u64,
    own: Option<usize>,
    after: u64,
}

impl RowPlan {
    fn apply(&self, passes: &[Pass], v: u64, dead_at: DeadRowPlacement) -> (u64, Option<usize>) {
        let v = v - self.before;
        let (v, dead) = match self.own {
            Some(k) => {
                let pass = &passes[k];
                match pass.remap(v) {
                    Some(v) => (v, None),
                    None => (dead_row_addr(&pass.runs, dead_at, pass.addr, v), Some(k)),
                }
            }
            None => (v, None),
        };
        (v - self.after, dead)
    }
}

/// The plan of `sect`'s rows; `None` when some pass shifts only part
/// of its range (those rows take [`walk_row`]).
pub(crate) fn row_plan(sect: &SectHdr, passes: &[Pass]) -> Option<RowPlan> {
    let (mut lo, mut hi) = (sect.addr, sect.addr + sect.size);
    let (mut before, mut own, mut after) = (0, None, 0);
    for (k, p) in passes.iter().enumerate() {
        if p.ord == sect.ord {
            // the remap lands inside the shortened section
            own = Some(k);
            (lo, hi) = (p.addr, p.addr + p.old_size - p.shift);
            continue;
        }
        let (a, b) = (lo >= p.old_end(), hi >= p.old_end());
        if a != b {
            return None;
        }
        if a {
            (lo, hi) = (lo - p.shift, hi - p.shift);
            if own.is_some() {
                after += p.shift;
            } else {
                before += p.shift;
            }
        }
    }
    Some(RowPlan {
        lo: sect.addr,
        hi: sect.addr + sect.size,
        before,
        own,
        after,
    })
}
