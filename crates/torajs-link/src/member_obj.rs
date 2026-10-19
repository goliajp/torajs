//! One runtime member as the link reads it once the dead-strip
//! pre-pass is done: its sections, reloc tables, section payloads and
//! nlist rows. The load commands are walked once per member and link
//! ([`MergedArchives::obj`]); every layout and emit stage reads the
//! result instead of walking the member's bytes again. A member the
//! pass cut reads its own bytes through the cut's tables
//! ([`MergedArchives::install_cut`]) and is never written out.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use torajs_obj::{LC_SEGMENT_64, LC_SYMTAB, MH_MAGIC_64, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE};

use crate::archive::ArMember;
use crate::archives_merge::MergedArchives;
use crate::dead_strip_cut::Cut;
use crate::dead_strip_rewrite_sections::{shift_addr, shift_off};
pub(crate) use crate::member_obj_syms::{Sym, symtab_lc_error, symtab_name_error};
use crate::member_obj_syms::{SymtabFault, checked_symtab};
use crate::member_reloc::{
    MemberRelocEntry, MemberRelocError, decode_reloc_entry, decode_reloc_table,
};
use crate::member_resolve::MemberSymtab;
use crate::member_sections::{MemberSectionInfo, section_info};

/// The parsed members of one link, by `(archive_idx, member_idx)`.
#[derive(Debug, Default)]
pub(crate) struct MemberObjs<'a>(RefCell<BTreeMap<(usize, usize), Rc<MemberObj<'a>>>>);

impl<'a> MergedArchives<'a> {
    /// Member `(a, m)` parsed, once per link.
    pub(crate) fn obj(&self, a: usize, m: usize) -> Result<Rc<MemberObj<'a>>, MemberRelocError> {
        if let Some(o) = self.objs.0.borrow().get(&(a, m)) {
            return Ok(o.clone());
        }
        let o = Rc::new(MemberObj::parse(self.member(a, m))?);
        self.objs.0.borrow_mut().insert((a, m), o.clone());
        Ok(o)
    }

    /// Read member `key` through `cut` for the rest of the link.
    pub(crate) fn install_cut(
        &self,
        key: (usize, usize),
        cut: Cut,
    ) -> Result<(), MemberRelocError> {
        let o = MemberObj::parse(self.member(key.0, key.1))?.with_cut(cut);
        self.objs.0.borrow_mut().insert(key, Rc::new(o));
        Ok(())
    }

    /// Member `key` when the dead-strip pass cut it.
    pub(crate) fn cut_obj(&self, key: (usize, usize)) -> Option<Rc<MemberObj<'a>>> {
        let objs = self.objs.0.borrow();
        objs.get(&key).filter(|o| o.cut.is_some()).cloned()
    }
}

#[derive(Debug)]
pub struct MemberObj<'a> {
    pub(crate) member: ArMember<'a>,
    /// Every section, ordinal `i + 1` at index `i`.
    sections: Vec<MemberSectionInfo>,
    /// Each section's reloc table in the member: `(reloff, nreloc)`.
    reloc_tables: Vec<(u32, u32)>,
    /// Each section's payload offset in the member's bytes.
    in_offs: Vec<u32>,
    pub(crate) symtab: Result<MemberSymtab, SymtabFault>,
    pub(crate) cut: Option<Box<Cut>>,
}

impl<'a> MemberObj<'a> {
    /// Walk `member`'s load commands.
    pub fn parse(member: ArMember<'a>) -> Result<Self, MemberRelocError> {
        let bytes = member.data;
        if bytes.len() < 32 || rd32(bytes, 0) != MH_MAGIC_64 {
            return Err(MemberRelocError::TruncatedHeader);
        }
        let ncmds = rd32(bytes, 16) as usize;
        let mut sections = Vec::new();
        let mut reloc_tables = Vec::new();
        let mut symtab = Err(SymtabFault::Missing);
        let mut cursor = 32usize;
        for _ in 0..ncmds {
            if bytes.len() < cursor + 8 {
                return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
            }
            let (cmd, cmdsize) = (rd32(bytes, cursor), rd32(bytes, cursor + 4) as usize);
            if cmdsize < 8 || bytes.len() < cursor + cmdsize {
                return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
            }
            if cmd == LC_SEGMENT_64 {
                for sec in segment_sections(bytes, cursor, cmdsize)? {
                    let index = u8::try_from(sections.len() + 1)
                        .expect("> 255 sections per Mach-O image not supported");
                    sections.push(section_info(bytes, sec, index));
                    reloc_tables.push((rd32(bytes, sec + 56), rd32(bytes, sec + 60)));
                }
            } else if cmd == LC_SYMTAB {
                if cmdsize < 24 {
                    return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
                }
                symtab = checked_symtab(bytes, cursor);
            }
            cursor += cmdsize;
        }
        Ok(MemberObj {
            member,
            sections,
            in_offs: Vec::new(),
            reloc_tables,
            symtab,
            cut: None,
        }
        .with_in_offs())
    }

