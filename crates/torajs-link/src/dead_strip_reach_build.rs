//! S2 dead-strip reach — member parsing half (extracted from
//! `dead_strip_reach` when the why-live tracing pushed the closure
//! file at the 500-line hard cap). Answers "what are this member's
//! atoms and symbols"; the closure file answers "which of them are
//! reachable".

use std::collections::BTreeMap;

use torajs_obj::{MachHeader64, N_SECT, N_TYPE, NLIST_64_SIZE};

use crate::archive::ArMember;
use crate::dead_strip_reach::{MemberReach, NlEntry, SectAtoms};
use crate::member_sections::{collect_member_sections, is_no_file_storage};
use crate::member_symtab::parse_member_symtab;

/// Parse one member into its reachability tables.
pub(crate) fn build_member_reach<'a>(member: &ArMember<'a>) -> Result<MemberReach<'a>, String> {
    let sections =
        collect_member_sections(member).map_err(|e| format!("{}: sections: {e:?}", member.name))?;

    let bytes = member.data;
    let mut nlist: Vec<NlEntry<'a>> = Vec::new();
    let mut defined: BTreeMap<&'a str, (u8, u64)> = BTreeMap::new();
    let mut dup_defined: Vec<&'a str> = Vec::new();
    if bytes.len() >= 32 && !sections.is_empty() {
        let st = parse_member_symtab(member).map_err(|e| format!("{}: {e:?}", member.name))?;
        let symoff = st.symoff as usize;
        let stroff = st.stroff as usize;
        for i in 0..st.nsyms as usize {
            let off = symoff + i * NLIST_64_SIZE as usize;
            if bytes.len() < off + NLIST_64_SIZE as usize {
                return Err(format!("{}: nlist row {i} truncated", member.name));
            }
            let n_strx = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
            let n_type = bytes[off + 4];
            let n_sect = bytes[off + 5];
            let n_desc = u16::from_le_bytes(bytes[off + 6..off + 8].try_into().unwrap());
            let n_value = u64::from_le_bytes(bytes[off + 8..off + 16].try_into().unwrap());
            let name = if n_strx == 0 {
                ""
            } else {
                let s = stroff + n_strx;
                let end = bytes[s..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| s + p)
                    .unwrap_or(bytes.len());
                std::str::from_utf8(&bytes[s..end]).unwrap_or("")
            };
            if n_type & N_TYPE == N_SECT && defined.insert(name, (n_sect, n_value)).is_some() {
                dup_defined.push(name);
            }
            nlist.push(NlEntry {
                name,
                n_type,
                n_sect,
                n_desc,
                n_value,
            });
        }
    }

    // Per-section atom tables. Only a member that declares
    // `MH_SUBSECTIONS_VIA_SYMBOLS` splits at its symbols; in any other
    // (hand-written assembly: compiler-builtins' outlined atomics) code
    // may fall through from one symbol into the next, so each section
    // is one atom.
    let subsections = splits_at_symbols(bytes);
    // each section's symbol addresses, in one pass over the nlist
    let mut starts_by_ord: Vec<Vec<u64>> = vec![Vec::new(); usize::from(u8::MAX) + 1];
    if subsections {
        for e in nlist.iter().filter(|e| e.n_type & N_TYPE == N_SECT) {
            starts_by_ord[usize::from(e.n_sect)].push(e.n_value);
        }
    }
    let mut sects: Vec<SectAtoms> = Vec::with_capacity(sections.len());
    for s in &sections {
        let mut starts = std::mem::take(&mut starts_by_ord[usize::from(s.section_index)]);
        starts.sort_unstable();
        starts.dedup();
        // Leading unnamed gap becomes atom 0.
        if starts.first().is_none_or(|&f| f > s.member_addr) {
            starts.insert(0, s.member_addr);
        }
        let sect_end = s.member_addr + u64::from(s.size);
        let mut atoms: Vec<(u64, u64)> = Vec::with_capacity(starts.len());
        for (i, &st) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(sect_end);
            atoms.push((st, end));
        }
        sects.push(SectAtoms {
            addr: s.member_addr,
            size: u64::from(s.size),
            file_off: s.member_internal_offset,
            no_file_storage: is_no_file_storage(s.flags),
            flags: s.flags,
            sectname: s.sectname,
            is_text: trim16(&s.sectname) == b"__text" && trim16(&s.segname) == b"__TEXT",
            live: vec![false; atoms.len()],
            atoms: atoms.into(),
            all_live: false,
        });
    }

    dup_defined.sort_unstable();
    dup_defined.dedup();
    Ok(MemberReach {
        member_name: member.name,
        sects,
        nlist: nlist.into(),
        defined: defined.into(),
        dup_defined: dup_defined.into(),
    })
}

/// Whether the object `bytes` declares `MH_SUBSECTIONS_VIA_SYMBOLS`,
/// so its sections may be cut at any symbol.
pub(crate) fn splits_at_symbols(bytes: &[u8]) -> bool {
    MachHeader64::parse(bytes).is_some_and(|h| h.splits_at_symbols())
}

/// Trim trailing NUL/space padding off a raw 16-byte name slot.
pub(crate) fn trim16(raw: &[u8; 16]) -> &[u8] {
    let end = raw
        .iter()
        .position(|&b| b == 0 || b == b' ')
        .unwrap_or(raw.len());
    &raw[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;

    fn leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: vec![0xC0, 0x03, 0x5F, 0xD6],
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// `__text` atoms, section-relative.
    fn text_atoms(obj: &[u8]) -> Vec<(u64, u64)> {
        let member = ArMember {
            name: "t.o",
            data: obj,
        };
        let reach = build_member_reach(&member).unwrap();
        let text = reach.sects.iter().find(|s| s.is_text).unwrap();
        text.atoms
            .iter()
            .map(|&(s, e)| (s - text.addr, e - text.addr))
            .collect()
    }

    #[test]
    fn only_a_subsections_object_splits_at_its_symbols() {
        let mut obj = torajs_obj::write_object(&[leaf("_a"), leaf("_b"), leaf("_c")]);
        assert!(!splits_at_symbols(&obj));
        assert_eq!(text_atoms(&obj), [(0, 12)]);

        let mut header = MachHeader64::parse(&obj).unwrap();
        header.flags |= torajs_obj::MH_SUBSECTIONS_VIA_SYMBOLS;
        header.write_over(&mut obj);
        assert!(splits_at_symbols(&obj));
        assert_eq!(text_atoms(&obj), [(0, 4), (4, 8), (8, 12)]);
    }
}
