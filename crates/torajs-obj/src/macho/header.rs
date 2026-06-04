//! `mach_header_64` — fixed 32-byte file header that every Mach-O
//! 64-bit container starts with.
//!
//! Field-for-field with `<mach-o/loader.h>`:
//!
//! ```text
//!   offset  size  name           note
//!     0      4    magic          MH_MAGIC_64 = 0xFEEDFACF
//!     4      4    cputype        CPU_TYPE_ARM64 = 0x0100000C
//!     8      4    cpusubtype     CPU_SUBTYPE_ARM64_ALL = 0
//!    12      4    filetype       MH_OBJECT = 1 (for `.o`)
//!    16      4    ncmds          number of load commands
//!    20      4    sizeofcmds     total size of all load commands
//!    24      4    flags          plain `.o` = 0 (S1);
//!                                clang typically adds
//!                                MH_SUBSECTIONS_VIA_SYMBOLS = 0x2000
//!    28      4    reserved       64-bit form only; always 0
//! ```
//!
//! Total = 32 bytes, little-endian on aarch64 Apple Silicon.

use crate::write::u32_le;

/// `MH_MAGIC_64` — 64-bit Mach-O magic, little-endian host
/// (the on-disk byte order is `CF FA ED FE`).
pub const MH_MAGIC_64: u32 = 0xFEED_FACF;

/// `CPU_TYPE_ARM64` — `CPU_TYPE_ARM (12) | CPU_ARCH_ABI64 (1<<24)`.
/// Defined in `<mach/machine.h>`. We hard-code the result so the
/// constant table reads the same way `otool -h` prints it.
pub const CPU_TYPE_ARM64: u32 = 0x0100_000C;

/// `CPU_SUBTYPE_ARM64_ALL` — generic ARM64 subtype, what every
/// `aarch64-apple-darwin` `.o` produced by clang uses.
pub const CPU_SUBTYPE_ARM64_ALL: u32 = 0x0000_0000;

/// `MH_OBJECT` (filetype = 1) — relocatable object file, the form
/// torajs-obj emits. (Executables / dylibs are emitted by
/// torajs-link in #8, not here.)
pub const MH_OBJECT: u32 = 0x0000_0001;

/// In-memory representation of `mach_header_64`. Every field maps
/// 1:1 to a 4-byte slot in the on-disk layout. `write_to` is the
/// only way to serialize it — there's no `Default::default()` that
/// would silently produce an invalid (`magic = 0`) header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachHeader64 {
    pub magic: u32,
    pub cputype: u32,
    pub cpusubtype: u32,
    pub filetype: u32,
    pub ncmds: u32,
    pub sizeofcmds: u32,
    pub flags: u32,
    pub reserved: u32,
}

impl MachHeader64 {
    /// Fixed on-disk size of the struct — 32 bytes.
    pub const SIZE: usize = 32;

    /// Build the canonical `MH_OBJECT` header for an aarch64
    /// Apple-Silicon `.o` with no load commands and no flags yet.
    /// S2 patches `ncmds` / `sizeofcmds` once the load-command
    /// section is appended; S5 may turn on
    /// `MH_SUBSECTIONS_VIA_SYMBOLS` (0x2000) if we want clang-shaped
    /// output. The two zero-init slots are explicit so the reader
    /// sees what the on-disk bytes will hold.
    pub fn arm64_object() -> Self {
        Self {
            magic: MH_MAGIC_64,
            cputype: CPU_TYPE_ARM64,
            cpusubtype: CPU_SUBTYPE_ARM64_ALL,
            filetype: MH_OBJECT,
            ncmds: 0,
            sizeofcmds: 0,
            flags: 0,
            reserved: 0,
        }
    }

