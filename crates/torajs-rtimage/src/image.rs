//! The runtime image: every `libtorajs_*.a` the compiler links,
//! preprocessed once when `tr` itself is built.
//!
//! What a `tr build` used to do per run — parse 37 archives, walk
//! 816 members' symbol tables (281k nlist rows), merge them first-
//! archive-wins into one index — the builder does here, and the
//! reader hands the result back as slices into the image bytes with
//! no parsing beyond the fixed header. Members are deduplicated by
//! `(name, bytes)`: each archive bundles its own copy of `core` /
//! `alloc` / `std`. [`build_reachable`], what `tr` embeds, goes
//! further and keeps only the members some program can pull
//! ([`crate::prune`]): of the copies only the one winning names in the
//! merged index, and of the rest only what the runtime's closure
//! reaches.
//!
//! Layout (little-endian, offsets absolute within the image):
//!
//! ```text
//!   header    MAGIC(8) version n_archives n_slots n_members n_syms
//!             archives_off slots_off members_off syms_off
//!             names_off names_len blob_off blob_len
//!             tables_off tables_len colds_off colds_len
//!   archives  n_archives × { slot_start, slot_count }       (u32 ×2)
//!   slots     n_slots × member_id                           (u32)
//!   members   n_members × { name_off, name_len, data_off,
//!                            data_len, table_off, table_len,
//!                            cold_off, cold_len }           (u32 ×8)
//!   syms      n_syms × { name_off, name_len, archive_idx,
//!                        member_idx, n_value(u64) }         (24 B)
//!   names     member and symbol names, no terminators
//!   blob      distinct kept member bodies
//!   tables    one table per distinct member, opaque here
//!   colds     one cold table per distinct member, opaque here
//! ```
//!
//! A member's tables are whatever the image's consumer derived from
//! the member's bytes when the image was built
//! ([`build_reachable_from_members_with_tables`]), so a link reads them
//! instead of parsing the bytes again; an empty table means none was
//! built. The cold tables sit apart from the tables, for what a reader
//! needs of only some of the members it reads tables of: the tables it
//! reads of every member stay packed together.
//!
//! A slot is one kept `(archive, member)` position, in the original
//! walk's order, so the linker's member ordering and first-wins
//! resolution are unchanged; a symbol's `member_idx` counts the kept
//! members of its archive. `syms` holds the names whose winner is
//! kept, sorted by name bytes, which is the order a `BTreeMap<&str, _>`
//! bulk-builds from.

use std::collections::{BTreeMap, HashMap};

use crate::archive::{
    ArMember, ArParseError, ArchiveParseError, build_archive_index, parse_archive,
};
use crate::prune::{Winners, live_members};

mod assemble;
mod rows;
mod view;
use assemble::{Sections, assemble};
pub use rows::SymbolRows;
use rows::{Names, sym_rows};
pub use view::{Image, ImageError, ImageSymbol};

const MAGIC: &[u8; 8] = b"TRIMG\0\0\0";
const VERSION: u32 = 3;
const HEADER_LEN: usize = 8 + 4 * 17;
const MEMBER_LEN: usize = 32;
const SYM_LEN: usize = 24;

/// Failures of [`build`], blamed on one archive.
#[derive(Debug)]
pub enum BuildError {
    Ar {
        archive_idx: usize,
        err: ArParseError,
    },
    Member {
        archive_idx: usize,
        err: ArchiveParseError,
    },
    /// The table callback refused a member.
    Table {
        archive_idx: usize,
        member: String,
        err: String,
    },
    /// A count or offset does not fit the image's 32-bit fields.
    TooLarge,
}

/// The two tables stored for one member body.
#[derive(Debug, Default)]
pub struct MemberTables {
    pub table: Vec<u8>,
    pub cold: Vec<u8>,
}

/// Builds the tables stored for one member body; the rows the image's
/// symbol index will give each name come along, so a table can name a
/// symbol by its row.
type TableFn<'f> = dyn FnMut(&ArMember<'_>, &SymbolRows<'_>) -> Result<MemberTables, String> + 'f;

fn u32v(n: usize) -> Result<u32, BuildError> {
    u32::try_from(n).map_err(|_| BuildError::TooLarge)
}

/// Preprocess `archives` (in link search order) into an image holding
/// every member.
pub fn build(archives: &[&[u8]]) -> Result<Vec<u8>, BuildError> {
    build_members(&parse_all(archives)?, false, &mut |_, _| {
        Ok(MemberTables::default())
    })
}

