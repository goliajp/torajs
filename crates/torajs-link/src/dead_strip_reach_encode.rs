//! A member's reachability table (layout in
//! [`crate::dead_strip_reach_table`]) built from the member's bytes:
//! what the runtime image stores beside each member when `tr` is
//! compiled, and what a link builds for a member the image holds no
//! table for.

use torajs_obj::{N_NO_DEAD_STRIP, N_SECT, N_TYPE, NLIST_64_SIZE};
use torajs_rtimage::archive_undef::member_undef_extern_rows;

use crate::archive::ArMember;
use crate::dead_strip_reach_build::{splits_at_symbols, trim16};
use crate::dead_strip_reach_table::{FOREIGN, HEADER_LEN, SECT_LEN};
use crate::member_sections::{MemberSectionInfo, collect_member_sections};
use crate::member_symtab::parse_member_symtab;

/// `(name, n_type, n_sect, n_value, n_desc)` of one nlist row.
type Row<'a> = (&'a str, u8, u8, u64, u16);

/// The member's reachability table; `sym_row` answers the row of a
/// name in the runtime image's symbol index.
pub(crate) fn encode_member_reach(
    member: &ArMember<'_>,
    sym_row: &dyn Fn(&str) -> Option<u32>,
) -> Result<Vec<u8>, String> {
    let sections =
        collect_member_sections(member).map_err(|e| format!("{}: sections: {e:?}", member.name))?;
    let (symoff, stroff, rows) = if member.data.len() >= 32 && !sections.is_empty() {
        nlist_rows(member)?
    } else {
        (0, 0, Vec::new())
    };
    let (defined, dup) = defined_by_name(&rows);
    // Defined rows flagged N_NO_DEAD_STRIP root every closure.
    let no_dead_strip: Vec<u32> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.1 & N_TYPE == N_SECT && r.4 & N_NO_DEAD_STRIP != 0)
        .map(|(i, _)| i as u32)
        .collect();
    let (sect_rows, atoms) = section_rows(member, &sections, &rows)?;
    let ImportCols {
        undef,
        refs: undef_ref,
        foreign,
    } = imports(member, sym_row)?;

    let n = |v: usize| table_u32(member, v);
    let mut out: Vec<u8> = Vec::with_capacity(
        HEADER_LEN
            + sect_rows.len()
            + 4 * (rows.len()
                + defined.len()
                + dup.len()
                + no_dead_strip.len()
                + atoms.len()
                + 2 * undef.len()
                + foreign.len()),
    );
    for v in [
        n(symoff)?,
        n(stroff)?,
        n(rows.len())?,
        n(sections.len())?,
        n(defined.len())?,
        n(dup.len())?,
        n(no_dead_strip.len())?,
        n(undef.len())?,
        n(foreign.len() / 2)?,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&sect_rows);
    for r in &rows {
        out.extend_from_slice(&n(r.0.len())?.to_le_bytes());
    }
    for v in defined
        .iter()
        .chain(&dup)
        .chain(&no_dead_strip)
        .chain(&atoms)
        .chain(&undef)
        .chain(&undef_ref)
        .chain(&foreign)
    {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(out)
}

/// The `undef`, `undef_ref` and `foreign` columns of a table.
#[derive(Default)]
struct ImportCols {
    undef: Vec<u32>,
    refs: Vec<u32>,
    foreign: Vec<u32>,
}

/// The member's imports in nlist order, read by the parser the member
/// closure reads them with: each one's nlist row, its reference (the
/// row of its name in the image's symbol index, or [`FOREIGN`] | k for
/// the k-th name the index lacks), and the `(offset, length)` of each
/// such name in the member's bytes.
fn imports(
    member: &ArMember<'_>,
    sym_row: &dyn Fn(&str) -> Option<u32>,
) -> Result<ImportCols, String> {
    // a member too short for a header defines nothing, so nothing
    // pulls it
    if member.data.len() < 32 {
        return Ok(Default::default());
    }
    let rows = member_undef_extern_rows(member).map_err(|e| format!("{}: {e:?}", member.name))?;
    let (mut undef, mut refs, mut foreign) = (Vec::new(), Vec::new(), Vec::new());
    let base = member.data.as_ptr() as usize;
    for (row, name) in rows {
        undef.push(row);
        let r = match sym_row(name) {
            Some(r) if r & FOREIGN == 0 => r,
            Some(_) => return Err(format!("{}: symbol index too large", member.name)),
            None => {
                let k = table_u32(member, foreign.len() / 2)?;
                // the parser hands back a slice of the member's bytes
                foreign.push(table_u32(member, name.as_ptr() as usize - base)?);
                foreign.push(table_u32(member, name.len())?);
                FOREIGN | k
            }
        };
        refs.push(r);
    }
    Ok(ImportCols {
        undef,
        refs,
        foreign,
    })
}

