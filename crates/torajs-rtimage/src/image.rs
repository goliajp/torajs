//! The runtime image: every `libtorajs_*.a` the compiler links,
//! preprocessed once when `tr` itself is built.
//!
//! What a `tr build` used to do per run — parse 37 archives, walk
//! 816 members' symbol tables (281k nlist rows), merge them first-
//! archive-wins into one index — the builder does here, and the
//! reader hands the result back as slices into the image bytes with
//! no parsing beyond the fixed header. Members are deduplicated by
//! `(name, bytes)`: each archive bundles its own copy of `core` /
//! `alloc` / `std`, so 816 members are 53 distinct ones and 62 MB of
//! archive is 4.4 MB of image.
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
//!   blob      distinct member bodies
//! ```
//!
//! A slot is one `(archive, member)` position of the original walk;
//! the slot table keeps that indexing intact so the linker's keys,
//! member ordering and first-wins resolution are unchanged. `syms` is
//! sorted by name bytes, which is the order a `BTreeMap<&str, _>`
//! bulk-builds from.

use std::collections::{BTreeMap, HashMap};

use crate::archive::{
    ArMember, ArParseError, ArchiveParseError, build_archive_index, parse_archive,
};

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

/// Preprocess `archives` (in link search order) into an image.
pub fn build(archives: &[&[u8]]) -> Result<Vec<u8>, BuildError> {
    let mut names = Names::default();

    let mut blob: Vec<u8> = Vec::new();
    let mut member_ids: HashMap<(&str, &[u8]), u32> = HashMap::new();
    // (name_off, name_len, data_off, data_len)
    let mut members: Vec<[u32; 4]> = Vec::new();
    let mut archive_table: Vec<[u32; 2]> = Vec::new();
    let mut slots: Vec<u32> = Vec::new();
    let mut index: BTreeMap<&str, (u32, u32, u64)> = BTreeMap::new();

    for (archive_idx, bytes) in archives.iter().enumerate() {
        let parsed = parse_archive(bytes).map_err(|err| BuildError::Ar { archive_idx, err })?;
        let per_archive =
            build_archive_index(&parsed).map_err(|err| BuildError::Member { archive_idx, err })?;
        for (name, sym) in per_archive {
            index
                .entry(name)
                .or_insert((u32v(archive_idx)?, u32v(sym.member_idx)?, sym.n_value));
        }
        archive_table.push([u32v(slots.len())?, u32v(parsed.len())?]);
        for m in &parsed {
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
    }

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
        archives.len(),
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

/// Why image bytes were refused by [`Image::parse`].
#[derive(Debug, PartialEq, Eq)]
pub enum ImageError {
    BadMagic,
    BadVersion(u32),
    Truncated,
}

/// One merged symbol: the first archive defining the name, the first
/// member of that archive defining it, and its `n_value` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSymbol<'a> {
    pub name: &'a str,
    pub archive_idx: usize,
    pub member_idx: usize,
    pub n_value: u64,
}

/// A view over image bytes. Construction checks the header and the
/// table extents; every accessor is a slice read.
#[derive(Debug, Clone, Copy)]
pub struct Image<'a> {
    bytes: &'a [u8],
    n_archives: usize,
    n_syms: usize,
    archives_off: usize,
    slots_off: usize,
    members_off: usize,
    syms_off: usize,
    names: &'a [u8],
    blob: &'a [u8],
}

impl<'a> Image<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Image<'a>, ImageError> {
        if bytes.len() < HEADER_LEN {
            return Err(ImageError::Truncated);
        }
        if &bytes[..8] != MAGIC {
            return Err(ImageError::BadMagic);
        }
        let f = |i: usize| {
            u32::from_le_bytes(bytes[8 + 4 * i..12 + 4 * i].try_into().unwrap()) as usize
        };
        let version = f(0);
        if version != VERSION as usize {
            return Err(ImageError::BadVersion(version as u32));
        }
        let (n_archives, n_slots, n_members, n_syms) = (f(1), f(2), f(3), f(4));
        let (archives_off, slots_off, members_off, syms_off) = (f(5), f(6), f(7), f(8));
        let (names_off, names_len, blob_off, blob_len) = (f(9), f(10), f(11), f(12));
        let ends = [
            archives_off + n_archives * 8,
            slots_off + n_slots * 4,
            members_off + n_members * MEMBER_LEN,
            syms_off + n_syms * SYM_LEN,
            names_off + names_len,
            blob_off + blob_len,
        ];
        if ends.iter().any(|&e| e > bytes.len()) {
            return Err(ImageError::Truncated);
        }
        Ok(Image {
            bytes,
            n_archives,
            n_syms,
            archives_off,
            slots_off,
            members_off,
            syms_off,
            names: &bytes[names_off..names_off + names_len],
            blob: &bytes[blob_off..blob_off + blob_len],
        })
    }

    fn u32_at(&self, off: usize) -> usize {
        u32::from_le_bytes(self.bytes[off..off + 4].try_into().unwrap()) as usize
    }

    fn name_at(&self, off: usize, len: usize) -> &'a str {
        let names: &'a [u8] = self.names;
        // the builder wrote every name from a `&str`
        std::str::from_utf8(&names[off..off + len]).unwrap()
    }

    pub fn archive_count(&self) -> usize {
        self.n_archives
    }

    /// The members of archive `archive_idx`, in archive order, each a
    /// slice into the image (duplicates share bytes).
    pub fn members(&self, archive_idx: usize) -> impl Iterator<Item = ArMember<'a>> + '_ {
        let row = self.archives_off + archive_idx * 8;
        let start = self.u32_at(row);
        let count = self.u32_at(row + 4);
        let blob: &'a [u8] = self.blob;
        (start..start + count).map(move |slot| {
            let id = self.u32_at(self.slots_off + slot * 4);
            let m = self.members_off + id * MEMBER_LEN;
            let name = self.name_at(self.u32_at(m), self.u32_at(m + 4));
            let data_off = self.u32_at(m + 8);
            let data_len = self.u32_at(m + 12);
            ArMember {
                name,
                data: &blob[data_off..data_off + data_len],
            }
        })
    }

    /// The merged first-archive-wins index, sorted by name.
    pub fn symbols(&self) -> impl Iterator<Item = ImageSymbol<'a>> + '_ {
        (0..self.n_syms).map(move |i| {
            let s = self.syms_off + i * SYM_LEN;
            ImageSymbol {
                name: self.name_at(self.u32_at(s), self.u32_at(s + 4)),
                archive_idx: self.u32_at(s + 8),
                member_idx: self.u32_at(s + 12),
                n_value: u64::from_le_bytes(self.bytes[s + 16..s + 24].try_into().unwrap()),
            }
        })
    }
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
        let mut strtab: Vec<u8> = vec![0];
        let mut nlist: Vec<u8> = Vec::new();
        for (i, s) in syms.iter().enumerate() {
            nlist.extend_from_slice(&(strtab.len() as u32).to_le_bytes());
            nlist.push(0x0f); // N_SECT | N_EXT
            nlist.push(1);
            nlist.extend_from_slice(&0u16.to_le_bytes());
            nlist.extend_from_slice(&((i * 4) as u64).to_le_bytes());
            strtab.extend_from_slice(s.as_bytes());
            strtab.push(0);
        }
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
        o.extend_from_slice(&(syms.len() as u32).to_le_bytes());
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
