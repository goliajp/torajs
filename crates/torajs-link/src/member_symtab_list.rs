//! The rows of one integrated member that `LC_SYMTAB` lists when
//! member symbols are kept (`strip_member_symbols == false`).
//!
//! Every defined extern is listed, plus the member's local
//! `__TEXT,__text` symbols: without them a sampled PC inside a
//! file-local function resolves to whichever extern precedes it in
//! the image, often one from another member. Assembler / linker
//! temporaries (`l*`, `L*`, e.g. `ltmp0`) are skipped as ld64 does,
//! and so are nameless locals (the rows of atoms the runtime image
//! dropped when `tr` was built).
//! Rows whose atom the dead-strip pre-pass dropped are left out: the
//! rewriter keeps them (clamped to the section base) so reloc indices
//! and resolution hold, but they name no code in the image.

use torajs_obj::{MH_MAGIC_64, N_EXT, N_SECT, N_TYPE};

use crate::archive::{ArMember, MemberSymtabError};
use crate::member_obj::{MemberObj, symtab_lc_error, symtab_name_error};

/// Debugger (stab) entries carry type bits here; never listed.
const N_STAB: u8 = 0xe0;

/// `(name, n_value, n_sect)` in the member's own object-file terms.
pub type ListedSym = (String, u64, u8);

/// Listed rows of one member, split by binding: `LC_DYSYMTAB` wants
/// all locals ahead of all extern definitions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberSymtabList {
    pub locals: Vec<ListedSym>,
    pub externs: Vec<ListedSym>,
}

/// Collect the listed rows of member `bytes`, skipping `dead_rows`
/// (ascending nlist indices).
pub fn list_member_symbols(
    bytes: &[u8],
    dead_rows: &[u32],
) -> Result<MemberSymtabList, MemberSymtabError> {
    if bytes.len() < 32 || u32::from_le_bytes(bytes[0..4].try_into().unwrap()) != MH_MAGIC_64 {
        return Ok(MemberSymtabList::default());
    }
    let member = ArMember {
        name: "",
        data: bytes,
    };
    MemberObj::parse(member)
        .map_err(symtab_lc_error)?
        .list_symbols(dead_rows)
}

