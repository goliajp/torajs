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
//!   archives  n_archives × { slot_start, slot_count }       (u32 ×2)
//!   slots     n_slots × member_id                           (u32)
//!   members   n_members × { name_off, name_len, data_off,
//!                            data_len }                     (u32 ×4)
//!   syms      n_syms × { name_off, name_len, archive_idx,
//!                        member_idx, n_value(u64) }         (24 B)
//!   names     member and symbol names, no terminators
//!   blob      distinct kept member bodies
//! ```
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

mod view;
pub use view::{Image, ImageError, ImageSymbol};

const MAGIC: &[u8; 8] = b"TRIMG\0\0\0";
const VERSION: u32 = 1;
const HEADER_LEN: usize = 8 + 4 * 13;
const MEMBER_LEN: usize = 16;
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
    /// A count or offset does not fit the image's 32-bit fields.
    TooLarge,
}

fn u32v(n: usize) -> Result<u32, BuildError> {
    u32::try_from(n).map_err(|_| BuildError::TooLarge)
}

/// Preprocess `archives` (in link search order) into an image holding
/// every member.
pub fn build(archives: &[&[u8]]) -> Result<Vec<u8>, BuildError> {
    build_members(&parse_all(archives)?, false)
}

/// [`build`] keeping only the members some program can pull into its
/// link; the names won by a dropped member leave the index, so a
/// lookup of one misses instead of pulling a different member.
pub fn build_reachable(archives: &[&[u8]]) -> Result<Vec<u8>, BuildError> {
    build_members(&parse_all(archives)?, true)
}

/// [`build_reachable`] over archives already split into members, in
/// link search order (members a caller rewrote included).
pub fn build_reachable_from_members(archives: &[Vec<ArMember<'_>>]) -> Result<Vec<u8>, BuildError> {
    build_members(archives, true)
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

fn build_members(
    parsed: &[Vec<ArMember<'_>>],
    reachable_only: bool,
) -> Result<Vec<u8>, BuildError> {
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

    let mut names = Names::default();
    let mut blob: Vec<u8> = Vec::new();
    let mut member_ids: HashMap<(&str, &[u8]), u32> = HashMap::new();
    // (name_off, name_len, data_off, data_len)
    let mut members: Vec<[u32; 4]> = Vec::new();
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
                    members.push([name_off, name_len, data_off, u32v(m.data.len())?]);
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

    let index: BTreeMap<&str, (u32, u32, u64)> = winners
        .into_iter()
        .filter_map(|(name, (a, m, v))| renumber[a as usize][m as usize].map(|k| (name, (a, k, v))))
        .collect();

    let mut syms: Vec<u8> = Vec::with_capacity(index.len() * SYM_LEN);
    for (name, (a, m, n_value)) in &index {
        let (name_off, name_len) = names.intern(name)?;
        for v in [name_off, name_len, *a, *m] {
            syms.extend_from_slice(&v.to_le_bytes());
        }
        syms.extend_from_slice(&n_value.to_le_bytes());
    }

    let archives_off = HEADER_LEN;
    let slots_off = archives_off + archive_table.len() * 8;
    let members_off = slots_off + slots.len() * 4;
    let syms_off = members_off + members.len() * MEMBER_LEN;
    let names_off = syms_off + syms.len();
    let names = names.bytes;
    let blob_off = align8(names_off + names.len());
    let total = blob_off + blob.len();

    let mut out: Vec<u8> = Vec::with_capacity(total);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    for n in [
        parsed.len(),
        slots.len(),
        members.len(),
        index.len(),
        archives_off,
        slots_off,
        members_off,
        syms_off,
        names_off,
        names.len(),
        blob_off,
        blob.len(),
    ] {
        out.extend_from_slice(&u32v(n)?.to_le_bytes());
    }
    debug_assert_eq!(out.len(), HEADER_LEN);
    for row in &archive_table {
        for v in row {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    for v in &slots {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for row in &members {
        for v in row {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out.extend_from_slice(&syms);
    out.extend_from_slice(&names);
    out.resize(blob_off, 0);
    out.extend_from_slice(&blob);
    Ok(out)
}

/// The names blob under construction, each distinct name written once.
#[derive(Default)]
struct Names<'a> {
    bytes: Vec<u8>,
    offs: HashMap<&'a str, (u32, u32)>,
}

impl<'a> Names<'a> {
    fn intern(&mut self, s: &'a str) -> Result<(u32, u32), BuildError> {
        if let Some(&r) = self.offs.get(s) {
            return Ok(r);
        }
        let r = (u32v(self.bytes.len())?, u32v(s.len())?);
        self.bytes.extend_from_slice(s.as_bytes());
        self.offs.insert(s, r);
        Ok(r)
    }
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
    fn build_is_deterministic() {
        let a = archive_of(&[("a.o", &object_with_syms(&["_a"]))]);
        assert_eq!(build(&[&a]).unwrap(), build(&[&a]).unwrap());
    }
}
