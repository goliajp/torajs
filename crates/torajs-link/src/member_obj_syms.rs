//! The nlist half of [`MemberObj`]: the bounds-checked symbol table
//! and its rows, names and defined externs.

use torajs_obj::{N_EXT, N_SECT, N_TYPE, N_UNDF, NLIST_64_SIZE};

use crate::archive::MemberSymtabError;
use crate::member_apply::MemberRelocApplyError;
use crate::member_obj::{MemberObj, rd32};
use crate::member_reloc::MemberRelocError;
use crate::member_resolve::MemberSymtab;

/// What is wrong with a member's `LC_SYMTAB`, kept until a reader that
/// needs the table reports it in its own terms.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SymtabFault {
    Missing,
    /// The command is shorter than an `LC_SYMTAB`.
    Cmd {
        offset: usize,
    },
    Nlist {
        symoff: usize,
        nsyms: usize,
    },
    Strtab {
        stroff: usize,
        strsize: usize,
    },
}

/// One nlist row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sym {
    pub(crate) n_strx: u32,
    pub(crate) n_type: u8,
    pub(crate) n_sect: u8,
    pub(crate) n_value: u64,
}

impl<'a> MemberObj<'a> {
    /// The bounds-checked `LC_SYMTAB`.
    pub fn symtab(&self) -> Result<MemberSymtab, MemberRelocApplyError> {
        self.symtab.map_err(|f| match f {
            SymtabFault::Missing => MemberRelocApplyError::NoSymtab,
            SymtabFault::Cmd { offset } => {
                MemberRelocApplyError::Decode(MemberRelocError::TruncatedLoadCommand { offset })
            }
            SymtabFault::Nlist { symoff, nsyms } => MemberRelocApplyError::TruncatedSymtab {
                symoff,
                nsyms,
                file_size: self.member.data.len(),
            },
            SymtabFault::Strtab { stroff, strsize } => MemberRelocApplyError::TruncatedStrtab {
                stroff,
                strsize,
                file_size: self.member.data.len(),
            },
        })
    }

    /// The symbol table, `None` when the member has none, failures in
    /// [`MemberSymtabError`] terms.
    pub(crate) fn symtab_if_any(&self) -> Result<Option<MemberSymtab>, MemberSymtabError> {
        match self.symtab {
            Ok(st) => Ok(Some(st)),
            Err(SymtabFault::Missing) => Ok(None),
            Err(f) => Err(self.symtab_error(f)),
        }
    }

    /// Row `i` of the symbol table `st` (from [`MemberObj::symtab`]).
    pub(crate) fn sym(&self, st: &MemberSymtab, i: u32) -> Sym {
        let mut s = self.raw_sym(st, i);
        if let Some(cut) = &self.cut {
            if s.n_type & N_TYPE == N_SECT {
                s.n_value = cut.move_row(s.n_sect, s.n_value).0;
            }
            if cut.is_neutered(i) {
                s.n_type &= !N_EXT;
            }
        }
        s
    }

    /// Row `i` as the member's bytes hold it.
    fn raw_sym(&self, st: &MemberSymtab, i: u32) -> Sym {
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
        let p = st.symoff as usize + i as usize * NLIST_64_SIZE as usize;
        name_at(self.strtab(st), rd32(self.member.data, p), i)
    }

