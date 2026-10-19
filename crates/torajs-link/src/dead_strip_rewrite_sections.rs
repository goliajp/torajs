//! Drop the dead atoms of several sections of one member `.o` in a
//! single rewrite. The bytes are those of rewriting one section at a
//! time in section order (see `dead_strip_rewrite`), each pass seeing
//! the headers the previous one shifted; the passes are planned in
//! that order and every moved value is pushed through them in the same
//! order, but the member is copied, and its nlist and reloc tables are
//! walked, once.

use crate::dead_strip_rewrite::{
    DeadRowPlacement, Run, SectHdr, StrippedMember, build_runs, run_modulus, scan_load_commands,
};
use crate::dead_strip_rewrite_tables::{filter_relocs, move_rows, patch_unsigned_addends};
use crate::runtime_prestrip::dead_content_in_prestripped;

const RELOC_SIZE: usize = torajs_obj::RELOCATION_INFO_SIZE as usize;

/// One section to cut: its atoms as section-relative `[start, end)`
/// ranges and which of them are live.
pub(crate) struct SectionCut<'r> {
    pub(crate) ord: u8,
    pub(crate) atoms_rel: &'r [(u64, u64)],
    pub(crate) live: &'r [bool],
}

/// One section the rewrite shortens, with the address and file offset
/// it has once the passes before it are applied.
pub(crate) struct Pass {
    pub(crate) ord: u8,
    pub(crate) addr: u64,
    pub(crate) old_size: u64,
    pub(crate) shift: u64,
    file_off: usize,
    /// payload offset in the input
    in_off: usize,
    pub(crate) runs: Vec<Run>,
}

impl Pass {
    pub(crate) fn old_end(&self) -> u64 {
        self.addr + self.old_size
    }

    /// Where a live address of this section lands; `None` in a dead atom.
    pub(crate) fn remap(&self, addr: u64) -> Option<u64> {
        let o = addr.checked_sub(self.addr)?;
        let i = self.runs.partition_point(|r| r.old_start <= o);
        let r = self.runs.get(i.checked_sub(1)?)?;
        (o < r.old_start + r.len).then(|| self.addr + r.new_start + (o - r.old_start))
    }

    /// [`Pass::remap`] of the section-relative offset `o`, also
    /// section-relative. Tries the run at `hint` first (sites in a row
    /// mostly fall in one run) and leaves the run found there.
    pub(crate) fn remap_rel(&self, o: u64, hint: &mut usize) -> Option<u64> {
        let i = match self.runs.get(*hint) {
            Some(r)
                if r.old_start <= o && self.runs.get(*hint + 1).is_none_or(|n| n.old_start > o) =>
            {
                *hint
            }
            _ => self
                .runs
                .partition_point(|r| r.old_start <= o)
                .checked_sub(1)?,
        };
        *hint = i;
        let r = &self.runs[i];
        (o < r.old_start + r.len).then(|| r.new_start + (o - r.old_start))
    }
}

/// An address outside the cut sections, after every pass.
fn shift_addr(passes: &[Pass], mut v: u64) -> u64 {
    for p in passes {
        if v >= p.old_end() {
            v -= p.shift;
        }
    }
    v
}

/// A file offset, after every pass.
pub(crate) fn shift_off(passes: &[Pass], mut off: u32) -> u32 {
    for p in passes {
        if off as usize >= p.file_off + p.old_size as usize {
            off -= p.shift as u32;
        }
    }
    off
}

pub(crate) fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

