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

/// The new values of the 8-byte addends of section-keyed
/// (`r_extern=0`) UNSIGNED relocs, which are object-file addresses: a
/// target in a cut section remaps through its runs (a dead one lands on
/// the section's base), later ones shift down. SUBTRACTOR pairs are
/// skipped (their stored value is a difference, not an address).
/// Returns `(ordinal, offset in the section as cut, value)`.
pub(crate) fn addend_patches(
    bytes: &[u8],
    sects: &[SectHdr],
    passes: &[Pass],
    kept: &[(u8, Vec<[u8; 8]>)],
) -> Result<Vec<(u8, u32, u64)>, String> {
    let mut patches = Vec::new();
    for s in sects {
        if s.reloff == 0 || s.nreloc == 0 || s.offset == 0 {
            continue;
        }
        let entries = tables_after(bytes, s, kept);
        let own = passes.iter().find(|p| p.ord == s.ord);
        let mut i = 0;
        while i < entries.len() {
            let info = rd32(&entries[i], 4);
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
                let site = rd32(&entries[i], 0);
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
                let v = moved_address(passes, target_sect, rd64(bytes, src));
                patches.push((s.ord, site, v));
            }
            i += 1;
        }
    }
    Ok(patches)
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
/// row no reloc left in any section names. Returns the bits and how
/// many are set.
pub(crate) fn unreferenced_undefs(
    bytes: &[u8],
    sects: &[SectHdr],
    kept: &[(u8, Vec<[u8; 8]>)],
    (symoff, nsyms): (usize, usize),
) -> (Vec<u64>, u32) {
    let mut referenced = vec![false; nsyms];
    for s in sects {
        for e in tables_after(bytes, s, kept) {
            let info = rd32(e, 4);
            if (info >> 27) & 1 == 1 {
                let idx = (info & 0x00FF_FFFF) as usize;
                if idx < nsyms {
                    referenced[idx] = true;
                }
            }
        }
    }
    let mut bits = vec![0u64; nsyms.div_ceil(64)];
    let mut n = 0;
    for (i, r) in referenced.iter().enumerate() {
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
