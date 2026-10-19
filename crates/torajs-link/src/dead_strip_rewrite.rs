//! S2 dead-strip blades 2b/3 — member `.o` rewriter (RFC
//! 20260824-s2-dead-strip, plan C "input normalization").
//!
//! Takes one required member's Mach-O bytes plus one section's atom
//! liveness and emits a new `.o` whose section keeps only the live
//! atoms. Blade 3 generalized the blade-2b `__TEXT,__text` pass to
//! any regular file-storage section; the driver
//! (`dead_strip_repack::strip_member`) applies it to every eligible
//! section, giving the bytes of one pass per section in section order
//! with each pass seeing the previous pass's shifted headers
//! (`dead_strip_rewrite_sections` makes all the passes in one
//! rewrite). The downstream link pipeline is untouched — it sees a
//! `.o` that was simply always this small.
//!
//! Rewrite shape (single-shift design, per pass): the output is
//! three spans — `[0, sect_off)` verbatim, the concatenated live
//! runs, and `[sect_off + old_size, ..)` verbatim — so every
//! absolute file offset after the stripped payload moves by exactly
//! one constant `shift`. Physical table sizes are preserved (the
//! stripped section's reloc table is filtered in place with
//! `nreloc` lowered and the tail zeroed; dead symbols keep their
//! nlist rows, moved to the section base or the gap they left) so no
//! second shift source exists and extern reloc indices stay valid.
//!
//! Address-space account:
//!   - live atoms move to their run-remapped address, each run
//!     placed congruent to its old address mod the section's
//!     placement modulus (see [`run_modulus`]) so every intra-section
//!     offset keeps the address class the compiler relied on (the
//!     final layout re-aligns each section's placement and resolves
//!     symbols as `final_vaddr + (n_value - member_addr)`, so only
//!     the class matters, not the absolute value);
//!   - every later section's `addr` and `offset` drop by `shift`,
//!     so `n_value - member_addr` stays invariant for later-section
//!     symbols (both sides move together);
//!   - stored 8-byte addends of section-keyed UNSIGNED relocs are
//!     object-file addresses and are rewritten (stripped-section
//!     targets run-remapped, later-section targets shifted);
//!     symbol-keyed addends are target-relative and stay untouched.

use torajs_obj::{
    LC_SEGMENT_64, LC_SYMTAB, MH_MAGIC_64, S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS,
    SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
};

const LC_DYSYMTAB: u32 = 0xb;

/// One kept run of live `__text` bytes: `[old_start, old_start+len)`
/// lands at `new_start` in the rewritten section.
pub(crate) struct Run {
    pub(crate) old_start: u64,
    pub(crate) len: u64,
    pub(crate) new_start: u64,
}

pub(crate) struct SectHdr {
    /// File position of this `section_64` header inside the `.o`.
    pub(crate) hdr_pos: usize,
    pub(crate) ord: u8,
    pub(crate) addr: u64,
    pub(crate) size: u64,
    pub(crate) offset: u32,
    align: u8,
    pub(crate) reloff: u32,
    pub(crate) nreloc: u32,
    flags: u32,
}

/// A rewritten member `.o` plus the nlist indices of the rows whose
/// atom was dropped (still defined, placed per [`DeadRowPlacement`]).
pub(crate) struct StrippedMember {
    pub(crate) bytes: Vec<u8>,
    pub(crate) dead_rows: Vec<u32>,
}

/// Where the rewriter leaves the rows of dropped atoms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeadRowPlacement {
    /// The section base. The per-program pass: a call site the link
    /// judged dead keeps its reloc to a dropped atom, and the binary
    /// resolves it there.
    SectionBase,
    /// The start of the gap the atom fell into, right after the last
    /// run before it. The runtime pre-strip: the row keeps an atom
    /// boundary on the gap, so stripping the result again against a
    /// subset of the live atoms places every run exactly as stripping
    /// the original would; at the base, the alignment padding ahead of
    /// a run would join the run before it and move later runs.
    GapStart,
}

/// Rewrite `bytes` (one member `.o`) keeping only the live atoms of
/// section `strip_ord`, given as section-relative `[start, end)`
/// ranges.
#[cfg(test)]
pub(crate) fn strip_member_section(
    bytes: &[u8],
    strip_ord: u8,
    atoms_rel: &[(u64, u64)],
    live: &[bool],
    dead_at: DeadRowPlacement,
) -> Result<Option<StrippedMember>, String> {
    let cut = crate::dead_strip_rewrite_sections::SectionCut {
        ord: strip_ord,
        atoms_rel,
        live,
    };
    crate::dead_strip_rewrite_sections::strip_sections(bytes, &[cut], dead_at)
}

/// Where a dead symbol at `addr` of the stripped section lands, as
/// `dead_at` places it: the section base, or the end of the last live
/// run before it.
pub(crate) fn dead_row_addr(
    runs: &[Run],
    dead_at: DeadRowPlacement,
    sect_addr: u64,
    addr: u64,
) -> u64 {
    let i = runs.partition_point(|r| r.old_start <= addr - sect_addr);
    match (dead_at, i) {
        (DeadRowPlacement::SectionBase, _) | (DeadRowPlacement::GapStart, 0) => sect_addr,
        (DeadRowPlacement::GapStart, _) => sect_addr + runs[i - 1].new_start + runs[i - 1].len,
    }
}