/// [`build`] keeping only the members some program can pull into its
/// link; the names won by a dropped member leave the index, so a
/// lookup of one misses instead of pulling a different member.
pub fn build_reachable(archives: &[&[u8]]) -> Result<Vec<u8>, BuildError> {
    build_members(&parse_all(archives)?, true, &mut |_, _| {
        Ok(MemberTables::default())
    })
}

/// [`build_reachable`] over archives already split into members, in
/// link search order (members a caller rewrote included), storing
/// `table(member)`'s tables for each distinct kept member body.
pub fn build_reachable_from_members_with_tables(
    archives: &[Vec<ArMember<'_>>],
    table: &mut TableFn<'_>,
) -> Result<Vec<u8>, BuildError> {
    build_members(archives, true, table)
}

fn parse_all<'a>(archives: &[&'a [u8]]) -> Result<Vec<Vec<ArMember<'a>>>, BuildError> {
    archives
        .iter()
        .enumerate()
        .map(|(archive_idx, bytes)| {
            parse_archive(bytes).map_err(|err| BuildError::Ar { archive_idx, err })
        })
        .collect()
}

/// The first-archive-wins index of every defined name, and which
/// members the image keeps.
fn winners_and_live<'a>(
    parsed: &[Vec<ArMember<'a>>],
    reachable_only: bool,
) -> Result<(Winners<'a>, Vec<Vec<bool>>), BuildError> {
    let mut winners: Winners<'_> = BTreeMap::new();
    for (archive_idx, members) in parsed.iter().enumerate() {
        let per_archive =
            build_archive_index(members).map_err(|err| BuildError::Member { archive_idx, err })?;
        for (name, sym) in per_archive {
            winners
                .entry(name)
                .or_insert((u32v(archive_idx)?, u32v(sym.member_idx)?, sym.n_value));
        }
    }
    let live = if reachable_only {
        live_members(parsed, &winners).map_err(|e| BuildError::Member {
            archive_idx: e.archive_idx,
            err: e.err,
        })?
    } else {
        parsed.iter().map(|ms| vec![true; ms.len()]).collect()
    };
    Ok((winners, live))
}