    /// Append the 32-byte little-endian on-disk form to `buf`.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        let start = buf.len();
        u32_le(buf, self.magic);
        u32_le(buf, self.cputype);
        u32_le(buf, self.cpusubtype);
        u32_le(buf, self.filetype);
        u32_le(buf, self.ncmds);
        u32_le(buf, self.sizeofcmds);
        u32_le(buf, self.flags);
        u32_le(buf, self.reserved);
        debug_assert_eq!(
            buf.len() - start,
            Self::SIZE,
            "mach_header_64 must serialize to exactly 32 bytes"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default `MH_OBJECT` header serializes to the exact 32-byte
    /// pattern `otool -h` decodes (annotated):
    ///
    /// ```text
    ///   CF FA ED FE   magic       = 0xFEEDFACF (MH_MAGIC_64)
    ///   0C 00 00 01   cputype     = 0x0100000C (CPU_TYPE_ARM64)
    ///   00 00 00 00   cpusubtype  = 0          (CPU_SUBTYPE_ARM64_ALL)
    ///   01 00 00 00   filetype    = 1          (MH_OBJECT)
    ///   00 00 00 00   ncmds       = 0
    ///   00 00 00 00   sizeofcmds  = 0
    ///   00 00 00 00   flags       = 0
    ///   00 00 00 00   reserved    = 0
    /// ```
    #[test]
    fn arm64_object_default_byte_equal() {
        let mut buf = Vec::new();
        MachHeader64::arm64_object().write_to(&mut buf);
        let expected: [u8; MachHeader64::SIZE] = [
            0xCF, 0xFA, 0xED, 0xFE, // magic
            0x0C, 0x00, 0x00, 0x01, // cputype
            0x00, 0x00, 0x00, 0x00, // cpusubtype
            0x01, 0x00, 0x00, 0x00, // filetype
            0x00, 0x00, 0x00, 0x00, // ncmds
            0x00, 0x00, 0x00, 0x00, // sizeofcmds
            0x00, 0x00, 0x00, 0x00, // flags
            0x00, 0x00, 0x00, 0x00, // reserved
        ];
        assert_eq!(
            buf.as_slice(),
            expected.as_slice(),
            "default arm64 MH_OBJECT header bytes diverge from spec layout"
        );
    }

    /// Patching `ncmds`, `sizeofcmds`, and `flags` (the three
    /// fields S2+ will mutate as load commands accrue) lands at the
    /// right byte offsets — proves the field order matches
    /// `<mach-o/loader.h>` slot-for-slot.
    #[test]
    fn mutated_header_writes_fields_at_correct_offsets() {
        let mut header = MachHeader64::arm64_object();
        header.ncmds = 2;
        header.sizeofcmds = 0x48; // e.g. two LC_SEGMENT_64 stubs
        header.flags = 0x2000; // MH_SUBSECTIONS_VIA_SYMBOLS

        let mut buf = Vec::new();
        header.write_to(&mut buf);

        // ncmds @ 16..20
        assert_eq!(&buf[16..20], &[0x02, 0x00, 0x00, 0x00]);
        // sizeofcmds @ 20..24
        assert_eq!(&buf[20..24], &[0x48, 0x00, 0x00, 0x00]);
        // flags @ 24..28
        assert_eq!(&buf[24..28], &[0x00, 0x20, 0x00, 0x00]);
        // reserved still 0 @ 28..32
        assert_eq!(&buf[28..32], &[0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn size_is_32_bytes() {
        let mut buf = Vec::new();
        MachHeader64::arm64_object().write_to(&mut buf);
        assert_eq!(buf.len(), MachHeader64::SIZE);
        assert_eq!(MachHeader64::SIZE, 32);
    }

    #[test]
    fn magic_constants_match_apple_loader_h() {
        // Sanity — these mirror the macros from `<mach-o/loader.h>`
        // and `<mach/machine.h>`; if any future refactor ever
        // touches the numeric literals, this test pins them.
        assert_eq!(MH_MAGIC_64, 0xFEED_FACF);
        assert_eq!(CPU_TYPE_ARM64, 0x0100_000C);
        assert_eq!(CPU_SUBTYPE_ARM64_ALL, 0);
        assert_eq!(MH_OBJECT, 1);
    }
}
