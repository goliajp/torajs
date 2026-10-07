//! Byte format of the prelinked runtime image: the whole runtime linked
//! once at a fixed layout, plus the tables a loader needs to place it
//! (rebase), bind it (imports, TLV descriptors) and resolve program
//! references into it (exports).
//!
//! Every integer is little-endian. Offsets inside table entries and
//! region `vm_off`s count from the image origin, the address
//! `preferred_base` names; table and template `file_off`s count from the
//! start of the blob. Text and data templates start on a
//! [`SEGMENT_ALIGN`] boundary of the blob.

pub const MAGIC: [u8; 8] = *b"TRLNK\0\0\0";
pub const VERSION: u32 = 1;
pub const HEADER_SIZE: usize = 136;
/// Header `u32` fields after magic, version, flags and `preferred_base`.
const N_WORDS: usize = 27;
pub const SEGMENT_ALIGN: u32 = 0x4000;
pub const IMPORT_ENTRY_SIZE: usize = 16;
pub const EXPORT_ENTRY_SIZE: usize = 12;

/// What fills an import slot at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportKind {
    /// A libSystem symbol, bound from the loading process.
    LibSystem = 0,
    /// A symbol of a dylib the loader opens on first use.
    LazyDylib = 1,
    /// A table the program being run provides (class layouts, fn names…).
    UserTable = 2,
}

impl ImportKind {
    fn from_u32(v: u32) -> Option<ImportKind> {
        [Self::LibSystem, Self::LazyDylib, Self::UserTable]
            .into_iter()
            .find(|k| *k as u32 == v)
    }
}

/// A mapped range. `file_size` bytes come from the blob at `file_off`;
/// the rest of `vm_size` is zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Region {
    pub vm_off: u32,
    pub file_off: u32,
    pub file_size: u32,
    pub vm_size: u32,
}

/// One table: `count` entries at blob offset `off`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Table {
    pub off: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Header {
    pub preferred_base: u64,
    /// Bytes from the origin to the end of the last region.
    pub vm_size: u32,
    /// R-X: member code, `__TEXT` constant sections, import stubs.
    pub text: Region,
    /// The `__TEXT` constant sections inside `text` (`vm_off`, `vm_size`).
    pub text_const: Region,
    /// RW: file-backed data template, then zero-fill.
    pub data: Region,
    /// Per-thread block: `vm_off` / `vm_size` span every thread-local
    /// section; `file_off` / `file_size` name the initialised part as an
    /// origin offset and length (copied from the loaded data region).
    pub tlv_template: Region,
    /// `u32` origin offsets of 8-byte absolute pointers to slide.
    pub rebase: Table,
    pub imports: Table,
    /// Sorted by name bytes.
    pub exports: Table,
    /// `u32` origin offsets of TLV descriptors (thunk, key, offset).
    pub tlv: Table,
    /// Name bytes the import and export entries point into.
    pub names: Table,
}

impl Header {
    fn words(&self) -> [u32; N_WORDS] {
        let r = |r: &Region| [r.vm_off, r.file_off, r.file_size, r.vm_size];
        let t = |t: &Table| [t.off, t.count];
        let mut w = [0u32; N_WORDS];
        w[0] = self.vm_size;
        w[1..5].copy_from_slice(&r(&self.text));
        w[5..9].copy_from_slice(&r(&self.text_const));
        w[9..13].copy_from_slice(&r(&self.data));
        w[13..17].copy_from_slice(&r(&self.tlv_template));
        for (i, tb) in [
            self.rebase,
            self.imports,
            self.exports,
            self.tlv,
            self.names,
        ]
        .iter()
        .enumerate()
        {
            w[17 + 2 * i..19 + 2 * i].copy_from_slice(&t(tb));
        }
        w
    }

    fn from_words(preferred_base: u64, w: &[u32; N_WORDS]) -> Header {
        let r = |i: usize| Region {
            vm_off: w[i],
            file_off: w[i + 1],
            file_size: w[i + 2],
            vm_size: w[i + 3],
        };
        let t = |i: usize| Table {
            off: w[17 + 2 * i],
            count: w[18 + 2 * i],
        };
        Header {
            preferred_base,
            vm_size: w[0],
            text: r(1),
            text_const: r(5),
            data: r(9),
            tlv_template: r(13),
            rebase: t(0),
            imports: t(1),
            exports: t(2),
            tlv: t(3),
            names: t(4),
        }
    }

