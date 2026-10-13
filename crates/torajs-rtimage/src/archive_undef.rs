//! Undefined-extern names of one Mach-O member: the imports the
//! linker's member closure follows. Here so the image builder, which
//! walks the same closure to find the members no program can pull,
//! and the linker read them with one parser.

use crate::archive::{ArMember, MemberSymtabError};

const MH_MAGIC_64: u32 = 0xFEED_FACF;
const LC_SYMTAB: u32 = 0x2;
const N_TYPE: u8 = 0x0e;
const N_UNDF: u8 = 0x0;
const N_EXT: u8 = 0x01;

/// Walk one `.o` member's `LC_SYMTAB` and return every
/// `(N_UNDF | N_EXT)` symbol name — those are the *imports* the
/// member needs satisfied by something else in the link.
///
/// Mirror of `archive::parse_member_defined_externs` with the
/// filter flipped to `N_UNDF`. Defined externs and undefined
/// externs sit in the same `LC_SYMTAB`, distinguished only by
/// `n_type & N_TYPE`.
#[inline]
pub fn member_undef_extern_names<'a>(
    member: &ArMember<'a>,
) -> Result<Vec<&'a str>, MemberSymtabError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberSymtabError::TruncatedHeader);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Ok(Vec::new());
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;

    let mut cursor = 32usize;
    let mut symtab: Option<(u32, u32, u32, u32)> = None;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberSymtabError::TruncatedLoadCommand { offset: cursor });
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberSymtabError::TruncatedLoadCommand { offset: cursor });
        }
        if cmd == LC_SYMTAB {
            if cmdsize < 24 {
                return Err(MemberSymtabError::TruncatedSymtabCmd { offset: cursor });
            }
            let symoff = u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap());
            let nsyms = u32::from_le_bytes(bytes[cursor + 12..cursor + 16].try_into().unwrap());
            let stroff = u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap());
            let strsize = u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap());
            symtab = Some((symoff, nsyms, stroff, strsize));
        }
        cursor += cmdsize;
    }

    let Some((symoff, nsyms, stroff, strsize)) = symtab else {
        return Ok(Vec::new());
    };

    let symoff = symoff as usize;
    let stroff = stroff as usize;
    let strsize = strsize as usize;
    let nsyms = nsyms as usize;
    if bytes.len() < symoff + nsyms * 16 {
        return Err(MemberSymtabError::TruncatedSymtab {
            symoff,
            nsyms,
            file_size: bytes.len(),
        });
    }
    if bytes.len() < stroff + strsize {
        return Err(MemberSymtabError::TruncatedStrtab {
            stroff,
            strsize,
            file_size: bytes.len(),
        });
    }

    let strtab = &bytes[stroff..stroff + strsize];
    let mut out: Vec<&'a str> = Vec::new();
    for i in 0..nsyms {
        let off = symoff + i * 16;
        let n_strx = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let n_type = bytes[off + 4];
        if (n_type & N_TYPE) != N_UNDF {
            continue;
        }
        if (n_type & N_EXT) == 0 {
            continue;
        }
        if n_strx >= strtab.len() {
            return Err(MemberSymtabError::BadStrx {
                sym_index: i,
                n_strx,
            });
        }
        let end_off = strtab[n_strx..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| n_strx + p)
            .ok_or(MemberSymtabError::UnterminatedSymName {
                sym_index: i,
                n_strx,
            })?;
        let name_bytes = &strtab[n_strx..end_off];
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| MemberSymtabError::NameNotUtf8 { sym_index: i })?;
        out.push(name);
    }
    Ok(out)
}
