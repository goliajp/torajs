//! The read side of the runtime image: [`Image`] is a view over the
//! bytes [`super::build`] writes, every accessor a slice read.

use super::{HEADER_LEN, MAGIC, MEMBER_LEN, SYM_LEN, VERSION};
use crate::archive::ArMember;
use crate::member_name::MemberName;

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
    tables: &'a [u8],
    colds: &'a [u8],
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
        let (tables_off, tables_len, colds_off, colds_len) = (f(13), f(14), f(15), f(16));
        let ends = [
            archives_off + n_archives * 8,
            slots_off + n_slots * 4,
            members_off + n_members * MEMBER_LEN,
            syms_off + n_syms * SYM_LEN,
            names_off + names_len,
            blob_off + blob_len,
            tables_off + tables_len,
            colds_off + colds_len,
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
            tables: &bytes[tables_off..tables_off + tables_len],
            colds: &bytes[colds_off..colds_off + colds_len],
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
        (start..start + count).map(move |slot| self.slot_member(slot))
    }

    /// Member `member_idx` of archive `archive_idx`.
    pub fn member(&self, archive_idx: usize, member_idx: usize) -> ArMember<'a> {
        let start = self.u32_at(self.archives_off + archive_idx * 8);
        self.slot_member(start + member_idx)
    }

    /// The table stored beside member `member_idx` of archive
    /// `archive_idx`; empty when the image was built without one.
    pub fn member_table(&self, archive_idx: usize, member_idx: usize) -> &'a [u8] {
        let m = self.slot_row(archive_idx, member_idx);
        let off = self.u32_at(m + 16);
        let tables: &'a [u8] = self.tables;
        &tables[off..off + self.u32_at(m + 20)]
    }

    /// The cold table stored for member `member_idx` of archive
    /// `archive_idx`; empty when the image was built without one.
    pub fn member_cold_table(&self, archive_idx: usize, member_idx: usize) -> &'a [u8] {
        let m = self.slot_row(archive_idx, member_idx);
        let off = self.u32_at(m + 24);
        let colds: &'a [u8] = self.colds;
        &colds[off..off + self.u32_at(m + 28)]
    }

    /// The `members` row of a slot.
    fn slot_row(&self, archive_idx: usize, member_idx: usize) -> usize {
        let start = self.u32_at(self.archives_off + archive_idx * 8);
        let id = self.u32_at(self.slots_off + (start + member_idx) * 4);
        self.members_off + id * MEMBER_LEN
    }

    fn slot_member(&self, slot: usize) -> ArMember<'a> {
        let id = self.u32_at(self.slots_off + slot * 4);
        let m = self.members_off + id * MEMBER_LEN;
        let (name_off, name_len) = (self.u32_at(m), self.u32_at(m + 4));
        let names: &'a [u8] = self.names;
        let name = MemberName::from_image(&names[name_off..name_off + name_len]);
        let data_off = self.u32_at(m + 8);
        let data_len = self.u32_at(m + 12);
        let blob: &'a [u8] = self.blob;
        ArMember {
            name,
            data: &blob[data_off..data_off + data_len],
        }
    }

    /// The merged first-archive-wins index, sorted by name.
    pub fn symbols(&self) -> impl Iterator<Item = ImageSymbol<'a>> + '_ {
        (0..self.n_syms).map(move |i| self.symbol(i))
    }

    pub fn symbol_count(&self) -> usize {
        self.n_syms
    }

    /// Row `i` of the sorted index.
    pub fn symbol(&self, i: usize) -> ImageSymbol<'a> {
        let s = self.syms_off + i * SYM_LEN;
        let (archive_idx, member_idx) = self.symbol_owner(i);
        ImageSymbol {
            name: self.name_at(self.u32_at(s), self.u32_at(s + 4)),
            archive_idx,
            member_idx,
            n_value: u64::from_le_bytes(self.bytes[s + 16..s + 24].try_into().unwrap()),
        }
    }

    /// The name of row `i` as bytes, read without the UTF-8 check
    /// [`Self::symbol`] makes (the builder writes every name from a
    /// `&str`).
    pub fn symbol_name(&self, i: usize) -> &'a [u8] {
        let s = self.syms_off + i * SYM_LEN;
        let (off, len) = (self.u32_at(s), self.u32_at(s + 4));
        let names: &'a [u8] = self.names;
        &names[off..off + len]
    }

    /// `(archive_idx, member_idx)` of row `i`, without reading its name.
    pub fn symbol_owner(&self, i: usize) -> (usize, usize) {
        let s = self.syms_off + i * SYM_LEN;
        (self.u32_at(s + 8), self.u32_at(s + 12))
    }

    /// Row of `name` in the sorted index, by binary search over the
    /// name bytes.
    pub fn find_symbol(&self, name: &str) -> Option<usize> {
        let key = name.as_bytes();
        let (mut lo, mut hi) = (0, self.n_syms);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let s = self.syms_off + mid * SYM_LEN;
            let off = self.u32_at(s);
            match self.names[off..off + self.u32_at(s + 4)].cmp(key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }
}
