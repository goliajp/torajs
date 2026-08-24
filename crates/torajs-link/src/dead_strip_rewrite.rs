//! S2 dead-strip blades 2b/3 — member `.o` rewriter (RFC
//! 20260824-s2-dead-strip, plan C "input normalization").
//!
//! Takes one required member's Mach-O bytes plus one section's atom
//! liveness and emits a new `.o` whose section keeps only the live
//! atoms. Blade 3 generalized the blade-2b `__TEXT,__text` pass to
//! any regular file-storage section; the driver
//! (`dead_strip_repack::strip_member`) applies it once per eligible
//! section, re-scanning load commands between passes so each pass
//! sees the previous pass's shifted headers. The downstream link
//! pipeline is untouched — it sees a `.o` that was simply always
//! this small.
//!
//! Rewrite shape (single-shift design, per pass): the output is
//! three spans — `[0, sect_off)` verbatim, the concatenated live
//! runs, and `[sect_off + old_size, ..)` verbatim — so every
//! absolute file offset after the stripped payload moves by exactly
//! one constant `shift`. Physical table sizes are preserved (the
//! stripped section's reloc table is filtered in place with
//! `nreloc` lowered and the tail zeroed; dead symbols keep their
//! nlist rows with `n_value` clamped to the section base) so no
//! second shift source exists and extern reloc indices stay valid.
//!
//! Address-space account:
//!   - live atoms move to their run-remapped address, each run
//!     placed congruent to its old address mod
//!     `max(16, 1 << align)` so every intra-section offset keeps
//!     the address class the compiler relied on (the final layout
//!     re-aligns each section's placement and resolves symbols as
//!     `final_vaddr + (n_value - member_addr)`, so only the class
//!     matters, not the absolute value);
//!   - every later section's `addr` and `offset` drop by `shift`,
//!     so `n_value - member_addr` stays invariant for later-section
//!     symbols (both sides move together);
//!   - stored 8-byte addends of section-keyed UNSIGNED relocs are
//!     object-file addresses and are rewritten (stripped-section
//!     targets run-remapped, later-section targets shifted);
//!     symbol-keyed addends are target-relative and stay untouched.

use torajs_obj::{
    ARM64_RELOC_UNSIGNED, LC_SEGMENT_64, LC_SYMTAB, MH_MAGIC_64, N_SECT, N_TYPE, NLIST_64_SIZE,
    RELOCATION_INFO_SIZE, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
};

const LC_DYSYMTAB: u32 = 0xb;
const ARM64_RELOC_SUBTRACTOR: u8 = 1;

/// One kept run of live `__text` bytes: `[old_start, old_start+len)`
/// lands at `new_start` in the rewritten section.
struct Run {
    old_start: u64,
    len: u64,
    new_start: u64,
}

pub(crate) struct SectHdr {
    /// File position of this `section_64` header inside the `.o`.
    hdr_pos: usize,
    ord: u8,
    addr: u64,
    size: u64,
    offset: u32,
    align: u8,
    pub(crate) reloff: u32,
    pub(crate) nreloc: u32,
}

