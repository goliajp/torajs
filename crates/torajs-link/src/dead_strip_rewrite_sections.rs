//! Drop the dead atoms of several sections of one member `.o` in a
//! single rewrite. The bytes are those of rewriting one section at a
//! time in section order (see `dead_strip_rewrite`), each pass seeing
//! the headers the previous one shifted; the passes are planned in
//! that order ([`crate::dead_strip_cut::Cut`]) and every moved value is
//! pushed through them in the same order, but the member is copied,
//! and its nlist and reloc tables are walked, once.

use crate::dead_strip_rewrite::{Run, SectHdr, build_runs, run_modulus};
use crate::runtime_prestrip::dead_content_in_prestripped;

/// One section to cut: its atoms as section-relative `[start, end)`
/// ranges and which of them are live.
pub(crate) struct SectionCut<'r> {
    pub(crate) ord: u8,
    pub(crate) atoms_rel: &'r [(u64, u64)],
    pub(crate) live: &'r [bool],
}

/// One section the rewrite shortens, with the address and file offset
/// it has once the passes before it are applied.
#[derive(Debug)]
pub(crate) struct Pass {
    pub(crate) ord: u8,
    pub(crate) addr: u64,
    pub(crate) old_size: u64,
    pub(crate) shift: u64,
    file_off: usize,
    /// payload offset in the input
    pub(crate) in_off: usize,
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

    /// The section-relative offset in the input of the kept offset `o`
    /// of the section as cut; `None` in the padding between runs.
    pub(crate) fn unmap_rel(&self, o: u64) -> Option<u64> {
        let i = self.runs.partition_point(|r| r.new_start <= o);
        let r = self.runs.get(i.checked_sub(1)?)?;
        (o < r.new_start + r.len).then(|| r.old_start + (o - r.new_start))
    }
}

