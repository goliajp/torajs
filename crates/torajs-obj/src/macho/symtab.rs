//! `LC_SYMTAB` load command + `nlist_64` symbol table + string
//! table.
//!
//! Field layout per `<mach-o/loader.h>` + `<mach-o/nlist.h>`:
//!
//! ```text
//!   struct symtab_command {           // 24 bytes total
//!       uint32_t cmd;        // LC_SYMTAB = 0x2
//!       uint32_t cmdsize;    // 24 (fixed)
//!       uint32_t symoff;     // file offset to nlist_64 table
//!       uint32_t nsyms;      // number of symbols
//!       uint32_t stroff;     // file offset to string table
//!       uint32_t strsize;    // size of string table in bytes
//!   };
//!
//!   struct nlist_64 {                 // 16 bytes each
//!       uint32_t n_strx;     // index into string table
//!       uint8_t  n_type;     // N_STAB | N_PEXT | N_TYPE | N_EXT
//!       uint8_t  n_sect;     // 1-based section index, or NO_SECT
//!       uint16_t n_desc;     // see <mach-o/stab.h>
//!       uint64_t n_value;    // address / offset / section-relative
//!   };
//! ```
//!
//! The string table is a flat byte buffer: byte 0 is always `\0`
//! (empty-string sentinel — Apple's convention is that the empty
//! symbol name at index 0 distinguishes "no name" from a real
//! one). Each symbol name is appended NUL-terminated; the `n_strx`
//! slot in `nlist_64` is the byte offset where that name starts.
//! S3 ships the builder; section-relative address resolution and
//! the final `symoff` / `stroff` patches happen when the
//! higher-level writer lays out the file (lands in S5).

use crate::write::{u32_le, u64_le};

/// Load-command tag for `LC_SYMTAB`. See `<mach-o/loader.h>`.
pub const LC_SYMTAB: u32 = 0x02;

/// On-disk size of `symtab_command` (always 24 — there is no
/// per-command variable payload).
pub const SYMTAB_COMMAND_SIZE: u32 = 24;

/// On-disk size of one `nlist_64` entry.
pub const NLIST_64_SIZE: u32 = 16;

/// `N_TYPE` mask — bits 0xe of `n_type`. The defined symbol types
/// (`N_UNDF`, `N_ABS`, `N_SECT`, `N_INDR`) live in this slot.
pub const N_TYPE: u8 = 0x0e;

/// `n_type` = N_SECT — symbol defined in the section indicated by
/// `n_sect`. The typical `nlist_64` for a `.text` function.
pub const N_SECT: u8 = 0x0e;

/// `n_type` = N_UNDF — undefined symbol (e.g. an external
/// function call site). `n_sect` is `NO_SECT` (0).
pub const N_UNDF: u8 = 0x00;

/// External-symbol bit — OR'd into `n_type` for symbols that are
/// visible to the linker.
pub const N_EXT: u8 = 0x01;

/// `n_desc` bit — no dead stripping for this symbol's atom.
/// rustc sets it for `#[used]` statics; a dead-strip closure must
/// root the covering atom. Mirrors
/// `<mach-o/loader.h>::N_NO_DEAD_STRIP`.
pub const N_NO_DEAD_STRIP: u16 = 0x0020;

/// `NO_SECT` — sentinel `n_sect` for undefined / absolute symbols.
pub const NO_SECT: u8 = 0x00;

/// Mirror of `symtab_command`. `symoff` / `stroff` / `strsize` are
/// patched by the higher-level writer once the byte layout of the
/// whole `.o` is decided. `nsyms` follows the symbol table the
/// builder accumulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymtabCommand {
    pub symoff: u32,
    pub nsyms: u32,
    pub stroff: u32,
    pub strsize: u32,
}

impl SymtabCommand {
    /// Build the canonical empty `LC_SYMTAB` skeleton — all four
    /// offset / size fields zero. The higher-level writer patches
    /// them once it knows the final byte layout.
    pub fn zeroed() -> Self {
        Self {
            symoff: 0,
            nsyms: 0,
            stroff: 0,
            strsize: 0,
        }
    }

    /// Append the 24-byte little-endian on-disk form to `buf`.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        let start = buf.len();
        u32_le(buf, LC_SYMTAB);
        u32_le(buf, SYMTAB_COMMAND_SIZE);
        u32_le(buf, self.symoff);
        u32_le(buf, self.nsyms);
        u32_le(buf, self.stroff);
        u32_le(buf, self.strsize);
        debug_assert_eq!(
            (buf.len() - start) as u32,
            SYMTAB_COMMAND_SIZE,
            "symtab_command must serialize to exactly 24 bytes"
        );
    }
}

