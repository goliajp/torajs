//! One member as the dead-strip pass cuts it, kept as tables over the
//! member's own bytes: the passes (where every kept run of each cut
//! section lands), each cut section's surviving relocs, the moved
//! section-keyed address addends, how every `N_SECT` row moves, and
//! the undef rows nothing names any more. The link reads the member
//! through these ([`crate::member_obj::MemberObj`]); the runtime
//! pre-strip, which ships the result as an object, writes it out
//! ([`Cut::write`]) as the rewrite one section at a time would.

use torajs_obj::{N_EXT, N_SECT, N_TYPE, NLIST_64_SIZE};

use crate::dead_strip_rewrite::{DeadRowPlacement, StrippedMember, scan_load_commands};
use crate::dead_strip_rewrite_sections::{
    Pass, SectionCut, copy_spans, patch_headers, plan, put32, put64, rd32, shift_off,
};
use crate::dead_strip_rewrite_tables::{
    RowPlan, filter_relocs, move_value, row_plan, unreferenced_undefs, walk_relocs,
};

#[derive(Debug)]
pub(crate) struct Cut {
    /// The cut sections in section order, each planned against the
    /// headers the ones before it leave.
    pub(crate) passes: Vec<Pass>,
    /// How each section's rows move, by ordinal − 1.
    plans: Vec<Option<RowPlan>>,
    /// The surviving relocs of each cut section that has a table.
    pub(crate) kept: Vec<(u8, Vec<[u8; 8]>)>,
    /// `(ordinal, offset in the section as cut, value)` of every moved
    /// address addend, in section order.
    pub(crate) patches: Vec<(u8, u32, u64)>,
    /// One bit per row: an undef row that loses `N_EXT`.
    neutered: Vec<u64>,
    dead_at: DeadRowPlacement,
}

