//! S2 dead-strip blade 2b — member `.o` rewriter (RFC
//! 20260824-s2-dead-strip, plan C "input normalization").
//!
//! Takes one required member's Mach-O bytes plus its reachability
//! verdict and emits a new `.o` whose `__TEXT,__text` keeps only the
//! live atoms. The downstream link pipeline is untouched — it sees a
//! `.o` that was simply always this small.
//!
//! Rewrite shape (single-shift design): the output is three spans —
//! `[0, text_off)` verbatim, the concatenated live runs, and
//! `[text_off + old_size, ..)` verbatim — so every absolute file
//! offset after `__text` moves by exactly one constant `shift`.
//! Physical table sizes are preserved (the `__text` reloc table is
//! filtered in place with `nreloc` lowered and the tail zeroed; dead
//! symbols keep their nlist rows with `n_value` clamped to 0) so no
//! second shift source exists and extern reloc indices stay valid.
//!
//! Address-space account:
//!   - live `__text` atoms move to their run-remapped address;
//!   - every later section's `addr` and `offset` drop by `shift`, so
//!     `n_value - member_addr` stays invariant for data symbols
//!     (both sides move together);
//!   - stored 8-byte addends of section-keyed UNSIGNED relocs are
//!     object-file addresses and are rewritten (text targets run-
//!     remapped, later-section targets shifted); symbol-keyed
//!     addends are target-relative and stay untouched.

use torajs_obj::{
    ARM64_RELOC_UNSIGNED, LC_SEGMENT_64, LC_SYMTAB, MH_MAGIC_64, N_SECT, N_TYPE, NLIST_64_SIZE,
    RELOCATION_INFO_SIZE, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
};

use crate::dead_strip_reach::MemberReach;

const LC_DYSYMTAB: u32 = 0xb;
const ARM64_RELOC_SUBTRACTOR: u8 = 1;

/// One kept run of live `__text` bytes: `[old_start, old_start+len)`
/// lands at `new_start` in the rewritten section.
struct Run {
    old_start: u64,
    len: u64,
    new_start: u64,
}

struct SectHdr {
    /// File position of this `section_64` header inside the `.o`.
    hdr_pos: usize,
    ord: u8,
    addr: u64,
    offset: u32,
    reloff: u32,
    nreloc: u32,
    is_text: bool,
}