/// Mirror of `nlist_64`. The whole 16-byte struct serializes
/// little-endian. `n_value` is section-relative for `N_SECT`-typed
/// symbols (= byte offset within the section indicated by
/// `n_sect`) and 0 for `N_UNDF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nlist64 {
    pub n_strx: u32,
    pub n_type: u8,
    pub n_sect: u8,
    pub n_desc: u16,
    pub n_value: u64,
}

impl Nlist64 {
    /// Build an external symbol defined in section `n_sect`
    /// (1-based) at section-relative offset `value`. Matches what
    /// `clang -c` emits for an `__attribute__((visibility("default")))`
    /// non-static function.
    pub fn defined_extern(n_strx: u32, n_sect: u8, value: u64) -> Self {
        Self {
            n_strx,
            n_type: N_SECT | N_EXT,
            n_sect,
            n_desc: 0,
            n_value: value,
        }
    }

    /// Build an undefined external symbol (e.g. a runtime call
    /// site that will be resolved by the linker against a
    /// staticlib).
    pub fn undefined_extern(n_strx: u32) -> Self {
        Self {
            n_strx,
            n_type: N_UNDF | N_EXT,
            n_sect: NO_SECT,
            n_desc: 0,
            n_value: 0,
        }
    }

    /// Append the 16-byte little-endian on-disk form to `buf`.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        let start = buf.len();
        u32_le(buf, self.n_strx);
        buf.push(self.n_type);
        buf.push(self.n_sect);
        buf.extend_from_slice(&self.n_desc.to_le_bytes());
        u64_le(buf, self.n_value);
        debug_assert_eq!(
            (buf.len() - start) as u32,
            NLIST_64_SIZE,
            "nlist_64 must serialize to exactly 16 bytes"
        );
    }
}

/// Mach-O string-table builder. Apple's convention pre-populates
/// the table with a single `\0` so index 0 holds the empty string
/// sentinel; every real name lives at a non-zero index. `add`
/// returns the byte offset of the name's first character so the
/// caller can stash it in `nlist_64.n_strx`.
///
/// No interning — duplicate names get duplicate entries. The
/// linker doesn't care, and #7's caller stitches one symbol at a
/// time so the dup-name case shouldn't fire in practice.
#[derive(Debug, Clone, Default)]
pub struct StringTable {
    bytes: Vec<u8>,
}

impl StringTable {
    /// Build an empty string table with the sentinel `\0` already
    /// installed at byte 0.
    pub fn new() -> Self {
        Self { bytes: vec![0u8] }
    }

    /// Append `name` (NUL-terminated) and return its starting
    /// byte offset.
    pub fn add(&mut self, name: &str) -> u32 {
        let offset = self.bytes.len() as u32;
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes.push(0);
        offset
    }

    /// Raw byte length — what goes into `symtab_command.strsize`.
    pub fn len(&self) -> u32 {
        self.bytes.len() as u32
    }

    /// `true` only when the table holds nothing but the sentinel
    /// `\0` (no real entries).
    pub fn is_empty(&self) -> bool {
        self.bytes.len() <= 1
    }

