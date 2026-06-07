//! Member-internal `LC_SYMTAB` parser + name lookup — extracted
//! from `member_apply` so both the `__TEXT,__text` reloc patcher and
//! the `__DATA,*` reloc patcher (swap-2k chunk 2) share one bounds-
//! checked symtab walk instead of duplicating the load-command scan.
//!
//! The split keeps `member_apply.rs` within the file-size ratchet and
//! gives `member_data_apply.rs` a focused dependency: every consumer
//! lifts the same `(symoff, nsyms, stroff, strsize)` four-tuple plus a
//! `n_strx → UTF-8 name` resolver, so duplicating that logic across
//! two patchers would have invited drift.

use torajs_obj::{LC_SYMTAB, NLIST_64_SIZE};

use crate::archive::ArMember;
use crate::member_apply::MemberRelocApplyError;
use crate::member_reloc::MemberRelocError;
use crate::member_resolve::MemberSymtab;

/// Walk a member's load commands and pull the `LC_SYMTAB` four-tuple.
/// Bounds-checks the symoff/stroff ranges against the member buffer
/// up front so per-reloc loops can index without further validation.
pub(crate) fn parse_member_symtab(
    member: &ArMember<'_>,
) -> Result<MemberSymtab, MemberRelocApplyError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberRelocApplyError::Decode(
            MemberRelocError::TruncatedHeader,
        ));
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut cursor = 32usize;
    let mut symtab: Option<MemberSymtab> = None;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberRelocApplyError::Decode(
                MemberRelocError::TruncatedLoadCommand { offset: cursor },
            ));
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberRelocApplyError::Decode(
                MemberRelocError::TruncatedLoadCommand { offset: cursor },
            ));
        }
        if cmd == LC_SYMTAB {
            if cmdsize < 24 {
                return Err(MemberRelocApplyError::Decode(
                    MemberRelocError::TruncatedLoadCommand { offset: cursor },
                ));
            }
            symtab = Some(MemberSymtab {
                symoff: u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap()),
                nsyms: u32::from_le_bytes(bytes[cursor + 12..cursor + 16].try_into().unwrap()),
                stroff: u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap()),
                strsize: u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap()),
            });
        }
        cursor += cmdsize;
    }
    let s = symtab.ok_or(MemberRelocApplyError::NoSymtab)?;

    let symoff_us = s.symoff as usize;
    let nsyms_us = s.nsyms as usize;
    let stroff_us = s.stroff as usize;
    let strsize_us = s.strsize as usize;
    let nlist_end = symoff_us
        .checked_add(nsyms_us * NLIST_64_SIZE as usize)
        .ok_or(MemberRelocApplyError::TruncatedSymtab {
            symoff: symoff_us,
            nsyms: nsyms_us,
            file_size: bytes.len(),
        })?;
    if nlist_end > bytes.len() {
        return Err(MemberRelocApplyError::TruncatedSymtab {
            symoff: symoff_us,
            nsyms: nsyms_us,
            file_size: bytes.len(),
        });
    }
    let strtab_end =
        stroff_us
            .checked_add(strsize_us)
            .ok_or(MemberRelocApplyError::TruncatedStrtab {
                stroff: stroff_us,
                strsize: strsize_us,
                file_size: bytes.len(),
            })?;
    if strtab_end > bytes.len() {
        return Err(MemberRelocApplyError::TruncatedStrtab {
            stroff: stroff_us,
            strsize: strsize_us,
            file_size: bytes.len(),
        });
    }
    Ok(s)
}

/// Resolve `nlist[sym_index].n_strx` to a UTF-8 symbol name.
pub(crate) fn read_nlist_name(
    member: &ArMember<'_>,
    symtab: &MemberSymtab,
    sym_index: u32,
) -> Result<String, MemberRelocApplyError> {
    let bytes = member.data;
    let nlist_off = symtab.symoff as usize + sym_index as usize * NLIST_64_SIZE as usize;
    let n_strx = u32::from_le_bytes(bytes[nlist_off..nlist_off + 4].try_into().unwrap()) as usize;
    let stroff = symtab.stroff as usize;
    let strsize = symtab.strsize as usize;
    if n_strx >= strsize {
        return Err(MemberRelocApplyError::BadStrx { sym_index, n_strx });
    }
    let strtab = &bytes[stroff..stroff + strsize];
    let end = strtab[n_strx..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| n_strx + p)
        .ok_or(MemberRelocApplyError::UnterminatedSymName { sym_index, n_strx })?;
    let name_bytes = &strtab[n_strx..end];
    std::str::from_utf8(name_bytes)
        .map(|s| s.to_string())
        .map_err(|_| MemberRelocApplyError::NameNotUtf8 { sym_index })
}
