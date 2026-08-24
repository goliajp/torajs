//! S2 dead-strip reach — member parsing half (extracted from
//! `dead_strip_reach` when the why-live tracing pushed the closure
//! file at the 500-line hard cap). Answers "what are this member's
//! atoms and symbols"; the closure file answers "which of them are
//! reachable".

use std::collections::BTreeMap;

use torajs_obj::{N_SECT, N_TYPE, NLIST_64_SIZE};

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
            if n_type & N_TYPE == N_SECT {
                defined.insert(name, (n_sect, n_value));
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

    // Per-section atom tables.
    let mut sects: Vec<SectAtoms> = Vec::with_capacity(sections.len());
    for s in &sections {
        let ord = s.section_index;
        let mut starts: Vec<u64> = nlist
            .iter()
            .filter(|e| e.n_type & N_TYPE == N_SECT && e.n_sect == ord)
            .map(|e| e.n_value)
            .collect();
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
            atoms,
            all_live: false,
        });
    }

    Ok(MemberReach {
        member_name: member.name,
        sects,
        nlist,
        defined,
    })
}

/// Trim trailing NUL/space padding off a raw 16-byte name slot.
pub(crate) fn trim16(raw: &[u8; 16]) -> &[u8] {
    let end = raw
        .iter()
        .position(|&b| b == 0 || b == b' ')
        .unwrap_or(raw.len());
    &raw[..end]
}