fn build_members(
    parsed: &[Vec<ArMember<'_>>],
    reachable_only: bool,
    table: &mut TableFn<'_>,
) -> Result<Vec<u8>, BuildError> {
    let (winners, live) = winners_and_live(parsed, reachable_only)?;
    let indexed = SymbolRows::kept_names(&winners, &live)?;
    let rows = SymbolRows { names: &indexed };
    let mut names = Names::default();
    let mut blob: Vec<u8> = Vec::new();
    let mut tables: Vec<u8> = Vec::new();
    let mut colds: Vec<u8> = Vec::new();
    let mut member_ids: HashMap<(&str, &[u8]), u32> = HashMap::new();
    // (name_off, name_len, data_off, data_len, table_off, table_len,
    //  cold_off, cold_len)
    let mut members: Vec<[u32; 8]> = Vec::new();
    let mut archive_table: Vec<[u32; 2]> = Vec::new();
    let mut slots: Vec<u32> = Vec::new();
    // original member index -> index among the archive's kept members
    let mut renumber: Vec<Vec<Option<u32>>> = Vec::with_capacity(parsed.len());

    for (a, archive_members) in parsed.iter().enumerate() {
        let start = slots.len();
        let mut kept: Vec<Option<u32>> = vec![None; archive_members.len()];
        for (i, m) in archive_members.iter().enumerate() {
            if !live[a][i] {
                continue;
            }
            kept[i] = Some(u32v(slots.len() - start)?);
            let id = match member_ids.get(&(m.name, m.data)) {
                Some(&id) => id,
                None => {
                    let (name_off, name_len) = names.intern(m.name)?;
                    let data_off = u32v(blob.len())?;
                    blob.extend_from_slice(m.data);
                    let t = table(m, &rows).map_err(|err| BuildError::Table {
                        archive_idx: a,
                        member: m.name.to_string(),
                        err,
                    })?;
                    let (table_off, cold_off) = (u32v(tables.len())?, u32v(colds.len())?);
                    tables.extend_from_slice(&t.table);
                    colds.extend_from_slice(&t.cold);
                    members.push([
                        name_off,
                        name_len,
                        data_off,
                        u32v(m.data.len())?,
                        table_off,
                        u32v(t.table.len())?,
                        cold_off,
                        u32v(t.cold.len())?,
                    ]);
                    let id = u32v(members.len() - 1)?;
                    member_ids.insert((m.name, m.data), id);
                    id
                }
            };
            slots.push(id);
        }
        archive_table.push([u32v(start)?, u32v(slots.len() - start)?]);
        renumber.push(kept);
    }

    let (n_syms, syms) = sym_rows(winners, &renumber, &mut names)?;
    debug_assert_eq!(n_syms, indexed.len());

    assemble(Sections {
        n_archives: parsed.len(),
        archive_table: &archive_table,
        slots: &slots,
        members: &members,
        n_syms,
        syms: &syms,
        names: &names.bytes,
        blob: &blob,
        tables: &tables,
        colds: &colds,
    })
}

fn align8(n: usize) -> usize {
    (n + 7) & !7
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};

    fn archive_of(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(AR_MAGIC);
        for (name, data) in members {
            let mut header = [b' '; AR_HEADER_SIZE];
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[16] = b'0';
            let size_str = format!("{}", data.len());
            header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
            header[58] = b'`';
            header[59] = b'\n';
            archive.extend_from_slice(&header);
            archive.extend_from_slice(data);
            if !data.len().is_multiple_of(2) {
                archive.push(0);
            }
        }
        archive
    }

    /// One `.o` with `LC_SYMTAB` naming `syms` as defined externs in
    /// section 1 at consecutive 4-byte offsets.
    fn object_with_syms(syms: &[&str]) -> Vec<u8> {
        object_with(syms, &[])
    }

    /// [`object_with_syms`] that also imports `undefs`.
    fn object_with(syms: &[&str], undefs: &[&str]) -> Vec<u8> {
        let mut strtab: Vec<u8> = vec![0];
        let mut nlist: Vec<u8> = Vec::new();
        let rows = syms.iter().map(|s| (s, 0x0f, 1)); // N_SECT | N_EXT
        let rows = rows.chain(undefs.iter().map(|s| (s, 0x01, 0))); // N_UNDF | N_EXT
        for (i, (s, n_type, n_sect)) in rows.enumerate() {
            nlist.extend_from_slice(&(strtab.len() as u32).to_le_bytes());
            nlist.push(n_type);
            nlist.push(n_sect);
            nlist.extend_from_slice(&0u16.to_le_bytes());
            nlist.extend_from_slice(&((i * 4) as u64).to_le_bytes());
            strtab.extend_from_slice(s.as_bytes());
            strtab.push(0);
        }
        let syms = nlist.len() / 16;
        let symoff = 32 + 24;
        let stroff = symoff + nlist.len();
        let mut o: Vec<u8> = Vec::new();
        o.extend_from_slice(&0xFEED_FACFu32.to_le_bytes());
        o.extend_from_slice(&[0u8; 12]);
        o.extend_from_slice(&1u32.to_le_bytes()); // ncmds
        o.extend_from_slice(&24u32.to_le_bytes()); // sizeofcmds
        o.extend_from_slice(&[0u8; 8]);
        o.extend_from_slice(&0x2u32.to_le_bytes()); // LC_SYMTAB
        o.extend_from_slice(&24u32.to_le_bytes());
        o.extend_from_slice(&(symoff as u32).to_le_bytes());
        o.extend_from_slice(&(syms as u32).to_le_bytes());
        o.extend_from_slice(&(stroff as u32).to_le_bytes());
        o.extend_from_slice(&(strtab.len() as u32).to_le_bytes());
        o.extend_from_slice(&nlist);
        o.extend_from_slice(&strtab);
        o
    }

    #[test]
    fn empty_image_round_trips() {
        let img = build(&[]).unwrap();
        let v = Image::parse(&img).unwrap();
        assert_eq!(v.archive_count(), 0);
        assert_eq!(v.symbols().count(), 0);
    }

    #[test]
    fn slots_keep_archive_indexing_and_duplicates_share_bytes() {
        let shared = object_with_syms(&["_core_x"]);
        let a_only = object_with_syms(&["_a"]);
        let b_only = object_with_syms(&["_b", "_core_x"]);
        let a = archive_of(&[("a.o", &a_only), ("std.o", &shared)]);
        let b = archive_of(&[("std.o", &shared), ("b.o", &b_only)]);
        let img = build(&[&a, &b]).unwrap();
        let v = Image::parse(&img).unwrap();
        assert_eq!(v.archive_count(), 2);
        let ma: Vec<_> = v.members(0).collect();
        let mb: Vec<_> = v.members(1).collect();
        assert_eq!(ma.len(), 2);
        assert_eq!(mb.len(), 2);
        assert_eq!(ma[0].name, "a.o");
        assert_eq!(ma[1].name, "std.o");
        assert_eq!(mb[0].name, "std.o");
        assert_eq!(mb[1].name, "b.o");
        assert_eq!(ma[1].data, shared.as_slice());
        assert_eq!(
            mb[0].data.as_ptr(),
            ma[1].data.as_ptr(),
            "one body for both slots"
        );
        assert_eq!(mb[1].data, b_only.as_slice());

        let syms: Vec<_> = v.symbols().collect();
        let names: Vec<&str> = syms.iter().map(|s| s.name).collect();
        assert_eq!(names, ["_a", "_b", "_core_x"], "sorted by name");
        let core_x = syms[2];
        assert_eq!(
            (core_x.archive_idx, core_x.member_idx),
            (0, 1),
            "first archive wins"
        );
        let b_sym = syms[1];
        assert_eq!(
            (b_sym.archive_idx, b_sym.member_idx, b_sym.n_value),
            (1, 1, 0)
        );

        // the copy of std.o in b wins no name, so no program pulls it
        let img = build_reachable(&[&a, &b]).unwrap();
        let v = Image::parse(&img).unwrap();
        let mb: Vec<&str> = v.members(1).map(|m| m.name).collect();
        assert_eq!(mb, ["b.o"]);
        let b_sym = v.symbols().find(|s| s.name == "_b").unwrap();
        assert_eq!((b_sym.archive_idx, b_sym.member_idx), (1, 0));
    }

    #[test]
    fn symbol_rows_are_found_by_name() {
        let shared = object_with_syms(&["_core_x"]);
        let a = archive_of(&[
            ("a.o", &object_with_syms(&["_a", "_aa"])),
            ("std.o", &shared),
        ]);
        let b = archive_of(&[
            ("std.o", &shared),
            ("b.o", &object_with_syms(&["_b", "_core_x", "_z"])),
        ]);
        let img = build(&[&a, &b]).unwrap();
        let v = Image::parse(&img).unwrap();
        let syms: Vec<_> = v.symbols().collect();
        assert_eq!(v.symbol_count(), syms.len());
        for (i, s) in syms.iter().enumerate() {
            assert_eq!(v.find_symbol(s.name), Some(i));
            assert_eq!(v.symbol_name(i), s.name.as_bytes());
            assert_eq!(v.symbol_owner(i), (s.archive_idx, s.member_idx));
        }
        for miss in ["", "_", "_ab", "_core", "_core_x0", "_zz", "~"] {
            assert_eq!(v.find_symbol(miss), None, "{miss}");
        }
    }

    #[test]
    fn same_name_different_bytes_stay_distinct() {
        let x = object_with_syms(&["_x"]);
        let y = object_with_syms(&["_y"]);
        let a = archive_of(&[("m.o", &x)]);
        let b = archive_of(&[("m.o", &y)]);
        let img = build(&[&a, &b]).unwrap();
        let v = Image::parse(&img).unwrap();
        assert_eq!(v.members(0).next().unwrap().data, x.as_slice());
        assert_eq!(v.members(1).next().unwrap().data, y.as_slice());
    }

    #[test]
    fn members_no_program_can_pull_are_left_out() {
        let rt = object_with(&["_torajs_f"], &["__ZN3std3barE"]);
        let std_o = object_with(&["__ZN3std3barE"], &["__ZN5gimli3fooE"]);
        let std_o2 = object_with(&["__ZN3std3bazE"], &[]);
        let gimli = object_with(&["__ZN5gimli3fooE"], &[]);
        let unused = object_with(&["__ZN5gimli3quxE"], &[]);
        let shim = object_with(&["_memcpy"], &[]);
        let a = archive_of(&[
            ("torajs_rt.o", &rt),
            ("std.o", &std_o),
            ("std2.o", &std_o2),
            ("gimli.o", &gimli),
            ("unused.o", &unused),
            ("shim.o", &shim),
        ]);
        // a second archive whose copies never win a name
        let b = archive_of(&[("std.o", &std_o), ("torajs_rt.o", &rt)]);
        let img = build_reachable(&[&a, &b]).unwrap();
        let v = Image::parse(&img).unwrap();
        let kept: Vec<&str> = v.members(0).map(|m| m.name).collect();
        assert_eq!(kept, ["torajs_rt.o", "std.o", "gimli.o", "shim.o"]);
        assert_eq!(
            v.members(1).count(),
            0,
            "a copy that wins nothing is never pulled"
        );
        let syms: Vec<_> = v.symbols().collect();
        let names: Vec<&str> = syms.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            ["__ZN3std3barE", "__ZN5gimli3fooE", "_memcpy", "_torajs_f"],
            "rows won by a dropped member leave the index"
        );
        let memcpy = syms[2];
        assert_eq!(
            (memcpy.archive_idx, memcpy.member_idx),
            (0, 3),
            "member indices count kept members only"
        );
    }

    #[test]
    fn rust_manglings_are_recognised() {
        use crate::prune::is_rust_mangled;
        assert!(is_rust_mangled("__ZN4core3fmt5write17h0123456789abcdefE"));
        assert!(is_rust_mangled("__RNvCs1234_4core5write"));
        assert!(!is_rust_mangled("___torajs_rc_inc"));
        assert!(!is_rust_mangled("_memcpy"));
        assert!(!is_rust_mangled("__ZNone"));
        assert!(!is_rust_mangled("___rust_alloc"));
    }

    #[test]
    fn parse_rejects_foreign_bytes() {
        assert_eq!(Image::parse(b"nope").unwrap_err(), ImageError::Truncated);
        let mut img = build(&[]).unwrap();
        img[0] = b'X';
        assert_eq!(Image::parse(&img).unwrap_err(), ImageError::BadMagic);
    }

    #[test]
    fn parse_rejects_an_image_of_another_version() {
        let mut img = build(&[]).unwrap();
        img[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(Image::parse(&img).unwrap_err(), ImageError::BadVersion(2));
    }

    #[test]
    fn a_table_is_built_once_per_body_and_read_back_per_slot() {
        let shared = object_with_syms(&["_core_x"]);
        let a = archive_of(&[("a.o", &object_with_syms(&["_a"])), ("std.o", &shared)]);
        let b = archive_of(&[("std.o", &shared), ("b.o", &object_with(&["_b"], &["_a"]))]);
        let parsed = [parse_archive(&a).unwrap(), parse_archive(&b).unwrap()];
        let mut calls: Vec<String> = Vec::new();
        let img = build_reachable_from_members_with_tables(&parsed, &mut |m, _| {
            calls.push(m.name.to_string());
            Ok(MemberTables {
                table: format!("{}:{}", m.name, m.data.len()).into_bytes(),
                cold: format!("cold {}", m.name).into_bytes(),
            })
        })
        .unwrap();
        // the copy of std.o in b wins no name and is left out
        assert_eq!(calls, ["a.o", "std.o", "b.o"]);
        let v = Image::parse(&img).unwrap();
        for a in 0..v.archive_count() {
            for (i, m) in v.members(a).enumerate() {
                let want = format!("{}:{}", m.name, m.data.len());
                assert_eq!(v.member_table(a, i), want.as_bytes());
                let cold = format!("cold {}", m.name);
                assert_eq!(v.member_cold_table(a, i), cold.as_bytes());
            }
        }
        let plain = build(&[&a, &b]).unwrap();
        let v = Image::parse(&plain).unwrap();
        assert!(v.member_table(1, 0).is_empty());
        assert!(v.member_cold_table(1, 0).is_empty());

        let err = build_reachable_from_members_with_tables(&parsed, &mut |m, _| {
            if m.name == "b.o" {
                Err("no".into())
            } else {
                Ok(MemberTables::default())
            }
        })
        .unwrap_err();
        assert!(
            matches!(err, BuildError::Table { archive_idx: 1, ref member, .. } if member == "b.o")
        );
    }

    #[test]
    fn a_table_sees_the_rows_the_index_gives_each_name() {
        let shared = object_with_syms(&["_core_x"]);
        let a = archive_of(&[("a.o", &object_with_syms(&["_a"])), ("std.o", &shared)]);
        let b = archive_of(&[
            ("std.o", &shared),
            ("b.o", &object_with(&["_b"], &["_a"])),
            ("unused.o", &object_with(&["__ZN5gimli3quxE"], &[])),
        ]);
        let parsed = [parse_archive(&a).unwrap(), parse_archive(&b).unwrap()];
        let probe = ["_a", "_b", "_core_x", "__ZN5gimli3quxE", "_none"];
        let mut seen: Vec<Option<u32>> = Vec::new();
        let img = build_reachable_from_members_with_tables(&parsed, &mut |_, rows| {
            seen = probe.iter().map(|n| rows.row(n)).collect();
            Ok(MemberTables::default())
        })
        .unwrap();
        let v = Image::parse(&img).unwrap();
        let want: Vec<Option<u32>> = probe
            .iter()
            .map(|n| v.find_symbol(n).map(|i| i as u32))
            .collect();
        assert_eq!(seen, want);
        assert_eq!(
            want[3], None,
            "a name only a dropped member wins has no row"
        );
    }

    #[test]
    fn build_is_deterministic() {
        let a = archive_of(&[("a.o", &object_with_syms(&["_a"]))]);
        assert_eq!(build(&[&a]).unwrap(), build(&[&a]).unwrap());
    }
}