fn table_u32(member: &ArMember<'_>, v: usize) -> Result<u32, String> {
    u32::try_from(v).map_err(|_| format!("{}: table overflow", member.name))
}

/// The member's symbol and string table offsets and its nlist rows.
fn nlist_rows<'a>(member: &ArMember<'a>) -> Result<(usize, usize, Vec<Row<'a>>), String> {
    let bytes = member.data;
    let st = parse_member_symtab(member).map_err(|e| format!("{}: {e:?}", member.name))?;
    let (symoff, stroff, nsyms) = (st.symoff as usize, st.stroff as usize, st.nsyms as usize);
    let rows_present = bytes.len().saturating_sub(symoff) / NLIST_64_SIZE as usize;
    if rows_present < nsyms {
        return Err(format!(
            "{}: nlist row {rows_present} truncated",
            member.name
        ));
    }
    let rows = (0..nsyms)
        .map(|i| {
            let off = symoff + i * NLIST_64_SIZE as usize;
            let n_strx = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
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
            let n_desc = u16::from_le_bytes(bytes[off + 6..off + 8].try_into().unwrap());
            (name, bytes[off + 4], bytes[off + 5], n_value, n_desc)
        })
        .collect();
    Ok((symoff, stroff, rows))
}

/// The defined rows by name: the last `N_SECT` row of each name
/// answers a lookup; a name more than one row carries is listed apart.
fn defined_by_name(rows: &[Row<'_>]) -> (Vec<u32>, Vec<u32>) {
    let mut by_name: Vec<(&str, u32)> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.1 & N_TYPE == N_SECT)
        .map(|(i, r)| (r.0, i as u32))
        .collect();
    by_name.sort_by(|a, b| a.0.cmp(b.0));
    let mut defined: Vec<u32> = Vec::new();
    let mut dup: Vec<u32> = Vec::new();
    for run in by_name.chunk_by(|a, b| a.0 == b.0) {
        defined.push(run[run.len() - 1].1);
        if run.len() > 1 {
            dup.push(run[0].1);
        }
    }
    (defined, dup)
}

/// The table's section rows and the atom starts they index.
fn section_rows(
    member: &ArMember<'_>,
    sections: &[MemberSectionInfo],
    rows: &[Row<'_>],
) -> Result<(Vec<u8>, Vec<u32>), String> {
    // Only a member that declares `MH_SUBSECTIONS_VIA_SYMBOLS` splits
    // at its symbols; in any other (hand-written assembly:
    // compiler-builtins' outlined atomics) code may fall through from
    // one symbol into the next, so each section is one atom.
    let mut starts_by_ord: Vec<Vec<u64>> = vec![Vec::new(); usize::from(u8::MAX) + 1];
    if splits_at_symbols(member.data) {
        for r in rows.iter().filter(|r| r.1 & N_TYPE == N_SECT) {
            starts_by_ord[usize::from(r.2)].push(r.3);
        }
    }
    let n = |v: usize| table_u32(member, v);
    let mut sect_rows: Vec<u8> = Vec::with_capacity(sections.len() * SECT_LEN);
    let mut atoms: Vec<u32> = Vec::new();
    for s in sections {
        let mut starts = std::mem::take(&mut starts_by_ord[usize::from(s.section_index)]);
        starts.sort_unstable();
        starts.dedup();
        // Leading unnamed gap becomes atom 0.
        if starts.first().is_none_or(|&f| f > s.member_addr) {
            starts.insert(0, s.member_addr);
        }
        let atom_off = n(atoms.len())?;
        for &st in &starts {
            // an atom start is stored relative to its section, so a
            // symbol below the section or 4 GiB past it is refused
            let rel = st
                .checked_sub(s.member_addr)
                .and_then(|r| u32::try_from(r).ok())
                .ok_or_else(|| format!("{}: symbol at {st:#x} outside its section", member.name))?;
            atoms.push(rel);
        }
        sect_rows.extend_from_slice(&s.member_addr.to_le_bytes());
        sect_rows.extend_from_slice(&u64::from(s.size).to_le_bytes());
        for v in [
            s.member_internal_offset,
            s.flags,
            atom_off,
            n(starts.len())?,
        ] {
            sect_rows.extend_from_slice(&v.to_le_bytes());
        }
        sect_rows.extend_from_slice(&s.sectname);
        let is_text = trim16(&s.sectname) == b"__text" && trim16(&s.segname) == b"__TEXT";
        sect_rows.push(u8::from(is_text));
    }
    Ok((sect_rows, atoms))
}