impl Cut {
    /// Plan cutting `cuts` (ascending section order) out of `bytes`; a
    /// section whose cut buys no bytes is left alone unless the runtime
    /// pre-strip left dead content in it. With `neuter`, the undef rows
    /// no surviving reloc names lose `N_EXT`. `None` when that changes
    /// nothing.
    pub(crate) fn plan(
        bytes: &[u8],
        cuts: &[SectionCut<'_>],
        dead_at: DeadRowPlacement,
        neuter: bool,
    ) -> Result<Option<Cut>, String> {
        let (_, symtab_pos, _, sects) = scan_load_commands(bytes)?;
        let passes = plan(bytes, symtab_pos, &sects, cuts)?;
        let kept: Vec<(u8, Vec<[u8; 8]>)> = passes
            .iter()
            .filter_map(|p| {
                let hdr = &sects[usize::from(p.ord) - 1];
                (hdr.reloff != 0 && hdr.nreloc != 0).then(|| {
                    let table = &bytes[hdr.reloff as usize..][..hdr.nreloc as usize * 8];
                    (p.ord, filter_relocs(table, p))
                })
            })
            .collect();
        let symtab = symtab_pos
            .filter(|_| neuter)
            .map(|sp| (rd32(bytes, sp + 8) as usize, rd32(bytes, sp + 12) as usize));
        let nsyms = symtab.map_or(0, |t| t.1);
        let (patches, referenced) = walk_relocs(bytes, &sects, &passes, &kept, nsyms)?;
        let (neutered, n) = match symtab {
            Some(t) => unreferenced_undefs(bytes, t, &referenced),
            None => (Vec::new(), 0),
        };
        if passes.is_empty() && n == 0 {
            return Ok(None);
        }
        let plans = sects.iter().map(|s| row_plan(s, &passes)).collect();
        Ok(Some(Cut {
            passes,
            plans,
            kept,
            patches,
            neutered,
            dead_at,
        }))
    }

    /// The pass that cuts section `ord`.
    pub(crate) fn pass(&self, ord: u8) -> Option<&Pass> {
        self.passes.iter().find(|p| p.ord == ord)
    }

    /// The surviving relocs of section `ord` when it is cut.
    pub(crate) fn kept(&self, ord: u8) -> Option<&[[u8; 8]]> {
        let (_, k) = self.kept.iter().find(|(o, _)| *o == ord)?;
        Some(k)
    }

    /// The moved address addends of section `ord`.
    pub(crate) fn patches_of(&self, ord: u8) -> &[(u8, u32, u64)] {
        let lo = self.patches.partition_point(|p| p.0 < ord);
        let hi = self.patches.partition_point(|p| p.0 <= ord);
        &self.patches[lo..hi]
    }

    /// Whether undef row `row` loses `N_EXT`.
    pub(crate) fn is_neutered(&self, row: u32) -> bool {
        let r = row as usize;
        self.neutered
            .get(r / 64)
            .is_some_and(|w| w >> (r % 64) & 1 == 1)
    }

    /// Where an `N_SECT` row of section `n_sect` at `v` lands, and
    /// whether its atom was dropped.
    pub(crate) fn move_row(&self, n_sect: u8, v: u64) -> (u64, bool) {
        let (v, dead) = move_value(&self.passes, &self.plans, n_sect, v, self.dead_at);
        (v, dead.is_some())
    }

    /// The rows of `bytes` whose atom was dropped, ascending.
    pub(crate) fn dead_rows(&self, bytes: &[u8]) -> Result<Vec<u32>, String> {
        let (_, symtab_pos, _, _) = scan_load_commands(bytes)?;
        let Some(sp) = symtab_pos else {
            return Ok(Vec::new());
        };
        let (symoff, nsyms) = (rd32(bytes, sp + 8) as usize, rd32(bytes, sp + 12));
        Ok((0..nsyms)
            .filter(|&i| {
                let p = symoff + i as usize * NLIST_64_SIZE as usize;
                let v = u64::from_le_bytes(bytes[p + 8..p + 16].try_into().unwrap());
                bytes[p + 4] & N_TYPE == N_SECT && self.move_row(bytes[p + 5], v).1
            })
            .collect())
    }

    /// The member written out as an object: the bytes, and the rows
    /// whose atom was dropped, pass by pass, ascending within a pass.
    pub(crate) fn write(&self, bytes: &[u8]) -> Result<StrippedMember, String> {
        let (seg_positions, symtab_pos, dysymtab_pos, sects) = scan_load_commands(bytes)?;
        let passes = &self.passes;
        let mut out = copy_spans(bytes, passes);
        let symtab = patch_headers(
            &mut out,
            passes,
            &sects,
            &seg_positions,
            symtab_pos,
            dysymtab_pos,
        );
        for (ord, kept) in &self.kept {
            let hdr = &sects[usize::from(*ord) - 1];
            let tbl = shift_off(passes, hdr.reloff) as usize;
            for (k, e) in kept.iter().enumerate() {
                out[tbl + k * 8..][..8].copy_from_slice(e);
            }
            out[tbl + kept.len() * 8..tbl + hdr.nreloc as usize * 8].fill(0);
            put32(&mut out, hdr.hdr_pos + 60, kept.len() as u32);
        }
        for &(ord, site, v) in &self.patches {
            let hdr = &sects[usize::from(ord) - 1];
            put64(
                &mut out,
                shift_off(passes, hdr.offset) as usize + site as usize,
                v,
            );
        }
        let dead_rows = match symtab {
            Some(symtab) => self.write_rows(&mut out, symtab),
            None => Vec::new(),
        };
        Ok(StrippedMember {
            bytes: out,
            dead_rows,
        })
    }

    /// Move every `N_SECT` row of the table `(symoff, nsyms)` of `out`
    /// and clear `N_EXT` on the neutered ones. Returns the rows whose
    /// atom was dropped, pass by pass, ascending within a pass.
    fn write_rows(&self, out: &mut [u8], (symoff, nsyms): (usize, usize)) -> Vec<u32> {
        let mut dead: Vec<Vec<u32>> = vec![Vec::new(); self.passes.len()];
        for i in 0..nsyms {
            let p = symoff + i * NLIST_64_SIZE as usize;
            if out[p + 4] & N_TYPE == N_SECT {
                let v = u64::from_le_bytes(out[p + 8..p + 16].try_into().unwrap());
                let (v, k) = move_value(&self.passes, &self.plans, out[p + 5], v, self.dead_at);
                out[p + 8..p + 16].copy_from_slice(&v.to_le_bytes());
                if let Some(k) = k {
                    dead[k].push(i as u32);
                }
            }
            if self.is_neutered(i as u32) {
                out[p + 4] &= !N_EXT;
            }
        }
        dead.concat()
    }
}
