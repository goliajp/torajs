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

impl<'a> MemberObj<'a> {
    /// The bounds-checked `LC_SYMTAB`.
    pub fn symtab(&self) -> Result<MemberSymtab, MemberRelocApplyError> {
        self.symtab.map_err(|f| match f {
            SymtabFault::Missing => MemberRelocApplyError::NoSymtab,
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
        let n_strx = self.raw_sym(st, i).n_strx as usize;
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
        let Some(st) = self.symtab_if_any()? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for i in 0..st.nsyms {
            // a cut moves the value of such a row, never its type
            let s = self.raw_sym(&st, i);
            if s.n_type & N_TYPE != N_SECT || s.n_type & N_EXT == 0 {
                continue;
            }
            let name = self
                .sym_name(&st, i)
                .map_err(|e| symtab_name_error(e, i as usize))?;
            let n_value = match &self.cut {
                Some(cut) => cut.move_row(s.n_sect, s.n_value).0,
                None => s.n_value,
            };
            out.push((name, n_value, s.n_sect));
        }
        Ok(out)
    }

    /// Every `N_UNDF | N_EXT` row's name, in nlist order; empty
    /// without a symbol table.
    pub fn undef_externs(&self) -> Result<Vec<&'a str>, MemberSymtabError> {
        let Some(st) = self.symtab_if_any()? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for i in 0..st.nsyms {
            // the type alone: a cut moves no undef row's value
            let t = self.member.data[st.symoff as usize + i as usize * NLIST_64_SIZE as usize + 4];
            let neutered = self.cut.as_ref().is_some_and(|c| c.is_neutered(i));
            if t & N_TYPE != N_UNDF || t & N_EXT == 0 || neutered {
                continue;
            }
            let name = self
                .sym_name(&st, i)
                .map_err(|e| symtab_name_error(e, i as usize))?;
            out.push(name);
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