    pub fn encode(&self) -> [u8; HEADER_SIZE] {
        let mut out = [0u8; HEADER_SIZE];
        out[..8].copy_from_slice(&MAGIC);
        out[8..12].copy_from_slice(&VERSION.to_le_bytes());
        out[16..24].copy_from_slice(&self.preferred_base.to_le_bytes());
        for (i, v) in self.words().iter().enumerate() {
            out[24 + 4 * i..28 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// An import as the reader returns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Import<'a> {
    pub kind: ImportKind,
    pub name: &'a str,
    /// Origin offset of the 8-byte slot to fill.
    pub slot_off: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FormatError {
    BadMagic,
    BadVersion(u32),
    /// A table or template runs past the end of the blob.
    OutOfBounds {
        what: &'static str,
    },
    BadImportKind(u32),
    NameNotUtf8,
}

/// A parsed view over an image blob.
#[derive(Debug, Clone, Copy)]
pub struct RtLinkedImage<'a> {
    pub bytes: &'a [u8],
    pub header: Header,
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

impl<'a> RtLinkedImage<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<RtLinkedImage<'a>, FormatError> {
        let head = bytes
            .get(..HEADER_SIZE)
            .ok_or(FormatError::OutOfBounds { what: "header" })?;
        if head[..8] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        let version = u32_at(head, 8);
        if version != VERSION {
            return Err(FormatError::BadVersion(version));
        }
        let base = u64::from_le_bytes(head[16..24].try_into().unwrap());
        let words: [u32; N_WORDS] = std::array::from_fn(|i| u32_at(head, 24 + 4 * i));
        let img = RtLinkedImage {
            bytes,
            header: Header::from_words(base, &words),
        };
        let h = &img.header;
        img.slice("rebase", h.rebase.off, h.rebase.count as usize * 4)?;
        img.slice(
            "imports",
            h.imports.off,
            h.imports.count as usize * IMPORT_ENTRY_SIZE,
        )?;
        img.slice(
            "exports",
            h.exports.off,
            h.exports.count as usize * EXPORT_ENTRY_SIZE,
        )?;
        img.slice("tlv", h.tlv.off, h.tlv.count as usize * 4)?;
        img.slice("names", h.names.off, h.names.count as usize)?;
        img.slice("text", h.text.file_off, h.text.file_size as usize)?;
        img.slice("data", h.data.file_off, h.data.file_size as usize)?;
        Ok(img)
    }

    fn slice(&self, what: &'static str, off: u32, len: usize) -> Result<&'a [u8], FormatError> {
        let off = off as usize;
        self.bytes
            .get(off..off + len)
            .ok_or(FormatError::OutOfBounds { what })
    }

    fn table(&self, t: Table, entry: usize) -> &'a [u8] {
        let off = t.off as usize;
        &self.bytes[off..off + t.count as usize * entry]
    }

    fn name(&self, off: u32, len: u32) -> Result<&'a str, FormatError> {
        let names = self.table(self.header.names, 1);
        let bytes = names
            .get(off as usize..(off + len) as usize)
            .ok_or(FormatError::OutOfBounds { what: "name" })?;
        std::str::from_utf8(bytes).map_err(|_| FormatError::NameNotUtf8)
    }

    pub fn text(&self) -> &'a [u8] {
        let t = self.header.text;
        &self.bytes[t.file_off as usize..][..t.file_size as usize]
    }

    pub fn data_template(&self) -> &'a [u8] {
        let d = self.header.data;
        &self.bytes[d.file_off as usize..][..d.file_size as usize]
    }

    pub fn rebases(&self) -> impl Iterator<Item = u32> + 'a {
        self.table(self.header.rebase, 4)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
    }

    pub fn tlv_descriptors(&self) -> impl Iterator<Item = u32> + 'a {
        self.table(self.header.tlv, 4)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
    }

    pub fn imports(&self) -> Result<Vec<Import<'a>>, FormatError> {
        self.table(self.header.imports, IMPORT_ENTRY_SIZE)
            .as_chunks::<IMPORT_ENTRY_SIZE>()
            .0
            .iter()
            .map(|e| {
                let kind = u32_at(e, 0);
                Ok(Import {
                    kind: ImportKind::from_u32(kind).ok_or(FormatError::BadImportKind(kind))?,
                    name: self.name(u32_at(e, 4), u32_at(e, 8))?,
                    slot_off: u32_at(e, 12),
                })
            })
            .collect()
    }

    /// Entry `i` of the export table: (name, origin offset).
    pub fn export_at(&self, i: usize) -> Result<(&'a str, u32), FormatError> {
        let e = &self.table(self.header.exports, EXPORT_ENTRY_SIZE)[i * EXPORT_ENTRY_SIZE..];
        Ok((self.name(u32_at(e, 0), u32_at(e, 4))?, u32_at(e, 8)))
    }

    /// Binary search of the sorted export table.
    pub fn export(&self, name: &str) -> Result<Option<u32>, FormatError> {
        let (mut lo, mut hi) = (0usize, self.header.exports.count as usize);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let (n, off) = self.export_at(mid)?;
            match n.as_bytes().cmp(name.as_bytes()) {
                std::cmp::Ordering::Equal => return Ok(Some(off)),
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
            }
        }
        Ok(None)
    }
}