pub(crate) fn rd64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// Rewrite `bytes` keeping only the live atoms of each of `cuts`
/// (ascending section order). A section whose cut buys no bytes is
/// left alone unless the runtime pre-strip left dead content in it;
/// `None` when that leaves every section alone.
pub(crate) fn strip_sections(
    bytes: &[u8],
    cuts: &[SectionCut<'_>],
    dead_at: DeadRowPlacement,
) -> Result<Option<StrippedMember>, String> {
    let (seg_positions, symtab_pos, dysymtab_pos, sects) = scan_load_commands(bytes)?;
    let passes = plan(bytes, symtab_pos, &sects, cuts)?;
    if passes.is_empty() {
        return Ok(None);
    }
    let mut out = copy_spans(bytes, &passes);
    let symtab = patch_headers(
        &mut out,
        &passes,
        &sects,
        &seg_positions,
        symtab_pos,
        dysymtab_pos,
    );
    for pass in &passes {
        let hdr = &sects[usize::from(pass.ord) - 1];
        if hdr.reloff != 0 && hdr.nreloc != 0 {
            let input = &bytes[hdr.reloff as usize..][..hdr.nreloc as usize * RELOC_SIZE];
            let tbl = shift_off(&passes, hdr.reloff) as usize;
            let kept = filter_relocs(&mut out, input, pass, tbl);
            put32(&mut out, hdr.hdr_pos + 60, kept);
        }
    }
    patch_unsigned_addends(&mut out, &sects, &passes)?;
    let dead_rows = match symtab {
        Some(symtab) => move_rows(&mut out, &sects, &passes, symtab, dead_at),
        None => Vec::new(),
    };
    Ok(Some(StrippedMember {
        bytes: out,
        dead_rows,
    }))
}

/// Point every header at the rewritten layout: section sizes,
/// addresses and file offsets, segment sizes, the symbol and string
/// table offsets and the `LC_DYSYMTAB` offsets. Returns the moved
/// symbol table as `(symoff, nsyms)`.
fn patch_headers(
    out: &mut [u8],
    passes: &[Pass],
    sects: &[SectHdr],
    seg_positions: &[usize],
    symtab_pos: Option<usize>,
    dysymtab_pos: Option<usize>,
) -> Option<(usize, usize)> {
    for s in sects {
        let p = s.hdr_pos;
        if let Some(pass) = passes.iter().find(|q| q.ord == s.ord) {
            put64(out, p + 40, pass.old_size - pass.shift);
        }
        put64(out, p + 32, shift_addr(passes, s.addr));
        if s.offset != 0 {
            put32(out, p + 48, shift_off(passes, s.offset));
        }
        if s.reloff != 0 {
            put32(out, p + 56, shift_off(passes, s.reloff));
        }
    }
    let total: u64 = passes.iter().map(|p| p.shift).sum();
    for &p in seg_positions {
        put64(out, p + 32, rd64(out, p + 32) - total);
        put64(out, p + 48, rd64(out, p + 48) - total);
    }
    // LC_DYSYMTAB file-offset fields (tocoff/modtaboff/extrefsymoff/
    // indirectsymoff/extreloff/locreloff)
    if let Some(p) = dysymtab_pos {
        for field in [32, 40, 48, 56, 64, 72].map(|f| p + f) {
            let v = rd32(out, field);
            if v != 0 {
                put32(out, field, shift_off(passes, v));
            }
        }
    }
    let p = symtab_pos?;
    let symoff = shift_off(passes, rd32(out, p + 8));
    put32(out, p + 8, symoff);
    put32(out, p + 16, shift_off(passes, rd32(out, p + 16)));
    Some((symoff as usize, rd32(out, p + 12) as usize))
}

/// The passes the section-at-a-time rewrite would make, in order.
fn plan(
    bytes: &[u8],
    symtab_pos: Option<usize>,
    sects: &[SectHdr],
    cuts: &[SectionCut<'_>],
) -> Result<Vec<Pass>, String> {
    let mut passes: Vec<Pass> = Vec::new();
    for cut in cuts {
        let hdr = sects
            .iter()
            .find(|s| s.ord == cut.ord)
            .ok_or("strip section header missing")?;
        let (runs, new_size) = build_runs(cut.atoms_rel, cut.live, run_modulus(hdr));
        if new_size >= hdr.size
            && !dead_content_in_prestripped(bytes, symtab_pos, hdr, cut.atoms_rel, cut.live)
        {
            continue;
        }
        passes.push(Pass {
            ord: cut.ord,
            addr: shift_addr(&passes, hdr.addr),
            old_size: hdr.size,
            shift: hdr.size - new_size,
            file_off: shift_off(&passes, hdr.offset) as usize,
            in_off: hdr.offset as usize,
            runs,
        });
    }
    Ok(passes)
}

/// The input with each cut section's payload replaced by its runs.
fn copy_spans(bytes: &[u8], passes: &[Pass]) -> Vec<u8> {
    let total: u64 = passes.iter().map(|p| p.shift).sum();
    let mut by_off: Vec<&Pass> = passes.iter().collect();
    by_off.sort_unstable_by_key(|p| p.in_off);
    let mut out = Vec::with_capacity(bytes.len() - total as usize);
    let mut cur = 0;
    for p in by_off {
        out.extend_from_slice(&bytes[cur..p.in_off]);
        let base = out.len();
        out.resize(base + (p.old_size - p.shift) as usize, 0);
        for r in &p.runs {
            let src = p.in_off + r.old_start as usize;
            let dst = base + r.new_start as usize;
            out[dst..dst + r.len as usize].copy_from_slice(&bytes[src..src + r.len as usize]);
        }
        cur = p.in_off + p.old_size as usize;
    }
    out.extend_from_slice(&bytes[cur..]);
    out
}