/// Rewrite `bytes` (one member `.o`) keeping only the live atoms
/// of section `strip_ord`. `atoms_rel` / `live` carry the section's
/// atom table as section-relative `[start, end)` ranges (relative
/// offsets survive earlier passes over other sections; absolute
/// reach addresses do not). Returns `None` when stripping buys
/// nothing (no dead bytes after run alignment).
pub(crate) fn strip_member_section(
    bytes: &[u8],
    strip_ord: u8,
    atoms_rel: &[(u64, u64)],
    live: &[bool],
) -> Result<Option<Vec<u8>>, String> {
    // Scan first — the header carries the section's CURRENT
    // addr/size (earlier strip passes over the same member shift
    // them; the reach pass's copies are stale after the first).
    let (seg_positions, symtab_pos, dysymtab_pos, sects) = scan_load_commands(bytes)?;
    let hdr = sects
        .iter()
        .find(|s| s.ord == strip_ord)
        .ok_or("strip section header missing")?;
    let sect_addr = hdr.addr;
    let old_size = hdr.size;

    let (runs, new_size) = build_runs(atoms_rel, live, hdr.align);
    if new_size >= old_size {
        return Ok(None);
    }
    let shift = old_size - new_size;
    let old_end = sect_addr + old_size;
    let sect_off = hdr.offset as usize;
    let old_size_us = old_size as usize;

    // Absolute object-file address in, absolute out; run offsets
    // are intra-section.
    let remap = |addr: u64| -> Option<u64> {
        let o = addr.checked_sub(sect_addr)?;
        let i = runs.partition_point(|r| r.old_start <= o);
        if i == 0 {
            return None;
        }
        let r = &runs[i - 1];
        (o < r.old_start + r.len).then(|| sect_addr + r.new_start + (o - r.old_start))
    };

    // Three-span copy.
    let mut out = Vec::with_capacity(bytes.len() - shift as usize);
    out.extend_from_slice(&bytes[..sect_off]);
    let new_base = out.len();
    out.resize(new_base + new_size as usize, 0);
    for r in &runs {
        let src = sect_off + r.old_start as usize;
        let dst = new_base + r.new_start as usize;
        out[dst..dst + r.len as usize].copy_from_slice(&bytes[src..src + r.len as usize]);
    }
    out.extend_from_slice(&bytes[sect_off + old_size_us..]);

    let sh = |off: u32| -> u32 {
        if off as usize >= sect_off + old_size_us {
            off - shift as u32
        } else {
            off
        }
    };

    // Patch section headers.
    for s in &sects {
        let p = s.hdr_pos;
        if s.ord == strip_ord {
            out[p + 40..p + 48].copy_from_slice(&new_size.to_le_bytes());
        } else {
            if s.addr >= old_end {
                out[p + 32..p + 40].copy_from_slice(&(s.addr - shift).to_le_bytes());
            }
            if s.offset != 0 {
                out[p + 48..p + 52].copy_from_slice(&sh(s.offset).to_le_bytes());
            }
        }
        if s.reloff != 0 {
            out[p + 56..p + 60].copy_from_slice(&sh(s.reloff).to_le_bytes());
        }
    }
    // Segment vmsize/filesize.
    for &p in &seg_positions {
        let vmsize = u64::from_le_bytes(out[p + 32..p + 40].try_into().unwrap());
        out[p + 32..p + 40].copy_from_slice(&(vmsize - shift).to_le_bytes());
        let filesize = u64::from_le_bytes(out[p + 48..p + 56].try_into().unwrap());
        out[p + 48..p + 56].copy_from_slice(&(filesize - shift).to_le_bytes());
    }
    // LC_SYMTAB symoff/stroff.
    let mut symtab = None;
    if let Some(p) = symtab_pos {
        let symoff = u32::from_le_bytes(out[p + 8..p + 12].try_into().unwrap());
        let nsyms = u32::from_le_bytes(out[p + 12..p + 16].try_into().unwrap());
        let stroff = u32::from_le_bytes(out[p + 16..p + 20].try_into().unwrap());
        out[p + 8..p + 12].copy_from_slice(&sh(symoff).to_le_bytes());
        out[p + 16..p + 20].copy_from_slice(&sh(stroff).to_le_bytes());
        symtab = Some((sh(symoff), nsyms));
    }
    // LC_DYSYMTAB file-offset fields (tocoff/modtaboff/extrefsymoff/
    // indirectsymoff/extreloff/locreloff at +32/+40/+48/+56/+64/+72).
    if let Some(p) = dysymtab_pos {
        for field in [32usize, 40, 48, 56, 64, 72] {
            let v = u32::from_le_bytes(out[p + field..p + field + 4].try_into().unwrap());
            if v != 0 {
                out[p + field..p + field + 4].copy_from_slice(&sh(v).to_le_bytes());
            }
        }
    }

    // Filter + remap the stripped section's reloc table in place
    // (physical size preserved; nreloc lowered; tail zeroed).
    if hdr.reloff != 0 && hdr.nreloc != 0 {
        let tbl = sh(hdr.reloff) as usize;
        let n = hdr.nreloc as usize;
        let esz = RELOCATION_INFO_SIZE as usize;
        let entries: Vec<[u8; 8]> = (0..n)
            .map(|i| out[tbl + i * esz..tbl + (i + 1) * esz].try_into().unwrap())
            .collect();
        let mut kept: Vec<[u8; 8]> = Vec::with_capacity(n);
        let mut i = 0;
        while i < n {
            // ARM64_RELOC_ADDEND precedes its mate with the same
            // r_address — keep/drop the pair together.
            let group = addend_group_len(&entries, i);
            let last = &entries[i + group - 1];
            let r_address = u32::from_le_bytes(last[0..4].try_into().unwrap());
            match remap(sect_addr + u64::from(r_address)) {
                Some(new_abs) => {
                    let new_rel = new_abs - sect_addr;
                    for e in &entries[i..i + group] {
                        let mut ne = *e;
                        ne[0..4].copy_from_slice(&(new_rel as u32).to_le_bytes());
                        kept.push(ne);
                    }
                }
                None => {} // site is in a dead atom — drop
            }
            i += group;
        }
        for (j, e) in kept.iter().enumerate() {
            out[tbl + j * esz..tbl + (j + 1) * esz].copy_from_slice(e);
        }
        for b in &mut out[tbl + kept.len() * esz..tbl + n * esz] {
            *b = 0;
        }
        let p = hdr.hdr_pos;
        out[p + 60..p + 64].copy_from_slice(&(kept.len() as u32).to_le_bytes());
    }

    // Rewrite stored addends of section-keyed UNSIGNED relocs in
    // every section (the addend is an object-file address).
    patch_unsigned_addends(
        &mut out,
        &sects,
        strip_ord,
        sect_addr,
        old_end,
        sect_off + old_size_us,
        shift,
        &remap,
    )?;

    // Patch nlist n_value: live stripped-section symbols remap,
    // dead ones clamp to the section base; later-section symbols
    // shift down.
    if let Some((symoff, nsyms)) = symtab {
        for i in 0..nsyms as usize {
            let p = symoff as usize + i * NLIST_64_SIZE as usize;
            let n_type = out[p + 4];
            let n_sect = out[p + 5];
            if n_type & N_TYPE != N_SECT {
                continue;
            }
            let n_value = u64::from_le_bytes(out[p + 8..p + 16].try_into().unwrap());
            let new = if n_sect == strip_ord {
                remap(n_value).unwrap_or(sect_addr)
            } else if n_value >= old_end {
                n_value - shift
            } else {
                n_value
            };
            out[p + 8..p + 16].copy_from_slice(&new.to_le_bytes());
        }
    }

    Ok(Some(out))
}

