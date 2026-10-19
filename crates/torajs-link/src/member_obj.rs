//! One runtime member as the link reads it once the dead-strip
//! pre-pass is done: its sections, reloc tables, section payloads and
//! nlist rows. The load commands are walked once per member and link
//! ([`MergedArchives::obj`]); every layout and emit stage reads the
//! result instead of walking the member's bytes again.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use torajs_obj::{
    LC_SEGMENT_64, LC_SYMTAB, MH_MAGIC_64, N_EXT, N_SECT, N_TYPE, NLIST_64_SIZE, SECTION_64_SIZE,
    SEGMENT_COMMAND_64_SIZE,
};

use crate::archive::{ArMember, MemberSymtabError};
use crate::archives_merge::MergedArchives;
use crate::member_apply::MemberRelocApplyError;
use crate::member_reloc::MemberRelocError;
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

    /// Forget the parse of `key`, whose bytes changed.
    pub(crate) fn forget_obj(&mut self, key: (usize, usize)) {
        self.objs.0.get_mut().remove(&key);
    }
}

/// What is wrong with a member's `LC_SYMTAB`, kept until a reader that
/// needs the table reports it in its own terms.
#[derive(Debug, Clone, Copy)]
enum SymtabFault {
    Missing,
    Nlist { symoff: usize, nsyms: usize },
    Strtab { stroff: usize, strsize: usize },
}

/// One nlist row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sym {
    pub(crate) n_strx: u32,
    pub(crate) n_type: u8,
    pub(crate) n_sect: u8,
    pub(crate) n_value: u64,
}

#[derive(Debug)]
pub struct MemberObj<'a> {
    member: ArMember<'a>,
    /// Every section, ordinal `i + 1` at index `i`.
    sections: Vec<MemberSectionInfo>,
    symtab: Result<MemberSymtab, SymtabFault>,
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
                if cmdsize < SEGMENT_COMMAND_64_SIZE as usize {
                    return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
                }
                let nsects = rd32(bytes, cursor + 64) as usize;
                let start = cursor + SEGMENT_COMMAND_64_SIZE as usize;
                let need = start + nsects * SECTION_64_SIZE as usize;
                if bytes.len() < need || cmdsize < need - cursor {
                    return Err(MemberRelocError::TruncatedSegmentSections { offset: cursor });
                }
                for i in 0..nsects {
                    let sec = start + i * SECTION_64_SIZE as usize;
                    let index = u8::try_from(sections.len() + 1)
                        .expect("> 255 sections per Mach-O image not supported");
                    sections.push(section_info(bytes, sec, index));
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
            symtab,
        })
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

    /// Row `i` of the symbol table `st` (from [`MemberObj::symtab`]).
    pub(crate) fn sym(&self, st: &MemberSymtab, i: u32) -> Sym {
        let b = self.member.data;
        let p = st.symoff as usize + i as usize * NLIST_64_SIZE as usize;
        Sym {
            n_strx: rd32(b, p),
            n_type: b[p + 4],
            n_sect: b[p + 5],
            n_value: u64::from_le_bytes(b[p + 8..p + 16].try_into().unwrap()),
        }
    }

    /// The name of row `i` of `st`.
    pub fn sym_name(&self, st: &MemberSymtab, i: u32) -> Result<&'a str, MemberRelocApplyError> {
        let n_strx = self.sym(st, i).n_strx as usize;
        let strtab = self.strtab(st);
        if n_strx >= strtab.len() {
            return Err(MemberRelocApplyError::BadStrx {
                sym_index: i,
                n_strx,
            });
        }
        let len = strtab[n_strx..].iter().position(|&b| b == 0).ok_or(
            MemberRelocApplyError::UnterminatedSymName {
                sym_index: i,
                n_strx,
            },
        )?;
        std::str::from_utf8(&strtab[n_strx..n_strx + len])
            .map_err(|_| MemberRelocApplyError::NameNotUtf8 { sym_index: i })
    }

    fn strtab(&self, st: &MemberSymtab) -> &'a [u8] {
        &self.member.data[st.stroff as usize..][..st.strsize as usize]
    }

    /// Every `N_SECT | N_EXT` row as `(name, n_value, n_sect)`, in
    /// nlist order; empty without a symbol table.
    pub fn defined_externs(&self) -> Result<Vec<(&'a str, u64, u8)>, MemberSymtabError> {
        let st = match self.symtab {
            Ok(st) => st,
            Err(SymtabFault::Missing) => return Ok(Vec::new()),
            Err(f) => return Err(self.symtab_error(f)),
        };
        let mut out = Vec::new();
        for i in 0..st.nsyms {
            let s = self.sym(&st, i);
            if s.n_type & N_TYPE != N_SECT || s.n_type & N_EXT == 0 {
                continue;
            }
            let name = self
                .sym_name(&st, i)
                .map_err(|e| symtab_name_error(e, i as usize))?;
            out.push((name, s.n_value, s.n_sect));
        }
        Ok(out)
    }

    fn symtab_error(&self, f: SymtabFault) -> MemberSymtabError {
        let file_size = self.member.data.len();
        match f {
            SymtabFault::Missing => unreachable!("a missing table is not an error here"),
            SymtabFault::Nlist { symoff, nsyms } => MemberSymtabError::TruncatedSymtab {
                symoff,
                nsyms,
                file_size,
            },
            SymtabFault::Strtab { stroff, strsize } => MemberSymtabError::TruncatedStrtab {
                stroff,
                strsize,
                file_size,
            },
        }
    }
}

/// The `LC_SYMTAB` at `cmd`, checked against the member's size.
fn checked_symtab(bytes: &[u8], cmd: usize) -> Result<MemberSymtab, SymtabFault> {
    let st = MemberSymtab {
        symoff: rd32(bytes, cmd + 8),
        nsyms: rd32(bytes, cmd + 12),
        stroff: rd32(bytes, cmd + 16),
        strsize: rd32(bytes, cmd + 20),
    };
    let (symoff, nsyms) = (st.symoff as usize, st.nsyms as usize);
    if bytes.len() < symoff + nsyms * NLIST_64_SIZE as usize {
        return Err(SymtabFault::Nlist { symoff, nsyms });
    }
    let (stroff, strsize) = (st.stroff as usize, st.strsize as usize);
    if bytes.len() < stroff + strsize {
        return Err(SymtabFault::Strtab { stroff, strsize });
    }
    Ok(st)
}

/// A row-name failure in [`MemberSymtabError`] terms.
fn symtab_name_error(e: MemberRelocApplyError, sym_index: usize) -> MemberSymtabError {
    match e {
        MemberRelocApplyError::BadStrx { n_strx, .. } => {
            MemberSymtabError::BadStrx { sym_index, n_strx }
        }
        MemberRelocApplyError::UnterminatedSymName { n_strx, .. } => {
            MemberSymtabError::UnterminatedSymName { sym_index, n_strx }
        }
        _ => MemberSymtabError::NameNotUtf8 { sym_index },
    }
}

fn rd32(b: &[u8], o: usize) -> u32 {
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
