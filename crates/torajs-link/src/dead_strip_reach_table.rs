//! A member's reachability tables as one flat byte table, and the
//! unpacking into [`MemberReach`]. The runtime image stores the table
//! of every member it holds, built from the member's bytes when `tr`
//! was compiled; a member the image holds no table for (rewritten
//! bytes, an image built without tables) gets one built from its
//! bytes. Everything a parse would search for is in the table already
//! (name lengths, the by-name lists, the no-dead-strip roots, each
//! section's atom starts), so unpacking it is one pass over its rows
//! with no NUL scan and no sort.
//!
//! Layout (little-endian u32 unless noted; nlist rows and names are
//! read from the member's own bytes):
//!
//! ```text
//!   header    symoff stroff n_nlist n_sects n_defined n_dup n_nds
//!   sects     n_sects × { addr(u64) size(u64) file_off flags
//!                         atom_off atom_count sectname[16] is_text(u8) }
//!   name_lens n_nlist × length of the row's name (0 = "")
//!   defined   n_defined × nlist row, sorted by name, the last row of
//!             each name
//!   dup       n_dup × nlist row, sorted by name, one per name more
//!             than one N_SECT row carries
//!   nds       n_nds × nlist row, in nlist order, of each N_SECT row
//!             flagged N_NO_DEAD_STRIP
//!   atoms     Σ atom_count × atom start − section addr
//! ```

use std::rc::Rc;

use torajs_obj::NLIST_64_SIZE;

use crate::archive::ArMember;
use crate::archives_merge::MergedArchives;
use crate::dead_strip_reach::{MemberReach, NlEntry, SectAtoms};
use crate::member_sections::is_no_file_storage;

pub(crate) const HEADER_LEN: usize = 7 * 4;
pub(crate) const SECT_LEN: usize = 8 + 8 + 4 * 4 + 16 + 1;

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

impl<'a> MemberReach<'a> {
    /// `(n_sect, n_value)` of the last `N_SECT` row named `name`.
    pub(crate) fn defined(&self, name: &str) -> Option<(u8, u64)> {
        let d = &self.defined;
        d.binary_search_by(|e| e.0.cmp(name))
            .ok()
            .map(|k| (d[k].1, d[k].2))
    }

    /// Whether more than one `N_SECT` row is named `name`.
    pub(crate) fn is_dup_defined(&self, name: &str) -> bool {
        self.dup_defined.binary_search(&name).is_ok()
    }

    /// The `N_SECT` rows flagged `N_NO_DEAD_STRIP`, in nlist order.
    pub(crate) fn no_dead_strip_rows(&self) -> impl Iterator<Item = &NlEntry<'a>> + '_ {
        self.no_dead_strip.iter().map(|&i| &self.nlist[i as usize])
    }
}

/// The image's table of member `m` of archive `a`; empty when the
/// image holds none or the link swapped the member's bytes out.
pub(crate) fn image_table<'a>(merged: &MergedArchives<'a>, (a, m): (usize, usize)) -> &'a [u8] {
    if std::ptr::eq(merged.member(a, m).data, merged.image.member(a, m).data) {
        merged.image.member_table(a, m)
    } else {
        &[]
    }
}