impl MemberObj<'_> {
    /// The listed rows of this member, skipping `dead_rows` (ascending
    /// nlist indices).
    pub(crate) fn list_symbols(
        &self,
        dead_rows: &[u32],
    ) -> Result<MemberSymtabList, MemberSymtabError> {
        let mut list = MemberSymtabList::default();
        let text_ords: Vec<u8> = self
            .sections()
            .iter()
            .filter(|s| &s.sectname[..7] == b"__text\0" && &s.segname[..7] == b"__TEXT\0")
            .map(|s| s.section_index)
            .collect();
        let Some(st) = self.symtab_if_any()? else {
            return Ok(list);
        };
        let mut dead = dead_rows.iter().peekable();
        for i in 0..st.nsyms {
            if dead.next_if(|&&r| r == i).is_some() {
                continue;
            }
            let sym = self.sym(&st, i);
            if sym.n_type & N_STAB != 0 || sym.n_type & N_TYPE != N_SECT {
                continue;
            }
            let external = sym.n_type & N_EXT != 0;
            if !external && !text_ords.contains(&sym.n_sect) {
                continue;
            }
            let name = self
                .sym_name(&st, i)
                .map_err(|e| symtab_name_error(e, i as usize))?;
            if !external && (name.is_empty() || name.starts_with(['l', 'L'])) {
                continue;
            }
            let row = (name.to_owned(), sym.n_value, sym.n_sect);
            if external {
                list.externs.push(row);
            } else {
                list.locals.push(row);
            }
        }
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_obj::{LC_SEGMENT_64, LC_SYMTAB};

    /// One `__TEXT,__text` + one `__DATA,__data` section and five
    /// rows: a live extern, a dead extern, a local fn, an `ltmp`
    /// temporary and a local data symbol.
    fn object() -> Vec<u8> {
        let names = ["_live", "_dead", "_local_fn", "ltmp0", "_local_data"];
        let rows: [(u8, u8, u64); 5] = [
            (N_SECT | N_EXT, 1, 0x10),
            (N_SECT | N_EXT, 1, 0x0),
            (N_SECT, 1, 0x20),
            (N_SECT, 1, 0x0),
            (N_SECT, 2, 0x40),
        ];
        let seg_size = 72 + 2 * 80;
        let symtab_cmd = 32 + seg_size;
        let symoff = symtab_cmd + 24;
        let stroff = symoff + rows.len() * 16;
        let mut strtab = vec![0u8];
        let mut strx = Vec::new();
        for n in names {
            strx.push(strtab.len() as u32);
            strtab.extend_from_slice(n.as_bytes());
            strtab.push(0);
        }
        let mut b = vec![0u8; stroff];
        b[0..4].copy_from_slice(&MH_MAGIC_64.to_le_bytes());
        b[16..20].copy_from_slice(&2u32.to_le_bytes());
        b[32..36].copy_from_slice(&LC_SEGMENT_64.to_le_bytes());
        b[36..40].copy_from_slice(&(seg_size as u32).to_le_bytes());
        b[96..100].copy_from_slice(&2u32.to_le_bytes());
        for (i, (sect, seg)) in [("__text", "__TEXT"), ("__data", "__DATA")]
            .iter()
            .enumerate()
        {
            let s = 32 + 72 + i * 80;
            b[s..s + sect.len()].copy_from_slice(sect.as_bytes());
            b[s + 16..s + 16 + seg.len()].copy_from_slice(seg.as_bytes());
        }
        let c = symtab_cmd;
        for (k, v) in [
            LC_SYMTAB,
            24,
            symoff as u32,
            rows.len() as u32,
            stroff as u32,
        ]
        .iter()
        .enumerate()
        {
            b[c + 4 * k..c + 4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[c + 20..c + 24].copy_from_slice(&(strtab.len() as u32).to_le_bytes());
        for (i, (t, s, v)) in rows.iter().enumerate() {
            let p = symoff + i * 16;
            b[p..p + 4].copy_from_slice(&strx[i].to_le_bytes());
            b[p + 4] = *t;
            b[p + 5] = *s;
            b[p + 8..p + 16].copy_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&strtab);
        b
    }

    #[test]
    fn lists_externs_and_text_locals_but_not_dead_rows() {
        let list = list_member_symbols(&object(), &[1]).unwrap();
        assert_eq!(list.externs, vec![("_live".to_owned(), 0x10, 1)]);
        assert_eq!(list.locals, vec![("_local_fn".to_owned(), 0x20, 1)]);
    }

    /// Through the dead-strip rewriter: with `_a` dead, `_b` moves to
    /// offset 0 and `_a`'s row is clamped there too. The rewriter must
    /// report that row so `_a` is not listed at `_b`'s address.
    #[test]
    fn rewriter_dead_rows_keep_a_dropped_fn_out_of_the_list() {
        use torajs_codegen::CompiledFunction;
        use torajs_codegen::frame::FrameLayout;
        let leaf = |name: &str| CompiledFunction {
            name: name.into(),
            bytes: [0xD5, 0x03, 0x20, 0x1F]
                .repeat(3)
                .into_iter()
                .chain([0xC0, 0x03, 0x5F, 0xD6])
                .collect(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        };
        let obj = torajs_obj::write_object(&[leaf("_a"), leaf("_b")]);
        let stripped = crate::dead_strip_rewrite::strip_member_section(
            &obj,
            1,
            &[(0, 16), (16, 32)],
            &[false, true],
            crate::dead_strip_rewrite::DeadRowPlacement::SectionBase,
        )
        .unwrap()
        .expect("half the section is dead");

        let all = list_member_symbols(&stripped.bytes, &[]).unwrap();
        assert_eq!(
            all.externs,
            vec![("_a".to_owned(), 0, 1), ("_b".to_owned(), 0, 1)]
        );
        let listed = list_member_symbols(&stripped.bytes, &stripped.dead_rows).unwrap();
        assert_eq!(listed.externs, vec![("_b".to_owned(), 0, 1)]);
    }

    #[test]
    fn without_dead_rows_every_extern_is_listed() {
        let list = list_member_symbols(&object(), &[]).unwrap();
        let names: Vec<&str> = list.externs.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(names, ["_live", "_dead"]);
    }
}