/// Merge adjacent live atoms into placement runs. Each run lands at
/// the smallest offset ≥ the running cursor congruent to its old
/// offset mod `a = max(16, 1 << align)` — preserving every atom's
/// address class (function atoms conventionally get 16 even when
/// the section header says less). Offsets are section-relative;
/// returns the runs plus the new payload size.
fn build_runs(atoms_rel: &[(u64, u64)], live: &[bool], align: u8) -> (Vec<Run>, u64) {
    let a = (1u64 << u32::from(align.min(14))).max(16);
    let mut runs: Vec<Run> = Vec::new();
    let mut cursor: u64 = 0;
    for (i, &(rs, re)) in atoms_rel.iter().enumerate() {
        if !live[i] || re == rs {
            continue;
        }
        if let Some(last) = runs.last_mut()
            && last.old_start + last.len == rs
        {
            last.len += re - rs;
            cursor += re - rs;
            continue;
        }
        let new_start = cursor + (rs % a + a - cursor % a) % a;
        runs.push(Run {
            old_start: rs,
            len: re - rs,
            new_start,
        });
        cursor = new_start + (re - rs);
    }
    (runs, cursor)
}

/// Length of the reloc group starting at `i`: an `ARM64_RELOC_ADDEND`
/// entry (r_type 10) binds to the entry after it.
fn addend_group_len(entries: &[[u8; 8]], i: usize) -> usize {
    let info = u32::from_le_bytes(entries[i][4..8].try_into().unwrap());
    let r_type = (info >> 28) & 0xF;
    if r_type == 10 && i + 1 < entries.len() {
        2
    } else {
        1
    }
}