/// The modulus a run keeps its old offset under. An instruction
/// section uses its declared alignment floored at the 4-byte A64
/// instruction, as ld64 places atoms: the assembler already raised it
/// to the widest `.p2align` any function or loop head asked for, and a
/// wider modulus makes a run's padding depend on where the bytes before
/// it ended, so dead code earlier in the member would move every later
/// live byte. Other sections keep the floor of 16 they always had.
pub(crate) fn run_modulus(hdr: &SectHdr) -> u64 {
    let declared = 1u64 << u32::from(hdr.align.min(14));
    let code = hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0;
    declared.max(if code { 4 } else { 16 })
}

/// Merge adjacent live atoms into placement runs. Each run lands at
/// the smallest offset ≥ the running cursor congruent to its old
/// offset mod `a` ([`run_modulus`]), preserving every atom's address
/// class. Offsets are section-relative; returns the runs plus the new
/// payload size.
pub(crate) fn build_runs(atoms_rel: &[(u64, u64)], live: &[bool], a: u64) -> (Vec<Run>, u64) {
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
pub(crate) fn addend_group_len(entries: &[[u8; 8]], i: usize) -> usize {
    let info = u32::from_le_bytes(entries[i][4..8].try_into().unwrap());
    let r_type = (info >> 28) & 0xF;
    if r_type == 10 && i + 1 < entries.len() {
        2
    } else {
        1
    }
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
                        flags: u32::from_le_bytes(bytes[p + 64..p + 68].try_into().unwrap()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;
    use torajs_obj::{N_SECT, N_TYPE, NLIST_64_SIZE};

    fn leaf(name: &str, words: usize) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: [0xD5, 0x03, 0x20, 0x1F]
                .repeat(words - 1)
                .into_iter()
                .chain([0xC0, 0x03, 0x5F, 0xD6])
                .collect(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// Section 1 of `bytes`: its payload and every row defined in it,
    /// as `(name, section-relative value)`.
    fn text_of(bytes: &[u8]) -> (Vec<u8>, Vec<(String, u64)>, u64) {
        let (_, symtab_pos, _, sects) = scan_load_commands(bytes).unwrap();
        let s = sects.iter().find(|s| s.ord == 1).unwrap();
        let sp = symtab_pos.unwrap();
        let rd = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
        let (symoff, nsyms, stroff) = (rd(sp + 8), rd(sp + 12), rd(sp + 16));
        let mut rows: Vec<(String, u64)> = Vec::new();
        for i in 0..nsyms {
            let p = symoff + i * NLIST_64_SIZE as usize;
            if bytes[p + 4] & N_TYPE != N_SECT || bytes[p + 5] != 1 {
                continue;
            }
            let name = &bytes[stroff + rd(p)..];
            let name = &name[..name.iter().position(|&b| b == 0).unwrap()];
            let v = u64::from_le_bytes(bytes[p + 8..p + 16].try_into().unwrap());
            rows.push((String::from_utf8(name.to_vec()).unwrap(), v - s.addr));
        }
        let off = s.offset as usize;
        (bytes[off..off + s.size as usize].to_vec(), rows, s.size)
    }

    /// Atoms the way the reach pass cuts them (one per distinct row
    /// value, the section start always one) and strip against the ones
    /// a `keep` name starts.
    fn strip(bytes: &[u8], keep: &[&str], dead_at: DeadRowPlacement) -> Vec<u8> {
        strip_rows(bytes, keep, dead_at).map_or_else(|| bytes.to_vec(), |m| m.bytes)
    }

    fn strip_rows(
        bytes: &[u8],
        keep: &[&str],
        dead_at: DeadRowPlacement,
    ) -> Option<StrippedMember> {
        let (_, rows, size) = text_of(bytes);
        let mut starts: Vec<u64> = rows.iter().map(|r| r.1).chain([0]).collect();
        starts.sort_unstable();
        starts.dedup();
        let atoms: Vec<(u64, u64)> = starts
            .iter()
            .enumerate()
            .map(|(i, &st)| (st, starts.get(i + 1).copied().unwrap_or(size)))
            .collect();
        let live: Vec<bool> = atoms
            .iter()
            .map(|a| {
                rows.iter()
                    .any(|r| r.1 == a.0 && keep.contains(&r.0.as_str()))
            })
            .collect();
        strip_member_section(bytes, 1, &atoms, &live, dead_at).unwrap()
    }

    /// Stripping against a superset of the live atoms first, then
    /// against the subset, lays the subset out exactly as stripping
    /// the original against it: the alignment padding the first pass
    /// leaves ahead of a run must not join the run before it. Padding
    /// only exists where the modulus exceeds the atom granule, so the
    /// section is declared 16-aligned.
    #[test]
    fn stripping_twice_places_runs_as_stripping_once() {
        let obj = packed_obj(4);
        let subset = ["_f0", "_f6"];
        let base = DeadRowPlacement::SectionBase;
        let once = strip(&obj, &subset, base);
        let pre = strip(&obj, &["_f0", "_f2", "_f6"], DeadRowPlacement::GapStart);
        let twice = strip(&pre, &subset, base);
        let (text_once, rows_once, _) = text_of(&once);
        let (text_twice, rows_twice, _) = text_of(&twice);
        let at = |rows: &[(String, u64)], n: &str| rows.iter().find(|r| r.0 == n).unwrap().1;
        assert_eq!(at(&rows_once, "_f6"), 16);
        // with the first pass's dead rows at the section base, _f0
        // would stretch over the 8 padding bytes ahead of _f2 and put
        // _f6 at 32
        assert_eq!(at(&rows_twice, "_f6"), 16);
        assert_eq!(at(&rows_twice, "_f0"), 0);
        assert_eq!(text_twice, text_once);
    }

    /// A pre-stripped section whose one dead atom sits where the next
    /// run's alignment padding goes buys no bytes from the second
    /// strip, yet stripping the original leaves zeros there: the second
    /// strip must still rewrite it. 16-aligned for the same reason as
    /// above.
    #[test]
    fn a_prestripped_dead_atom_in_padding_is_zeroed() {
        let obj = packed_obj(4);
        let subset = ["_f0", "_f6"];
        let once = strip(&obj, &subset, DeadRowPlacement::SectionBase);
        let pre = strip_rows(&obj, &["_f0", "_f5", "_f6"], DeadRowPlacement::GapStart).unwrap();
        let mut pre_bytes = pre.bytes;
        crate::runtime_prestrip::anonymize_rows(&mut pre_bytes, &pre.dead_rows).unwrap();
        // f5 sits at 12..16 and f6 at 16 in the pre-stripped section,
        // where f6 lands with f5 gone too
        let (text_pre, _, _) = text_of(&pre_bytes);
        let twice = strip(&pre_bytes, &subset, DeadRowPlacement::SectionBase);
        let (text_once, _, _) = text_of(&once);
        let (text_twice, _, _) = text_of(&twice);
        assert_eq!(text_pre.len(), text_once.len());
        assert_ne!(text_pre, text_once);
        assert_eq!(text_twice, text_once);
    }

    /// In a 4-aligned instruction section a run lands right at the
    /// cursor, so the live bytes after a dead atom come out the same
    /// whatever its length: a change confined to dead code cannot move
    /// live code. Under a modulus of 16 the dead 4 vs 8 bytes put
    /// `_f2` at 16 vs 20.
    #[test]
    fn dead_atom_length_does_not_move_later_runs() {
        let stripped = |dead_words: usize| {
            let obj = torajs_obj::write_object(&[
                leaf("_f0", 3),
                leaf("_dead", dead_words),
                leaf("_f2", 5),
            ]);
            strip(&obj, &["_f0", "_f2"], DeadRowPlacement::SectionBase)
        };
        let (text_a, rows_a, _) = text_of(&stripped(1));
        let (text_b, rows_b, _) = text_of(&stripped(2));
        let at = |rows: &[(String, u64)], n: &str| rows.iter().find(|r| r.0 == n).unwrap().1;
        assert_eq!(at(&rows_a, "_f2"), 12);
        assert_eq!(at(&rows_b, "_f2"), 12);
        assert_eq!(text_a, text_b);
        assert_eq!(text_a.len(), 32);
    }

    /// Seven leaf fns whose rows are re-pointed so the atoms pack at 4
    /// bytes: f0 0..12, f1 ..36, f2 ..44, f3 ..52, f4 ..60, f5 ..64,
    /// f6 64.., in a `__text` declared `1 << align`-aligned.
    fn packed_obj(align: u32) -> Vec<u8> {
        let mut obj = torajs_obj::write_object(&[
            leaf("_f0", 32),
            leaf("_f1", 1),
            leaf("_f2", 1),
            leaf("_f3", 1),
            leaf("_f4", 1),
            leaf("_f5", 1),
            leaf("_f6", 1),
        ]);
        let (_, symtab_pos, _, sects) = scan_load_commands(&obj).unwrap();
        let text = sects.iter().find(|s| s.ord == 1).unwrap();
        let base = text.addr;
        obj[text.hdr_pos + 52..text.hdr_pos + 56].copy_from_slice(&align.to_le_bytes());
        let sp = symtab_pos.unwrap();
        let rd = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
        let (symoff, nsyms) = (rd(&obj, sp + 8), rd(&obj, sp + 12));
        let (_, rows, _) = text_of(&obj);
        let offs = [0u64, 12, 36, 44, 52, 60, 64];
        for i in 0..nsyms {
            let p = symoff + i * NLIST_64_SIZE as usize;
            let v = u64::from_le_bytes(obj[p + 8..p + 16].try_into().unwrap()) - base;
            if let Some(k) = rows.iter().position(|r| r.1 == v) {
                let k = rows[k].0[2..].parse::<usize>().unwrap();
                obj[p + 8..p + 16].copy_from_slice(&(base + offs[k]).to_le_bytes());
            }
        }
        obj
    }
}