    fn with_in_offs(mut self) -> Self {
        self.in_offs = self
            .sections
            .iter()
            .map(|s| s.member_internal_offset)
            .collect();
        self
    }

    /// The member as `cut` leaves it: each section's address, size and
    /// file offset as the cut object would declare them.
    pub(crate) fn with_cut(mut self, cut: Cut) -> Self {
        for s in &mut self.sections {
            s.member_addr = shift_addr(&cut.passes, s.member_addr);
            if let Some(p) = cut.pass(s.section_index) {
                s.size = (p.old_size - p.shift) as u32;
            }
            if s.member_internal_offset != 0 {
                s.member_internal_offset = shift_off(&cut.passes, s.member_internal_offset);
            }
        }
        self.cut = Some(Box::new(cut));
        self
    }

    /// Every section in ordinal order.
    pub fn sections(&self) -> &[MemberSectionInfo] {
        &self.sections
    }

    /// The first `__TEXT,__text` section.
    pub fn text_section(&self) -> Option<&MemberSectionInfo> {
        self.sections
            .iter()
            .find(|s| trim16(&s.sectname) == b"__text" && trim16(&s.segname) == b"__TEXT")
    }

    /// The reloc table of section `ord`; empty when there is none.
    pub fn section_relocs(&self, ord: u8) -> Result<Vec<MemberRelocEntry>, MemberRelocError> {
        if let Some(kept) = self.cut.as_ref().and_then(|c| c.kept(ord)) {
            return Ok(kept.iter().map(decode_reloc_entry).collect());
        }
        match usize::from(ord)
            .checked_sub(1)
            .and_then(|i| self.reloc_tables.get(i))
        {
            Some(&(reloff, nreloc)) => decode_reloc_table(self.member.data, reloff, nreloc),
            None => Ok(Vec::new()),
        }
    }

    /// The reloc table of the `__TEXT,__text` section.
    pub fn text_relocs(&self) -> Result<Vec<MemberRelocEntry>, MemberRelocError> {
        let text = self.text_section().ok_or(MemberRelocError::NoTextSection)?;
        self.section_relocs(text.section_index)
    }

    /// The payload of section `ord`.
    pub fn payload(&self, ord: u8) -> Vec<u8> {
        let mut out = Vec::new();
        self.payload_into(ord, &mut out);
        out
    }

    /// Append the payload of section `ord` to `buf`.
    pub fn payload_into(&self, ord: u8, buf: &mut Vec<u8>) {
        let k = usize::from(ord) - 1;
        let (data, size, start) = (self.member.data, self.sections[k].size as usize, buf.len());
        match self.cut.as_ref().and_then(|c| c.pass(ord)) {
            Some(p) => {
                buf.resize(start + size, 0);
                for r in &p.runs {
                    let (src, len) = (p.in_off + r.old_start as usize, r.len as usize);
                    let at = start + r.new_start as usize;
                    buf[at..at + len].copy_from_slice(&data[src..src + len]);
                }
            }
            None => buf.extend_from_slice(&data[self.in_offs[k] as usize..][..size]),
        }
        for &(_, site, v) in self.cut.iter().flat_map(|c| c.patches_of(ord)) {
            buf[start + site as usize..][..8].copy_from_slice(&v.to_le_bytes());
        }
    }
}

/// The header positions of the sections of the `LC_SEGMENT_64` at
/// `cursor`.
fn segment_sections(
    bytes: &[u8],
    cursor: usize,
    cmdsize: usize,
) -> Result<impl Iterator<Item = usize>, MemberRelocError> {
    if cmdsize < SEGMENT_COMMAND_64_SIZE as usize {
        return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
    }
    let nsects = rd32(bytes, cursor + 64) as usize;
    let start = cursor + SEGMENT_COMMAND_64_SIZE as usize;
    let need = start + nsects * SECTION_64_SIZE as usize;
    if bytes.len() < need || cmdsize < need - cursor {
        return Err(MemberRelocError::TruncatedSegmentSections { offset: cursor });
    }
    Ok((0..nsects).map(move |i| start + i * SECTION_64_SIZE as usize))
}

pub(crate) fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// A 16-byte name slot without its NUL / space padding.
pub(crate) fn trim16(slot: &[u8; 16]) -> &[u8] {
    let end = slot
        .iter()
        .rposition(|&b| b != 0 && b != b' ')
        .map_or(0, |p| p + 1);
    &slot[..end]
}