    /// Borrow the raw byte buffer for writing to the output stream.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symtab_command_zeroed_writes_24_bytes() {
        let cmd = SymtabCommand::zeroed();
        let mut buf = Vec::new();
        cmd.write_to(&mut buf);
        assert_eq!(buf.len(), SYMTAB_COMMAND_SIZE as usize);
        // cmd @ 0..4 = LC_SYMTAB = 0x2
        assert_eq!(&buf[0..4], &[0x02, 0x00, 0x00, 0x00]);
        // cmdsize @ 4..8 = 24 = 0x18
        assert_eq!(&buf[4..8], &[0x18, 0x00, 0x00, 0x00]);
        // remaining four fields = 0
        assert_eq!(&buf[8..24], &[0u8; 16]);
    }

    #[test]
    fn symtab_command_patched_fields_land_at_correct_offsets() {
        let cmd = SymtabCommand {
            symoff: 0x200,
            nsyms: 3,
            stroff: 0x230,
            strsize: 0x40,
        };
        let mut buf = Vec::new();
        cmd.write_to(&mut buf);
        assert_eq!(&buf[8..12], &[0x00, 0x02, 0x00, 0x00]); // symoff
        assert_eq!(&buf[12..16], &[0x03, 0x00, 0x00, 0x00]); // nsyms
        assert_eq!(&buf[16..20], &[0x30, 0x02, 0x00, 0x00]); // stroff
        assert_eq!(&buf[20..24], &[0x40, 0x00, 0x00, 0x00]); // strsize
    }

    #[test]
    fn nlist64_defined_extern_writes_16_bytes_with_correct_type_bits() {
        // _foo at section 1, offset 0
        let entry = Nlist64::defined_extern(0x01, 1, 0);
        let mut buf = Vec::new();
        entry.write_to(&mut buf);
        assert_eq!(buf.len(), NLIST_64_SIZE as usize);
        // n_strx @ 0..4 = 1
        assert_eq!(&buf[0..4], &[0x01, 0x00, 0x00, 0x00]);
        // n_type @ 4 = N_SECT | N_EXT = 0x0e | 0x01 = 0x0F
        assert_eq!(buf[4], 0x0F);
        // n_sect @ 5 = 1
        assert_eq!(buf[5], 0x01);
        // n_desc @ 6..8 = 0
        assert_eq!(&buf[6..8], &[0x00, 0x00]);
        // n_value @ 8..16 = 0
        assert_eq!(&buf[8..16], &[0u8; 8]);
    }

    #[test]
    fn nlist64_undefined_extern_uses_n_undf_no_sect() {
        // External undefined sym (e.g. _malloc reference)
        let entry = Nlist64::undefined_extern(0x10);
        let mut buf = Vec::new();
        entry.write_to(&mut buf);
        // n_strx @ 0..4 = 0x10
        assert_eq!(&buf[0..4], &[0x10, 0x00, 0x00, 0x00]);
        // n_type @ 4 = N_UNDF | N_EXT = 0x0 | 0x01 = 0x01
        assert_eq!(buf[4], N_EXT);
        // n_sect @ 5 = NO_SECT = 0
        assert_eq!(buf[5], NO_SECT);
        // n_value @ 8..16 = 0
        assert_eq!(&buf[8..16], &[0u8; 8]);
    }

    #[test]
    fn nlist64_value_lands_at_correct_offset() {
        // Defined sym at offset 0x100 in section 1.
        let entry = Nlist64::defined_extern(0x05, 1, 0x100);
        let mut buf = Vec::new();
        entry.write_to(&mut buf);
        // n_value @ 8..16 = 0x100 LE
        assert_eq!(&buf[8..16], &[0x00, 0x01, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn string_table_sentinel_at_zero() {
        let st = StringTable::new();
        assert_eq!(st.len(), 1);
        assert!(st.is_empty());
        assert_eq!(st.as_bytes(), &[0u8]);
    }

    #[test]
    fn string_table_add_returns_offset_and_nul_terminates() {
        let mut st = StringTable::new();
        let foo = st.add("_foo");
        let bar = st.add("_bar");

        // First real entry starts at offset 1 (after sentinel)
        assert_eq!(foo, 1);
        // "_bar" follows: 1 (offset) + 4 ("_foo") + 1 (NUL) = 6
        assert_eq!(bar, 6);
        // strsize = sentinel + "_foo\0" + "_bar\0" = 1 + 5 + 5 = 11
        assert_eq!(st.len(), 11);
        assert!(!st.is_empty());
        // Byte layout: \0 _ f o o \0 _ b a r \0
        assert_eq!(
            st.as_bytes(),
            &[0u8, b'_', b'f', b'o', b'o', 0, b'_', b'b', b'a', b'r', 0]
        );
    }

    #[test]
    fn string_table_duplicate_names_allowed() {
        let mut st = StringTable::new();
        let a = st.add("_x");
        let b = st.add("_x");
        // No interning — different offsets.
        assert_ne!(a, b);
        assert_eq!(a, 1);
        assert_eq!(b, 4); // 1 + "_x\0" = 4
    }

    #[test]
    fn struct_sizes_match_apple_nlist_h() {
        assert_eq!(LC_SYMTAB, 0x02);
        assert_eq!(SYMTAB_COMMAND_SIZE, 24);
        assert_eq!(NLIST_64_SIZE, 16);
        assert_eq!(N_TYPE, 0x0e);
        assert_eq!(N_SECT, 0x0e);
        assert_eq!(N_UNDF, 0x00);
        assert_eq!(N_EXT, 0x01);
        assert_eq!(NO_SECT, 0x00);
    }
}