/// Rewrite `bytes` (one member `.o`) keeping only the live `__text`
/// atoms recorded in `reach`. Returns `None` when there is nothing
/// to strip (no text, fully live, or the closure fell back to
/// whole-section liveness).
pub(crate) fn strip_member_text(
    bytes: &[u8],
    reach: &MemberReach<'_>,
) -> Result<Option<Vec<u8>>, String> {
    let Some((text_ord0, sa)) = reach
        .sects
        .iter()
        .enumerate()
        .find(|(_, s)| s.is_text && s.size > 0)
    else {
        return Ok(None);
    };
    let text_ord = (text_ord0 + 1) as u8;
    if sa.all_live || sa.live.iter().all(|&l| l) {
        return Ok(None);
    }

    // Merge adjacent live atoms into runs; 16-byte-align each run.
    let mut runs: Vec<Run> = Vec::new();
    let mut cursor: u64 = 0;
    for (i, &(start, end)) in sa.atoms.iter().enumerate() {
        if !sa.live[i] || end == start {
            continue;
        }
        if let Some(last) = runs.last_mut()
            && last.old_start + last.len == start
        {
            last.len += end - start;
            cursor += end - start;
            continue;
        }
        cursor = cursor.next_multiple_of(16);
        runs.push(Run {
            old_start: start,
            len: end - start,
            new_start: cursor,
        });
        cursor += end - start;
    }
    let new_text_size = cursor.next_multiple_of(16);
    let old_text_size = sa.size;
    if new_text_size >= old_text_size {
        return Ok(None);
    }
    let shift = old_text_size - new_text_size;
    let text_addr = sa.addr;
    let old_text_end = text_addr + old_text_size;

    // Scan load commands for patch positions.
    let (seg_positions, symtab_pos, dysymtab_pos, sects) = scan_load_commands(bytes)?;
    let text_hdr = sects
        .iter()
        .find(|s| s.ord == text_ord)
        .ok_or("text section header vanished")?;
    let text_off = text_hdr.offset as usize;
    let old_size_us = old_text_size as usize;

    let remap = |addr: u64| -> Option<u64> {
        let i = runs.partition_point(|r| r.old_start <= addr);
        if i == 0 {
            return None;
        }
        let r = &runs[i - 1];
        (addr < r.old_start + r.len).then(|| r.new_start + (addr - r.old_start))
    };

    // Three-span copy.
    let mut out = Vec::with_capacity(bytes.len() - shift as usize);
    out.extend_from_slice(&bytes[..text_off]);
    let text_new_base = out.len();
    out.resize(text_new_base + new_text_size as usize, 0);
    for r in &runs {
        let src = text_off + (r.old_start - text_addr) as usize;
        let dst = text_new_base + r.new_start as usize;
        out[dst..dst + r.len as usize].copy_from_slice(&bytes[src..src + r.len as usize]);
    }
    out.extend_from_slice(&bytes[text_off + old_size_us..]);

    let sh = |off: u32| -> u32 {
        if off as usize >= text_off + old_size_us {
            off - shift as u32
        } else {
            off
        }
    };

    // Patch section headers.
    for s in &sects {
        let p = s.hdr_pos;
        if s.is_text {
            out[p + 40..p + 48].copy_from_slice(&new_text_size.to_le_bytes());
        } else {
            if s.addr >= old_text_end {
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

    // Filter + remap the __text reloc table in place (physical size
    // preserved; nreloc lowered; tail zeroed).
    if text_hdr.reloff != 0 && text_hdr.nreloc != 0 {
        let tbl = sh(text_hdr.reloff) as usize;
        let n = text_hdr.nreloc as usize;
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
            match remap(text_addr + u64::from(r_address)) {
                Some(new_rel) => {
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
        let p = text_hdr.hdr_pos;
        out[p + 60..p + 64].copy_from_slice(&(kept.len() as u32).to_le_bytes());
    }

    // Rewrite stored addends of section-keyed UNSIGNED relocs in
    // every section (the addend is an object-file address).
    patch_unsigned_addends(
        &mut out,
        &sects,
        text_ord,
        text_addr,
        old_text_end,
        text_off + old_size_us,
        shift,
        &remap,
    )?;

    // Patch nlist n_value: live text symbols remap, dead clamp to
    // the (new) text base; later-section symbols shift down.
    if let Some((symoff, nsyms)) = symtab {
        for i in 0..nsyms as usize {
            let p = symoff as usize + i * NLIST_64_SIZE as usize;
            let n_type = out[p + 4];
            let n_sect = out[p + 5];
            if n_type & N_TYPE != N_SECT {
                continue;
            }
            let n_value = u64::from_le_bytes(out[p + 8..p + 16].try_into().unwrap());
            let new = if n_sect == text_ord {
                remap(n_value).unwrap_or(text_addr)
            } else if n_value >= old_text_end {
                n_value - shift
            } else {
                n_value
            };
            out[p + 8..p + 16].copy_from_slice(&new.to_le_bytes());
        }
    }

    Ok(Some(out))
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
/// UNSIGNED relocs: text targets remap through the run table, later
/// sections shift down. SUBTRACTOR pairs are skipped (their stored
/// value is a difference, not an address).
/// Rewrite the 8-byte addends of section-keyed (`r_extern=0`)
/// UNSIGNED relocs: text targets remap through the run table, later
/// sections shift down. SUBTRACTOR pairs are skipped (their stored
/// value is a difference, not an address).
#[allow(clippy::too_many_arguments)]
fn patch_unsigned_addends(
    out: &mut [u8],
    sects: &[SectHdr],
    text_ord: u8,
    text_addr: u64,
    old_text_end: u64,
    text_file_end: usize,
    shift: u64,
    remap: &dyn Fn(u64) -> Option<u64>,
) -> Result<(), String> {
    let shift_us = shift as usize;
    for s in sects {
        if s.reloff == 0 || s.nreloc == 0 {
            continue;
        }
        // Table position in the REWRITTEN buffer (one shift source).
        let tbl = if s.reloff as usize >= text_file_end {
            s.reloff as usize - shift_us
        } else {
            s.reloff as usize
        };
        let payload = if s.is_text {
            s.offset as usize // rebuilt in place; sites use NEW r_address
        } else if s.offset == 0 {
            continue; // zerofill: no stored addends
        } else if s.offset as usize >= text_file_end {
            s.offset as usize - shift_us
        } else {
            s.offset as usize
        };
        let n = if s.is_text {
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
                let new = if target_sect == text_ord {
                    remap(addend).unwrap_or(text_addr)
                } else if addend >= old_text_end {
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
fn scan_load_commands(
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
                    let sectname = &bytes[p..p + 7];
                    let segname = &bytes[p + 16..p + 23];
                    sects.push(SectHdr {
                        hdr_pos: p,
                        ord,
                        addr: u64::from_le_bytes(bytes[p + 32..p + 40].try_into().unwrap()),
                        offset: u32::from_le_bytes(bytes[p + 48..p + 52].try_into().unwrap()),
                        reloff: u32::from_le_bytes(bytes[p + 56..p + 60].try_into().unwrap()),
                        nreloc: u32::from_le_bytes(bytes[p + 60..p + 64].try_into().unwrap()),
                        is_text: sectname == b"__text\0" && segname == b"__TEXT\0",
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