/// An address outside the cut sections, after every pass.
pub(crate) fn shift_addr(passes: &[Pass], mut v: u64) -> u64 {
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

pub(crate) fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn put64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// Rewrite `bytes` keeping only the live atoms of each of `cuts`
/// (ascending section order). A section whose cut buys no bytes is
/// left alone unless the runtime pre-strip left dead content in it;
/// `None` when that leaves every section alone.
#[cfg(test)]
pub(crate) fn strip_sections(
    bytes: &[u8],
    cuts: &[SectionCut<'_>],
    dead_at: crate::dead_strip_rewrite::DeadRowPlacement,
) -> Result<Option<crate::dead_strip_rewrite::StrippedMember>, String> {
    crate::dead_strip_cut::Cut::plan(bytes, cuts, dead_at, false)?
        .map(|c| c.write(bytes))
        .transpose()
}

/// Point every header at the rewritten layout: section sizes,
/// addresses and file offsets, segment sizes, the symbol and string
/// table offsets and the `LC_DYSYMTAB` offsets. Returns the moved
/// symbol table as `(symoff, nsyms)`.
pub(crate) fn patch_headers(
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
pub(crate) fn plan(
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
pub(crate) fn copy_spans(bytes: &[u8], passes: &[Pass]) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dead_strip_rewrite::{DeadRowPlacement, scan_load_commands, strip_member_section};
    use crate::dead_strip_rewrite_tables::WALKED_ROWS;
    use torajs_obj::{MH_MAGIC_64, MachHeader64, NLIST_64_SIZE, SECTION_64_SIZE};

    /// The cuts one after another, each a one-section rewrite of the
    /// previous result: what `strip_sections` stands in for. Returns
    /// the bytes, the dead rows in pass order and how many passes
    /// rewrote anything.
    fn chained(
        bytes: &[u8],
        cuts: &[SectionCut<'_>],
        dead_at: DeadRowPlacement,
    ) -> (Vec<u8>, Vec<u32>, usize) {
        let (mut cur, mut dead, mut passes) = (bytes.to_vec(), Vec::new(), 0);
        for c in cuts {
            let m = strip_member_section(&cur, c.ord, c.atoms_rel, c.live, dead_at).unwrap();
            if let Some(m) = m {
                cur = m.bytes;
                dead.extend(m.dead_rows);
                passes += 1;
            }
        }
        (cur, dead, passes)
    }

    fn one_shot(
        bytes: &[u8],
        cuts: &[SectionCut<'_>],
        dead_at: DeadRowPlacement,
    ) -> (Vec<u8>, Vec<u32>) {
        strip_sections(bytes, cuts, dead_at)
            .unwrap()
            .map_or_else(|| (bytes.to_vec(), Vec::new()), |m| (m.bytes, m.dead_rows))
    }

    /// Whether the cut sections' payloads sit in the file in another
    /// order than their ordinals.
    fn file_order_differs(bytes: &[u8], cuts: &[SectionCut<'_>]) -> bool {
        let (_, _, _, sects) = scan_load_commands(bytes).unwrap();
        let offs: Vec<u32> = cuts
            .iter()
            .map(|c| sects[usize::from(c.ord) - 1].offset)
            .collect();
        offs.windows(2).any(|w| w[0] > w[1])
    }

    struct Sect {
        seg: &'static str,
        name: &'static str,
        addr: u64,
        align: u32,
        flags: u32,
        /// `(r_address, r_info)`
        relocs: Vec<(u32, u32)>,
        /// `(offset, value)` 8-byte words written into the payload
        words: Vec<(usize, u64)>,
    }

    const SIZE: u64 = 0x40;

    fn info(sym: u32, ext: bool, r_type: u8) -> u32 {
        sym | 3 << 25 | u32::from(ext) << 27 | u32::from(r_type) << 28
    }

    fn name16(s: &str) -> [u8; 16] {
        let mut n = [0u8; 16];
        n[..s.len()].copy_from_slice(s.as_bytes());
        n
    }

    fn w32(b: &mut [u8], o: usize, v: u32) {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn w64(b: &mut [u8], o: usize, v: u64) {
        b[o..o + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// A `.o` holding `sects` (each `SIZE` bytes) whose payloads sit in
    /// the file in `file_order` (indices into `sects`), then the reloc
    /// tables, the nlist rows `(name, n_type, n_sect, n_value)` and the
    /// strings.
    fn object(sects: &[Sect], file_order: &[usize], rows: &[(String, u8, u8, u64)]) -> Vec<u8> {
        let seg_size = 72 + SECTION_64_SIZE as usize * sects.len();
        let cmds = seg_size + 24;
        let mut off = MachHeader64::SIZE + cmds;
        let mut payload_off = vec![0usize; sects.len()];
        for &i in file_order {
            payload_off[i] = off;
            off += SIZE as usize;
        }
        let mut reloff = vec![0usize; sects.len()];
        for (i, s) in sects.iter().enumerate() {
            if !s.relocs.is_empty() {
                reloff[i] = off;
                off += s.relocs.len() * 8;
            }
        }
        let symoff = off;
        let mut strtab = vec![0u8];
        let mut strx = Vec::new();
        for r in rows {
            strx.push(strtab.len() as u32);
            strtab.extend_from_slice(r.0.as_bytes());
            strtab.push(0);
        }
        let stroff = symoff + rows.len() * NLIST_64_SIZE as usize;
        let mut b = vec![0u8; stroff + strtab.len()];
        w32(&mut b, 0, MH_MAGIC_64);
        w32(&mut b, 4, torajs_obj::CPU_TYPE_ARM64);
        w32(&mut b, 12, torajs_obj::MH_OBJECT);
        w32(&mut b, 16, 2);
        w32(&mut b, 20, cmds as u32);
        w32(&mut b, 24, torajs_obj::MH_SUBSECTIONS_VIA_SYMBOLS);
        let seg = MachHeader64::SIZE;
        let total = SIZE * sects.len() as u64;
        w32(&mut b, seg, torajs_obj::LC_SEGMENT_64);
        w32(&mut b, seg + 4, seg_size as u32);
        w64(&mut b, seg + 32, total);
        w64(&mut b, seg + 40, (MachHeader64::SIZE + cmds) as u64);
        w64(&mut b, seg + 48, total);
        w32(&mut b, seg + 64, sects.len() as u32);
        for (i, s) in sects.iter().enumerate() {
            let p = seg + 72 + i * SECTION_64_SIZE as usize;
            b[p..p + 16].copy_from_slice(&name16(s.name));
            b[p + 16..p + 32].copy_from_slice(&name16(s.seg));
            w64(&mut b, p + 32, s.addr);
            w64(&mut b, p + 40, SIZE);
            w32(&mut b, p + 48, payload_off[i] as u32);
            w32(&mut b, p + 52, s.align);
            w32(&mut b, p + 56, reloff[i] as u32);
            w32(&mut b, p + 60, s.relocs.len() as u32);
            w32(&mut b, p + 64, s.flags);
            for k in 0..SIZE as usize {
                b[payload_off[i] + k] = (0x40 * (i + 1) + k) as u8 | 1;
            }
            for &(o, v) in &s.words {
                w64(&mut b, payload_off[i] + o, v);
            }
            for (k, &(addr, inf)) in s.relocs.iter().enumerate() {
                w32(&mut b, reloff[i] + k * 8, addr);
                w32(&mut b, reloff[i] + k * 8 + 4, inf);
            }
        }
        let st = seg + seg_size;
        w32(&mut b, st, torajs_obj::LC_SYMTAB);
        w32(&mut b, st + 4, 24);
        w32(&mut b, st + 8, symoff as u32);
        w32(&mut b, st + 12, rows.len() as u32);
        w32(&mut b, st + 16, stroff as u32);
        w32(&mut b, st + 20, strtab.len() as u32);
        for (k, r) in rows.iter().enumerate() {
            let p = symoff + k * NLIST_64_SIZE as usize;
            w32(&mut b, p, strx[k]);
            b[p + 4] = r.1;
            b[p + 5] = r.2;
            w64(&mut b, p + 8, r.3);
        }
        b[stroff..].copy_from_slice(&strtab);
        b
    }

    /// Several sections cut together, laid out in the file in another
    /// order than their ordinals, with section-keyed address addends
    /// into the cut sections, a SUBTRACTOR pair, a row outside its own
    /// section and an uncut section whose address range one pass
    /// shifts only part of: one rewrite gives the bytes and the dead
    /// rows of the one-section rewrites made one after another.
    #[test]
    fn several_sections_in_one_rewrite_match_one_section_at_a_time() {
        use torajs_obj::{ARM64_RELOC_BRANCH26 as BR, ARM64_RELOC_UNSIGNED as UNS};
        let sub = 1;
        let sect = |seg, name, addr, align, flags| Sect {
            seg,
            name,
            addr,
            align,
            flags,
            relocs: Vec::new(),
            words: Vec::new(),
        };
        let mut sects = [
            sect(
                "__TEXT",
                "__text",
                0,
                2,
                torajs_obj::S_ATTR_PURE_INSTRUCTIONS,
            ),
            sect("__DATA", "__data", 0x40, 3, 0),
            sect("__TEXT", "__const", 0x80, 4, 0),
            // overlaps the end of `__data`, which the `__data` pass moves
            sect("__DATA", "__odd", 0x70, 3, 0),
        ];
        sects[0].relocs = vec![(0x24, info(0, true, BR)), (0x04, info(1, true, BR))];
        // section-keyed pointers into `__text`, `__const` (a dead atom)
        // and `__data` itself, an extern pointer in a dead atom, and a
        // SUBTRACTOR pair
        sects[1].relocs = vec![
            (0x00, info(1, false, UNS)),
            (0x10, info(3, false, UNS)),
            (0x20, info(0, true, UNS)),
            (0x18, info(2, false, UNS)),
            (0x38, info(0, true, sub)),
            (0x38, info(1, true, UNS)),
        ];
        sects[1].words = vec![(0x00, 0x20), (0x10, 0xA8), (0x18, 0x78), (0x38, 0x99)];
        let (ext, loc, und) = (0x0f, 0x0e, 0x01);
        let mut rows = vec![("_u0".to_string(), und, 0, 0), ("_u1".into(), und, 0, 0)];
        for (i, s) in sects.iter().enumerate().take(3) {
            for k in 0..4u64 {
                let t = if k % 2 == 0 { ext } else { loc };
                rows.push((format!("_s{i}_{k}"), t, i as u8 + 1, s.addr + 0x10 * k));
            }
        }
        rows.push(("_odd0".into(), ext, 4, 0x70));
        rows.push(("_odd1".into(), loc, 4, 0x98));
        // n_sect says `__data`, the value lies in `__const`
        rows.push(("_stray".into(), loc, 2, 0x88));
        let obj = object(&sects, &[1, 2, 3, 0], &rows);
        let atoms: Vec<(u64, u64)> = (0..4).map(|k| (0x10 * k, 0x10 * k + 0x10)).collect();
        let lives = [
            [true, false, true, false],
            [true, true, false, true],
            [false, true, false, true],
        ];
        let cuts: Vec<SectionCut<'_>> = (0..3)
            .map(|i| SectionCut {
                ord: i as u8 + 1,
                atoms_rel: &atoms,
                live: &lives[i],
            })
            .collect();
        assert!(file_order_differs(&obj, &cuts));
        for dead_at in [DeadRowPlacement::SectionBase, DeadRowPlacement::GapStart] {
            let (want, want_dead, passes) = chained(&obj, &cuts, dead_at);
            assert_eq!(passes, 3);
            WALKED_ROWS.with(|w| w.set(0));
            let (got, got_dead) = one_shot(&obj, &cuts, dead_at);
            // `_stray` and both `__odd` rows
            assert_eq!(WALKED_ROWS.with(|w| w.get()), 3);
            // two dead atoms in `__text` and `__const`, one in
            // `__data`, and `_stray`, past the last run of `__data`
            assert_eq!(got_dead.len(), 6);
            assert_eq!(got_dead, want_dead);
            assert!(got == want);
            // and the link reads it through the cut as written
            let read =
                crate::member_obj_check::check_cut_reads_as_written("t.o", &obj, &cuts, dead_at);
            assert_eq!(read, Some(3));
        }
    }

    /// Every member of the embedded runtime, as built and pre-stripped,
    /// under a few made-up liveness patterns: one rewrite gives what
    /// cutting its sections one at a time gives.
    #[test]
    fn runtime_members_rewrite_as_one_section_at_a_time() {
        use crate::archives_merge::merge_archive_indexes;
        use crate::dead_strip_reach_build::build_member_reach;
        use crate::dead_strip_repack::eligible_sections;
        let original = torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE;
        let pre = crate::runtime_prestrip::prestrip_image(original).unwrap();
        let (mut checked, mut multi, mut reordered) = (0, 0, 0);
        for (image, dead_at) in [
            (original, DeadRowPlacement::GapStart),
            (&pre[..], DeadRowPlacement::SectionBase),
        ] {
            let merged = merge_archive_indexes(image).unwrap();
            for a in 0..merged.archive_count() {
                for m in 0..merged.member_count(a) {
                    let member = merged.member(a, m);
                    let mut reach = build_member_reach(&member).unwrap();
                    for pattern in 0..3usize {
                        for (si, sa) in reach.sects.iter_mut().enumerate() {
                            let keep_all = (si + pattern) % 4 == 3;
                            for (i, l) in sa.live.iter_mut().enumerate() {
                                *l = keep_all || (i * 7 + si * 3 + pattern) % 4 != 0;
                            }
                        }
                        let eligible = eligible_sections(&reach);
                        let cuts: Vec<SectionCut<'_>> = eligible
                            .iter()
                            .map(|(ord, atoms_rel, live)| SectionCut {
                                ord: *ord,
                                atoms_rel,
                                live,
                            })
                            .collect();
                        let (want, want_dead, passes) = chained(member.data, &cuts, dead_at);
                        let (got, got_dead) = one_shot(member.data, &cuts, dead_at);
                        assert_eq!(got_dead, want_dead, "{}", member.name);
                        assert!(got == want, "{}: bytes", member.name);
                        checked += 1;
                        multi += usize::from(passes > 1);
                        let moved = passes > 1 && file_order_differs(member.data, &cuts);
                        reordered += usize::from(moved);
                    }
                }
            }
        }
        eprintln!("checked {checked}, several sections {multi}, file order differs {reordered}");
        assert!(checked > 1000, "{checked} rewrites");
        assert!(multi > 100, "{multi} rewrites of several sections");
    }
}
