//! Writing the image: the header, then every section at its offset.

use super::{BuildError, HEADER_LEN, MAGIC, MEMBER_LEN, VERSION, align8, u32v};

/// The image's sections in file order, as `build_members` gathered them.
pub(super) struct Sections<'a> {
    pub(super) n_archives: usize,
    pub(super) archive_table: &'a [[u32; 2]],
    pub(super) slots: &'a [u32],
    pub(super) members: &'a [[u32; 8]],
    pub(super) n_syms: usize,
    pub(super) syms: &'a [u8],
    pub(super) names: &'a [u8],
    pub(super) blob: &'a [u8],
    pub(super) tables: &'a [u8],
    pub(super) colds: &'a [u8],
}

/// Lays the sections out behind the header and writes the image.
pub(super) fn assemble(s: Sections<'_>) -> Result<Vec<u8>, BuildError> {
    let Sections {
        n_archives,
        archive_table,
        slots,
        members,
        n_syms,
        syms,
        names,
        blob,
        tables,
        colds,
    } = s;
    let archives_off = HEADER_LEN;
    let slots_off = archives_off + archive_table.len() * 8;
    let members_off = slots_off + slots.len() * 4;
    let syms_off = members_off + members.len() * MEMBER_LEN;
    let names_off = syms_off + syms.len();
    let blob_off = align8(names_off + names.len());
    let tables_off = blob_off + blob.len();
    let colds_off = tables_off + tables.len();
    let total = colds_off + colds.len();

    let mut out: Vec<u8> = Vec::with_capacity(total);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    for n in [
        n_archives,
        slots.len(),
        members.len(),
        n_syms,
        archives_off,
        slots_off,
        members_off,
        syms_off,
        names_off,
        names.len(),
        blob_off,
        blob.len(),
        tables_off,
        tables.len(),
        colds_off,
        colds.len(),
    ] {
        out.extend_from_slice(&u32v(n)?.to_le_bytes());
    }
    debug_assert_eq!(out.len(), HEADER_LEN);
    let rows = archive_table.iter().flatten().chain(slots);
    for v in rows.chain(members.iter().flatten()) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(syms);
    out.extend_from_slice(names);
    out.resize(blob_off, 0);
    out.extend_from_slice(blob);
    out.extend_from_slice(tables);
    out.extend_from_slice(colds);
    Ok(out)
}