/// The member's reachability tables, every atom dead, unpacked from
/// `tab` (the member's table) and the member's own bytes.
pub(crate) fn reach_from_table<'a>(member: &ArMember<'a>, tab: &[u8]) -> MemberReach<'a> {
    let h = |i: usize| u32_at(tab, i * 4) as usize;
    let (symoff, stroff, n_nlist, n_sects) = (h(0), h(1), h(2), h(3));
    let (n_defined, n_dup, n_nds) = (h(4), h(5), h(6));
    let lens_off = HEADER_LEN + n_sects * SECT_LEN;
    let defined_off = lens_off + n_nlist * 4;
    let dup_off = defined_off + n_defined * 4;
    let nds_off = dup_off + n_dup * 4;
    let atoms_off = nds_off + n_nds * 4;
    let sects = (0..n_sects)
        .map(|i| {
            let s = HEADER_LEN + i * SECT_LEN;
            let addr = u64_at(tab, s);
            let size = u64_at(tab, s + 8);
            let flags = u32_at(tab, s + 20);
            let atom_count = u32_at(tab, s + 28) as usize;
            let first = atoms_off + u32_at(tab, s + 24) as usize * 4;
            let start = |k: usize| addr + u64::from(u32_at(tab, first + k * 4));
            let atoms: Rc<[(u64, u64)]> = (0..atom_count)
                .map(|k| {
                    let end = if k + 1 < atom_count {
                        start(k + 1)
                    } else {
                        addr + size
                    };
                    (start(k), end)
                })
                .collect();
            SectAtoms {
                addr,
                size,
                file_off: u32_at(tab, s + 16),
                no_file_storage: is_no_file_storage(flags),
                flags,
                sectname: tab[s + 32..s + 48].try_into().unwrap(),
                is_text: tab[s + 48] != 0,
                atoms,
                live: vec![false; atom_count],
                all_live: false,
            }
        })
        .collect();
    let b = member.data;
    let nlist: Rc<[NlEntry<'a>]> = (0..n_nlist)
        .map(|i| {
            let off = symoff + i * NLIST_64_SIZE as usize;
            let row: &[u8; 16] = b[off..off + 16].try_into().unwrap();
            // a length of 0 is a row without a name or with one that
            // is not UTF-8; any other length is a checked UTF-8 name
            let len = u32_at(tab, lens_off + i * 4) as usize;
            let name = if len == 0 {
                ""
            } else {
                let strx = u32::from_le_bytes(row[0..4].try_into().unwrap()) as usize;
                std::str::from_utf8(&b[stroff + strx..][..len]).unwrap_or("")
            };
            NlEntry {
                name,
                n_type: row[4],
                n_sect: row[5],
                n_value: u64::from_le_bytes(row[8..16].try_into().unwrap()),
            }
        })
        .collect();
    let row = |off: usize, k: usize| &nlist[u32_at(tab, off + k * 4) as usize];
    let defined = (0..n_defined)
        .map(|k| {
            let e = row(defined_off, k);
            (e.name, e.n_sect, e.n_value)
        })
        .collect();
    let dup_defined = (0..n_dup).map(|k| row(dup_off, k).name).collect();
    let no_dead_strip = (0..n_nds).map(|k| u32_at(tab, nds_off + k * 4)).collect();
    MemberReach {
        member_name: member.name,
        sects,
        nlist,
        defined,
        dup_defined,
        no_dead_strip,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::dead_strip_reach_build::build_member_reach;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;
    use torajs_obj::{MH_SUBSECTIONS_VIA_SYMBOLS, MachHeader64, SEGMENT_COMMAND_64_SIZE};

    /// An object splitting at its symbols with one `__TEXT,__text`
    /// section of `size` bytes at `addr` and the defined rows `rows`
    /// (name, address), in nlist order.
    pub(crate) fn text_object(addr: u64, size: u64, rows: &[(&str, u64)]) -> Vec<u8> {
        let funcs: Vec<CompiledFunction> = rows
            .iter()
            .enumerate()
            .map(|(i, (name, _))| CompiledFunction {
                name: (*name).into(),
                bytes: if i == 0 {
                    vec![0; size as usize]
                } else {
                    Vec::new()
                },
                relocs: Vec::new(),
                frame: FrameLayout::leaf_no_spill(),
            })
            .collect();
        let mut obj = torajs_obj::write_object(&funcs);
        let mut header = MachHeader64::parse(&obj).unwrap();
        header.flags |= MH_SUBSECTIONS_VIA_SYMBOLS;
        header.write_over(&mut obj);
        let sect = MachHeader64::SIZE + SEGMENT_COMMAND_64_SIZE as usize;
        obj[sect + 32..sect + 40].copy_from_slice(&addr.to_le_bytes());
        let symoff = u32::from_le_bytes(obj[sect + 80 + 8..sect + 80 + 12].try_into().unwrap());
        for (i, (_, v)) in rows.iter().enumerate() {
            let p = symoff as usize + i * NLIST_64_SIZE as usize + 8;
            obj[p..p + 8].copy_from_slice(&v.to_le_bytes());
        }
        obj
    }

    /// [`build_member_reach`] of [`text_object`], the bytes leaked.
    pub(crate) fn text_reach(addr: u64, size: u64, rows: &[(&str, u64)]) -> MemberReach<'static> {
        let data: &'static [u8] = Vec::leak(text_object(addr, size, rows));
        build_member_reach(&ArMember { name: "t.o", data }).unwrap()
    }

    /// A member without symbols whose sections are `(addr, size, atom
    /// starts)`.
    pub(crate) fn sections_reach(sects: &[(u64, u64, &[u64])]) -> MemberReach<'static> {
        let mut t: Vec<u8> = Vec::new();
        for v in [0, 0, 0, sects.len() as u32, 0, 0, 0] {
            t.extend_from_slice(&v.to_le_bytes());
        }
        let mut atoms: Vec<u32> = Vec::new();
        for &(addr, size, starts) in sects {
            t.extend_from_slice(&addr.to_le_bytes());
            t.extend_from_slice(&size.to_le_bytes());
            for v in [0, 0, atoms.len() as u32, starts.len() as u32] {
                t.extend_from_slice(&v.to_le_bytes());
            }
            t.extend_from_slice(b"__text\0\0\0\0\0\0\0\0\0\0");
            t.push(1);
            atoms.extend(starts.iter().map(|&s| (s - addr) as u32));
        }
        for v in atoms {
            t.extend_from_slice(&v.to_le_bytes());
        }
        let member = ArMember {
            name: "m.o",
            data: &[],
        };
        reach_from_table(&member, &t)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::text_reach;

    #[test]
    fn the_table_answers_what_the_object_holds() {
        let r = text_reach(
            0x4000,
            0x1000,
            &[("_b", 0x4800), ("_a", 0x4c00), ("_b", 0x4900), ("", 0x4800)],
        );
        let text = &r.sects[0];
        assert!(text.is_text);
        let atoms: Vec<(u64, u64)> = text.atoms.to_vec();
        assert_eq!(
            atoms,
            [
                (0x4000, 0x4800),
                (0x4800, 0x4900),
                (0x4900, 0x4c00),
                (0x4c00, 0x5000)
            ]
        );
        let names: Vec<&str> = r.nlist.iter().map(|e| e.name).collect();
        assert_eq!(names, ["_b", "_a", "_b", ""]);
        assert_eq!(r.nlist.iter().count(), 4);
        assert!(r.nlist.get(4).is_none());
        // the last row of a name answers
        assert_eq!(r.defined("_b"), Some((1, 0x4900)));
        assert_eq!(r.defined("_a"), Some((1, 0x4c00)));
        assert_eq!(r.defined(""), Some((1, 0x4800)));
        assert_eq!(r.defined("_c"), None);
        assert!(r.is_dup_defined("_b"));
        assert!(!r.is_dup_defined("_a"));
        assert!(!r.is_dup_defined(""));
        assert_eq!(r.no_dead_strip_rows().count(), 0);
    }

    #[test]
    fn rows_flagged_no_dead_strip_are_listed_in_nlist_order() {
        use super::test_support::text_object;
        use crate::archive::ArMember;
        use crate::dead_strip_reach_build::build_member_reach;
        use torajs_obj::{MachHeader64, N_NO_DEAD_STRIP, NLIST_64_SIZE, SEGMENT_COMMAND_64_SIZE};
        let mut obj = text_object(0, 0x40, &[("_a", 0), ("_b", 0x10), ("_c", 0x20)]);
        let sect = MachHeader64::SIZE + SEGMENT_COMMAND_64_SIZE as usize;
        let symoff = u32::from_le_bytes(obj[sect + 80 + 8..sect + 80 + 12].try_into().unwrap());
        for i in [2, 0] {
            let p = symoff as usize + i * NLIST_64_SIZE as usize + 6;
            obj[p..p + 2].copy_from_slice(&N_NO_DEAD_STRIP.to_le_bytes());
        }
        let data: &'static [u8] = Vec::leak(obj);
        let r = build_member_reach(&ArMember { name: "t.o", data }).unwrap();
        let rows: Vec<(&str, u64)> = r
            .no_dead_strip_rows()
            .map(|e| (e.name, e.n_value))
            .collect();
        assert_eq!(rows, [("_a", 0), ("_c", 0x20)]);
    }

    /// Every member of the embedded runtime's pre-stripped image: the
    /// table the image stores reads back as what the member's bytes
    /// build.
    #[test]
    fn the_image_tables_match_the_member_bytes() {
        use super::{MemberReach, image_table, reach_from_table};
        use crate::archives_merge::merge_archive_indexes;
        use crate::dead_strip_reach_build::build_member_reach;
        use crate::runtime_prestrip::prestrip_image;
        type Sect = (u64, u64, u32, u32, [u8; 16], bool, Vec<(u64, u64)>);
        let sects = |r: &MemberReach<'_>| -> Vec<Sect> {
            r.sects
                .iter()
                .map(|s| {
                    let atoms = s.atoms.to_vec();
                    (
                        s.addr, s.size, s.file_off, s.flags, s.sectname, s.is_text, atoms,
                    )
                })
                .collect()
        };
        let image = prestrip_image(torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE).unwrap();
        let merged = merge_archive_indexes(&image).unwrap();
        let mut checked = 0;
        for a in 0..merged.archive_count() {
            for m in 0..merged.member_count(a) {
                let member = merged.member(a, m);
                let table = image_table(&merged, (a, m));
                assert!(!table.is_empty(), "{}: no table", member.name);
                let (got, want) = (
                    reach_from_table(&member, table),
                    build_member_reach(&member).unwrap(),
                );
                assert_eq!(sects(&got), sects(&want), "{}: sects", member.name);
                assert_eq!(got.defined, want.defined, "{}: defined", member.name);
                assert_eq!(got.dup_defined, want.dup_defined, "{}", member.name);
                assert_eq!(got.no_dead_strip, want.no_dead_strip, "{}", member.name);
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked} members");
    }
}