/// Rewrite the 8-byte addends of section-keyed (`r_extern=0`)
/// UNSIGNED relocs: stripped-section targets remap through the run
/// table, later sections shift down. SUBTRACTOR pairs are skipped
/// (their stored value is a difference, not an address).
#[allow(clippy::too_many_arguments)]
fn patch_unsigned_addends(
    out: &mut [u8],
    sects: &[SectHdr],
    strip_ord: u8,
    sect_addr: u64,
    old_end: u64,
    strip_file_end: usize,
    shift: u64,
    remap: &dyn Fn(u64) -> Option<u64>,
) -> Result<(), String> {
    let shift_us = shift as usize;
    for s in sects {
        if s.reloff == 0 || s.nreloc == 0 {
            continue;
        }
        // Table position in the REWRITTEN buffer (one shift source).
        let tbl = if s.reloff as usize >= strip_file_end {
            s.reloff as usize - shift_us
        } else {
            s.reloff as usize
        };
        let payload = if s.ord == strip_ord {
            s.offset as usize // rebuilt in place; sites use NEW r_address
        } else if s.offset == 0 {
            continue; // zerofill: no stored addends
        } else if s.offset as usize >= strip_file_end {
            s.offset as usize - shift_us
        } else {
            s.offset as usize
        };
        let n = if s.ord == strip_ord {
            // nreloc was lowered in the filter pass; re-read it.
            u32::from_le_bytes(out[s.hdr_pos + 60..s.hdr_pos + 64].try_into().unwrap()) as usize
        } else {
            s.nreloc as usize
        };
        let esz = RELOCATION_INFO_SIZE as usize;
        let mut i = 0;
        while i < n {
            let e: [u8; 8] = out[tbl + i * esz..tbl + (i + 1) * esz].try_into().unwrap();
            let info = u32::from_le_bytes(e[4..8].try_into().unwrap());
            let r_type = ((info >> 28) & 0xF) as u8;
            if r_type == ARM64_RELOC_SUBTRACTOR {
                i += 2; // skip the pair — stored value is a difference
                continue;
            }
            let r_extern = ((info >> 27) & 0x1) as u8;
            let r_length = ((info >> 25) & 0x3) as u8;
            let r_pcrel = ((info >> 24) & 0x1) as u8;
            let target_sect = (info & 0x00FF_FFFF) as u8;
            if r_extern == 0 && r_type == ARM64_RELOC_UNSIGNED && r_length == 3 && r_pcrel == 0 {
                let r_address = u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize;
                let site = payload + r_address;
                if out.len() < site + 8 {
                    return Err(format!("sect {} addend site out of range", s.ord));
                }
                let addend = u64::from_le_bytes(out[site..site + 8].try_into().unwrap());
                let new = if target_sect == strip_ord {
                    remap(addend).unwrap_or(sect_addr)
                } else if addend >= old_end {
                    addend - shift
                } else {
                    addend
                };
                out[site..site + 8].copy_from_slice(&new.to_le_bytes());
            }
            i += 1;
        }
    }
    Ok(())
}

/// Walk load commands once, returning every patch position.
#[allow(clippy::type_complexity)]
pub(crate) fn scan_load_commands(
    bytes: &[u8],
) -> Result<(Vec<usize>, Option<usize>, Option<usize>, Vec<SectHdr>), String> {
    if bytes.len() < 32 {
        return Err("truncated header".into());
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Err("not MH_MAGIC_64".into());
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut seg_positions = Vec::new();
    let mut symtab_pos = None;
    let mut dysymtab_pos = None;
    let mut sects = Vec::new();
    let mut ord: u8 = 0;
    let mut cursor = 32usize;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(format!("truncated LC at {cursor}"));
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(format!("bad LC size at {cursor}"));
        }
        match cmd {
            LC_SEGMENT_64 => {
                seg_positions.push(cursor);
                let nsects = u32::from_le_bytes(bytes[cursor + 64..cursor + 68].try_into().unwrap())
                    as usize;
                let base = cursor + SEGMENT_COMMAND_64_SIZE as usize;
                for i in 0..nsects {
                    let p = base + i * SECTION_64_SIZE as usize;
                    if bytes.len() < p + SECTION_64_SIZE as usize {
                        return Err("truncated section header".into());
                    }
                    ord += 1;
                    sects.push(SectHdr {
                        hdr_pos: p,
                        ord,
                        addr: u64::from_le_bytes(bytes[p + 32..p + 40].try_into().unwrap()),
                        size: u64::from_le_bytes(bytes[p + 40..p + 48].try_into().unwrap()),
                        offset: u32::from_le_bytes(bytes[p + 48..p + 52].try_into().unwrap()),
                        align: u32::from_le_bytes(bytes[p + 52..p + 56].try_into().unwrap()).min(31)
                            as u8,
                        reloff: u32::from_le_bytes(bytes[p + 56..p + 60].try_into().unwrap()),
                        nreloc: u32::from_le_bytes(bytes[p + 60..p + 64].try_into().unwrap()),
                    });
                }
            }
            LC_SYMTAB => symtab_pos = Some(cursor),
            LC_DYSYMTAB => dysymtab_pos = Some(cursor),
            _ => {}
        }
        cursor += cmdsize;
    }
    Ok((seg_positions, symtab_pos, dysymtab_pos, sects))
}