    fn strtab(&self, st: &MemberSymtab) -> &'a [u8] {
        &self.member.data[st.stroff as usize..][..st.strsize as usize]
    }

    /// The nlist rows of `st`.
    fn rows(&self, st: &MemberSymtab) -> &'a [[u8; 16]] {
        let len = st.nsyms as usize * NLIST_64_SIZE as usize;
        self.member.data[st.symoff as usize..][..len].as_chunks().0
    }

    /// Every `N_SECT | N_EXT` row as `(name, n_value, n_sect)`, in
    /// nlist order; empty without a symbol table.
    pub fn defined_externs(&self) -> Result<Vec<(&'a str, u64, u8)>, MemberSymtabError> {
        let Some(st) = self.symtab_if_any()? else {
            return Ok(Vec::new());
        };
        let strtab = self.strtab(&st);
        let mut out = Vec::new();
        for (i, row) in self.rows(&st).iter().enumerate() {
            // a cut moves the value of such a row, never its type
            if row[4] & N_TYPE != N_SECT || row[4] & N_EXT == 0 {
                continue;
            }
            let name =
                name_at(strtab, rd32(row, 0), i as u32).map_err(|e| symtab_name_error(e, i))?;
            let v = u64::from_le_bytes(row[8..16].try_into().unwrap());
            let n_value = match &self.cut {
                Some(cut) => cut.move_row(row[5], v).0,
                None => v,
            };
            out.push((name, n_value, row[5]));
        }
        Ok(out)
    }

    /// Every `N_UNDF | N_EXT` row's name, in nlist order; empty
    /// without a symbol table.
    pub fn undef_externs(&self) -> Result<Vec<&'a str>, MemberSymtabError> {
        let Some(st) = self.symtab_if_any()? else {
            return Ok(Vec::new());
        };
        let strtab = self.strtab(&st);
        let mut out = Vec::new();
        for (i, row) in self.rows(&st).iter().enumerate() {
            // the type alone: a cut moves no undef row's value
            let neutered = self.cut.as_ref().is_some_and(|c| c.is_neutered(i as u32));
            if row[4] & N_TYPE != N_UNDF || row[4] & N_EXT == 0 || neutered {
                continue;
            }
            let name =
                name_at(strtab, rd32(row, 0), i as u32).map_err(|e| symtab_name_error(e, i))?;
            out.push(name);
        }
        Ok(out)
    }

    fn symtab_error(&self, f: SymtabFault) -> MemberSymtabError {
        let file_size = self.member.data.len();
        match f {
            SymtabFault::Missing => unreachable!("a missing table is not an error here"),
            SymtabFault::Cmd { offset } => MemberSymtabError::TruncatedSymtabCmd { offset },
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

/// The NUL-terminated UTF-8 name at `n_strx` of `strtab`, for row `i`.
fn name_at(strtab: &[u8], n_strx: u32, i: u32) -> Result<&str, MemberRelocApplyError> {
    let n_strx = n_strx as usize;
    let tail = strtab.get(n_strx..).filter(|t| !t.is_empty());
    let tail = tail.ok_or(MemberRelocApplyError::BadStrx {
        sym_index: i,
        n_strx,
    })?;
    let len =
        tail.iter()
            .position(|&b| b == 0)
            .ok_or(MemberRelocApplyError::UnterminatedSymName {
                sym_index: i,
                n_strx,
            })?;
    std::str::from_utf8(&tail[..len])
        .map_err(|_| MemberRelocApplyError::NameNotUtf8 { sym_index: i })
}

/// The `LC_SYMTAB` at `cmd`, checked against the member's size.
pub(crate) fn checked_symtab(bytes: &[u8], cmd: usize) -> Result<MemberSymtab, SymtabFault> {
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

/// A load-command walk failure in [`MemberSymtabError`] terms.
pub(crate) fn symtab_lc_error(e: MemberRelocError) -> MemberSymtabError {
    match e {
        MemberRelocError::TruncatedLoadCommand { offset }
        | MemberRelocError::TruncatedSegmentSections { offset } => {
            MemberSymtabError::TruncatedLoadCommand { offset }
        }
        _ => MemberSymtabError::TruncatedHeader,
    }
}

/// A row-name failure in [`MemberSymtabError`] terms.
pub(crate) fn symtab_name_error(e: MemberRelocApplyError, sym_index: usize) -> MemberSymtabError {
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

#[cfg(test)]
mod tests {
    use crate::archive::{ArMember, MemberSymtabError};
    use crate::member_obj::{MemberObj, rd32};
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;

    #[test]
    fn a_short_symtab_command_is_reported_by_the_readers_of_the_table() {
        let f = CompiledFunction {
            name: "_f".into(),
            bytes: vec![0xC0, 0x03, 0x5F, 0xD6],
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        };
        let mut bytes = torajs_obj::write_object(&[f]);
        let at = 184;
        assert_eq!(rd32(&bytes, at), torajs_obj::LC_SYMTAB);
        bytes[at + 4..at + 8].copy_from_slice(&16u32.to_le_bytes());
        let obj = MemberObj::parse(ArMember {
            name: crate::archive::MemberName::new("m.o"),
            data: &bytes,
        })
        .unwrap();
        assert!(obj.text_section().is_some());
        assert_eq!(
            obj.symtab_if_any().unwrap_err(),
            MemberSymtabError::TruncatedSymtabCmd { offset: at }
        );
    }
}
