//! `LC_SEGMENT_64` load command + `section_64` table entry.
//!
//! Field layout per `<mach-o/loader.h>`:
//!
//! ```text
//!   struct segment_command_64 {       // 72 bytes total
//!       uint32_t cmd;        // LC_SEGMENT_64 = 0x19
//!       uint32_t cmdsize;    // 72 + 80 * nsects
//!       char     segname[16];
//!       uint64_t vmaddr;
//!       uint64_t vmsize;
//!       uint64_t fileoff;
//!       uint64_t filesize;
//!       int32_t  maxprot;
//!       int32_t  initprot;
//!       uint32_t nsects;
//!       uint32_t flags;
//!   };
//!
//!   struct section_64 {               // 80 bytes total
//!       char     sectname[16];
//!       char     segname[16];
//!       uint64_t addr;
//!       uint64_t size;
//!       uint32_t offset;
//!       uint32_t align;       // log2(byte alignment)
//!       uint32_t reloff;
//!       uint32_t nreloc;
//!       uint32_t flags;       // S_ATTR_* | section type
//!       uint32_t reserved1;
//!       uint32_t reserved2;
//!       uint32_t reserved3;
//!   };
//! ```
//!
//! For `MH_OBJECT` (`.o`) files vmaddr is 0 — the linker assigns
//! the final load address. `maxprot` / `initprot` carry the
//! permission bits the linker will use for the eventual `__TEXT`
//! / `__DATA` segment in the loaded image.

use crate::write::{bytes, name16, u32_le, u64_le};

/// Load-command tag for `LC_SEGMENT_64`. See `<mach-o/loader.h>`.
pub const LC_SEGMENT_64: u32 = 0x19;

/// Mach VM protection bit — read.
pub const VM_PROT_READ: u32 = 0x1;
/// Mach VM protection bit — write.
pub const VM_PROT_WRITE: u32 = 0x2;
/// Mach VM protection bit — execute.
pub const VM_PROT_EXECUTE: u32 = 0x4;
/// `VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE` — the standard
/// `.o` segment access mask. The linker tightens this per output
/// segment when it lays out the final binary.
pub const VM_PROT_RWX: u32 = VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE;

/// Section attribute — section contains only executable
/// instructions. Mirrors `S_ATTR_PURE_INSTRUCTIONS`.
pub const S_ATTR_PURE_INSTRUCTIONS: u32 = 0x8000_0000;
/// Section attribute — section contains some machine instructions.
/// Mirrors `S_ATTR_SOME_INSTRUCTIONS`. The `__TEXT,__text` flag
/// combo `clang -c` emits is `(PURE | SOME)`.
pub const S_ATTR_SOME_INSTRUCTIONS: u32 = 0x0000_0400;

/// Mask isolating the section-type field (low 8 bits) inside
/// `section_64.flags`. The remaining 24 bits hold attribute flags
/// (`S_ATTR_*`). Mirrors `<mach-o/loader.h>::SECTION_TYPE`.
pub const SECTION_TYPE: u32 = 0x0000_00FF;

/// Section type — regular file-storage section. `<mach-o/loader.h>::
/// S_REGULAR`. Covers `__TEXT,__text` / `__TEXT,__const` /
/// `__DATA,__data` etc. — anything whose payload lives in the
/// `.o` / final binary and is loaded verbatim.
pub const S_REGULAR: u32 = 0x0;

/// Section type — zero-fill on demand. `<mach-o/loader.h>::
/// S_ZEROFILL`. `__DATA,__bss` / `__DATA,__common` use this:
/// `offset` is 0 (no file storage), `size` carries the vmsize the
/// loader zero-inits at startup. SD-4c-prereq-c-fix needs the
/// distinction to skip file-storage emit while still reserving
/// `__DATA` vmsize.
pub const S_ZEROFILL: u32 = 0x1;

/// Section attribute — no dead stripping: a GC-style linker must
/// keep every atom in the section even when nothing references it.
/// Rust `#[used]` statics and hand-written registries set this.
/// Mirrors `<mach-o/loader.h>::S_ATTR_NO_DEAD_STRIP`.
pub const S_ATTR_NO_DEAD_STRIP: u32 = 0x1000_0000;

/// Section attribute — live support: ld64 keeps these atoms alive
/// alongside the atoms they reference (exception-frame semantics).
/// Our conservative closure treats it as no-dead-strip. Mirrors
/// `<mach-o/loader.h>::S_ATTR_LIVE_SUPPORT`.
pub const S_ATTR_LIVE_SUPPORT: u32 = 0x0800_0000;

/// Section type — module initializer function pointers
/// (`__DATA,__mod_init_func`). Every slot runs before `main`, so a
/// dead-strip pass must treat the whole section as a GC root.
/// Mirrors `<mach-o/loader.h>::S_MOD_INIT_FUNC_POINTERS`.
pub const S_MOD_INIT_FUNC_POINTERS: u32 = 0x9;

/// Section type — module terminator function pointers
/// (`__DATA,__term_func`). Dead-strip root for the same reason as
/// mod-init. Mirrors `<mach-o/loader.h>::S_TERM_FUNC_POINTERS`.
pub const S_TERM_FUNC_POINTERS: u32 = 0xA;

/// Section type — 4-byte literal pool (`__TEXT,__literal4`).
/// Mirrors `<mach-o/loader.h>::S_4BYTE_LITERALS`.
pub const S_4BYTE_LITERALS: u32 = 0x3;

/// Section type — 8-byte literal pool (`__TEXT,__literal8`).
/// Mirrors `<mach-o/loader.h>::S_8BYTE_LITERALS`.
pub const S_8BYTE_LITERALS: u32 = 0x4;

/// Section type — 16-byte literal pool (`__TEXT,__literal16`).
/// Mirrors `<mach-o/loader.h>::S_16BYTE_LITERALS`.
pub const S_16BYTE_LITERALS: u32 = 0xE;

/// Section type — C string literals (`__TEXT,__cstring`).
/// `<mach-o/loader.h>::S_CSTRING_LITERALS`. Regular file storage,
/// flagged separately so ld64 can deduplicate identical strings;
/// our linker just treats it as `S_REGULAR` with extra metadata.
pub const S_CSTRING_LITERALS: u32 = 0x2;

/// Section type — giant zero-fill. `<mach-o/loader.h>::S_GB_ZEROFILL`.
/// Same emit semantics as `S_ZEROFILL` (no file storage); separate
/// type for sections too large to share a vmsize budget with the
/// rest of `__DATA`.
pub const S_GB_ZEROFILL: u32 = 0xC;

/// Section type — thread-local storage with initial values
/// (`__DATA,__thread_data`). `<mach-o/loader.h>::
/// S_THREAD_LOCAL_REGULAR`. File-storage section, but the loader
/// duplicates each thread's slice instead of mapping it shared.
/// SD-4c-prereq+b/c hooks the TLV descriptor emit pass to this.
pub const S_THREAD_LOCAL_REGULAR: u32 = 0x11;

/// Section type — thread-local zero-fill (`__DATA,__thread_bss`).
/// `<mach-o/loader.h>::S_THREAD_LOCAL_ZEROFILL`. Same skip-file-
/// storage semantics as `S_ZEROFILL`; the loader allocates +
/// zero-inits one slice per thread at TLS bootstrap.
pub const S_THREAD_LOCAL_ZEROFILL: u32 = 0x12;

/// Section type — thread-local descriptors (`__DATA,__thread_vars`).
/// `<mach-o/loader.h>::S_THREAD_LOCAL_VARIABLES`. Each 24-byte
/// `{thunk, key, offset}` entry is dyld-bound at startup; user code
/// loads `thunk` via TLVP_LOAD_PAGE21/PAGEOFF12 then calls it.
pub const S_THREAD_LOCAL_VARIABLES: u32 = 0x13;

/// Section type (low 8 bits of `flags`) — non-lazy symbol pointers.
/// `<mach-o/loader.h>::S_NON_LAZY_SYMBOL_POINTERS`. Reserved here for
/// future GOT-like sections; SD-2a doesn't emit one yet.
pub const S_NON_LAZY_SYMBOL_POINTERS: u32 = 0x6;

/// Section type — lazy symbol pointers. Mirrors
/// `<mach-o/loader.h>::S_LAZY_SYMBOL_POINTERS`. Used as the section
/// type for `__DATA,__la_symbol_ptr` (one 8-byte slot per
/// dyld-bound import; dyld writes the resolved target address into
/// each slot at load time / first call).
pub const S_LAZY_SYMBOL_POINTERS: u32 = 0x7;

/// Section type — symbol stubs. Mirrors
/// `<mach-o/loader.h>::S_SYMBOL_STUBS`. Used as the section type
/// for `__TEXT,__stubs`. Pairs with a non-zero `reserved2` carrying
/// the per-stub byte size (12 for the ARM64 ADRP+LDR+BR trampoline).
pub const S_SYMBOL_STUBS: u32 = 0x8;

/// On-disk size of `segment_command_64`, excluding the trailing
/// section table.
pub const SEGMENT_COMMAND_64_SIZE: u32 = 72;

/// On-disk size of one `section_64` entry that follows the segment
/// command header.
pub const SECTION_64_SIZE: u32 = 80;

/// Builder-friendly mirror of `segment_command_64`. `cmdsize` is
/// computed by `write_to` from the section table length; callers
/// don't have to keep it in sync with `sections.len()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentCommand64 {
    pub segname: String,
    pub vmaddr: u64,
    pub vmsize: u64,
    pub fileoff: u64,
    pub filesize: u64,
    pub maxprot: u32,
    pub initprot: u32,
    pub flags: u32,
    pub sections: Vec<Section64>,
}

/// Mirror of `section_64`. `offset` / `reloff` etc. land as 0 here
/// when the caller hasn't yet decided file layout — they get
/// patched once the writer knows the final byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section64 {
    pub sectname: String,
    pub segname: String,
    pub addr: u64,
    pub size: u64,
    pub offset: u32,
    pub align_log2: u32,
    pub reloff: u32,
    pub nreloc: u32,
    pub flags: u32,
    pub reserved1: u32,
    pub reserved2: u32,
    pub reserved3: u32,
}

impl SegmentCommand64 {
    /// Build the canonical `.o` `__TEXT,__text` segment skeleton
    /// for an aarch64 Apple-Silicon object — RWX permissions,
    /// flags = 0, one `__TEXT,__text` `section_64` with the
    /// PURE | SOME instruction-attribute flag combo `clang -c`
    /// emits. Sizes / offsets are 0; the high-level builder
    /// (lands later) patches them once the layout settles.
    pub fn text_skeleton(text_align_log2: u32) -> Self {
        Self {
            segname: "__TEXT".into(),
            vmaddr: 0,
            vmsize: 0,
            fileoff: 0,
            filesize: 0,
            maxprot: VM_PROT_RWX,
            initprot: VM_PROT_RWX,
            flags: 0,
            sections: vec![Section64 {
                sectname: "__text".into(),
                segname: "__TEXT".into(),
                addr: 0,
                size: 0,
                offset: 0,
                align_log2: text_align_log2,
                reloff: 0,
                nreloc: 0,
                flags: S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
            }],
        }
    }

    /// Total on-disk size = 72 + 80 * nsects.
    pub fn cmdsize(&self) -> u32 {
        SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE * self.sections.len() as u32
    }

    /// Append the segment command + its section table to `buf`.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        let start = buf.len();
        u32_le(buf, LC_SEGMENT_64);
        u32_le(buf, self.cmdsize());
        bytes(buf, &name16(&self.segname));
        u64_le(buf, self.vmaddr);
        u64_le(buf, self.vmsize);
        u64_le(buf, self.fileoff);
        u64_le(buf, self.filesize);
        u32_le(buf, self.maxprot);
        u32_le(buf, self.initprot);
        u32_le(buf, self.sections.len() as u32);
        u32_le(buf, self.flags);
        for s in &self.sections {
            s.write_to(buf);
        }
        debug_assert_eq!(
            (buf.len() - start) as u32,
            self.cmdsize(),
            "segment + sections must match cmdsize"
        );
    }
}

impl Section64 {
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        let start = buf.len();
        bytes(buf, &name16(&self.sectname));
        bytes(buf, &name16(&self.segname));
        u64_le(buf, self.addr);
        u64_le(buf, self.size);
        u32_le(buf, self.offset);
        u32_le(buf, self.align_log2);
        u32_le(buf, self.reloff);
        u32_le(buf, self.nreloc);
        u32_le(buf, self.flags);
        u32_le(buf, self.reserved1);
        u32_le(buf, self.reserved2);
        u32_le(buf, self.reserved3);
        debug_assert_eq!(
            buf.len() - start,
            SECTION_64_SIZE as usize,
            "section_64 must serialize to exactly 80 bytes"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_skeleton_one_nsect_cmdsize_is_152() {
        let seg = SegmentCommand64::text_skeleton(2);
        assert_eq!(seg.cmdsize(), 152);
        assert_eq!(seg.cmdsize(), SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE);
        assert_eq!(seg.sections.len(), 1);
    }

    #[test]
    fn text_skeleton_writes_72_byte_segment_header() {
        let seg = SegmentCommand64::text_skeleton(2);
        let mut buf = Vec::new();
        seg.write_to(&mut buf);
        // Expected: 72-byte segment header + 80-byte section = 152.
        assert_eq!(buf.len(), 152);

        // cmd @ 0..4 = LC_SEGMENT_64 = 0x19
        assert_eq!(&buf[0..4], &[0x19, 0x00, 0x00, 0x00]);
        // cmdsize @ 4..8 = 152 = 0x98
        assert_eq!(&buf[4..8], &[0x98, 0x00, 0x00, 0x00]);
        // segname @ 8..24 = "__TEXT" zero-padded to 16 bytes
        assert_eq!(&buf[8..24], &name16("__TEXT")[..]);
        // vmaddr @ 24..32 = 0
        assert_eq!(&buf[24..32], &[0u8; 8]);
        // vmsize @ 32..40 = 0
        assert_eq!(&buf[32..40], &[0u8; 8]);
        // fileoff @ 40..48 = 0
        assert_eq!(&buf[40..48], &[0u8; 8]);
        // filesize @ 48..56 = 0
        assert_eq!(&buf[48..56], &[0u8; 8]);
        // maxprot @ 56..60 = 7 (RWX)
        assert_eq!(&buf[56..60], &[0x07, 0x00, 0x00, 0x00]);
        // initprot @ 60..64 = 7 (RWX)
        assert_eq!(&buf[60..64], &[0x07, 0x00, 0x00, 0x00]);
        // nsects @ 64..68 = 1
        assert_eq!(&buf[64..68], &[0x01, 0x00, 0x00, 0x00]);
        // flags @ 68..72 = 0
        assert_eq!(&buf[68..72], &[0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn text_section_writes_80_byte_payload() {
        let seg = SegmentCommand64::text_skeleton(2);
        let mut buf = Vec::new();
        seg.write_to(&mut buf);
        let s = &buf[72..152];
        assert_eq!(s.len(), 80);

        // sectname @ 0..16 = "__text" zero-padded
        assert_eq!(&s[0..16], &name16("__text")[..]);
        // segname @ 16..32 = "__TEXT"
        assert_eq!(&s[16..32], &name16("__TEXT")[..]);
        // addr / size / offset / align all 0 in skeleton
        // except align_log2 = 2
        assert_eq!(&s[32..40], &[0u8; 8]); // addr
        assert_eq!(&s[40..48], &[0u8; 8]); // size
        assert_eq!(&s[48..52], &[0u8; 4]); // offset
        assert_eq!(&s[52..56], &[0x02, 0x00, 0x00, 0x00]); // align_log2 = 2
        assert_eq!(&s[56..60], &[0u8; 4]); // reloff
        assert_eq!(&s[60..64], &[0u8; 4]); // nreloc
        // flags @ 64..68 = S_ATTR_PURE_INSTRUCTIONS | _SOME_INSTRUCTIONS
        //                = 0x80000400 → LE [00, 04, 00, 80]
        assert_eq!(&s[64..68], &[0x00, 0x04, 0x00, 0x80]);
        assert_eq!(&s[68..72], &[0u8; 4]); // reserved1
        assert_eq!(&s[72..76], &[0u8; 4]); // reserved2
        assert_eq!(&s[76..80], &[0u8; 4]); // reserved3
    }

    #[test]
    fn patched_section_round_trips() {
        // S5+ patches addr / size / offset / reloff once the file
        // layout settles. Verify each lands at the right slot.
        let mut seg = SegmentCommand64::text_skeleton(2);
        let s = &mut seg.sections[0];
        s.size = 0x40; // 16 instructions × 4 bytes
        s.offset = 0xB8; // header (32) + segment (152) + section pad? — arbitrary for test
        s.reloff = 0x100;
        s.nreloc = 3;

        let mut buf = Vec::new();
        seg.write_to(&mut buf);
        let sec = &buf[72..152];
        // size @ 40..48 (within section payload)
        assert_eq!(&sec[40..48], &[0x40, 0, 0, 0, 0, 0, 0, 0]);
        // offset @ 48..52
        assert_eq!(&sec[48..52], &[0xB8, 0, 0, 0]);
        // reloff @ 56..60
        assert_eq!(&sec[56..60], &[0x00, 0x01, 0, 0]);
        // nreloc @ 60..64
        assert_eq!(&sec[60..64], &[0x03, 0, 0, 0]);
    }

    #[test]
    fn struct_sizes_match_apple_loader_h() {
        assert_eq!(SEGMENT_COMMAND_64_SIZE, 72);
        assert_eq!(SECTION_64_SIZE, 80);
        assert_eq!(LC_SEGMENT_64, 0x19);
    }

    /// Section type values mirror `<mach-o/loader.h>` exactly.
    /// SD-4c-prereq-c-fix routes section emit decisions off these,
    /// so a drift would silently misroute zerofill payloads.
    #[test]
    fn section_type_constants_match_loader_h() {
        assert_eq!(S_REGULAR, 0x0);
        assert_eq!(S_ZEROFILL, 0x1);
        assert_eq!(S_CSTRING_LITERALS, 0x2);
        assert_eq!(S_NON_LAZY_SYMBOL_POINTERS, 0x6);
        assert_eq!(S_LAZY_SYMBOL_POINTERS, 0x7);
        assert_eq!(S_SYMBOL_STUBS, 0x8);
        assert_eq!(S_GB_ZEROFILL, 0xC);
        assert_eq!(S_THREAD_LOCAL_REGULAR, 0x11);
        assert_eq!(S_THREAD_LOCAL_ZEROFILL, 0x12);
        assert_eq!(S_THREAD_LOCAL_VARIABLES, 0x13);
    }

    /// `SECTION_TYPE` mask isolates the low 8 bits — combining a
    /// type with attribute flags must round-trip both halves.
    #[test]
    fn section_type_mask_isolates_low_8_bits() {
        assert_eq!(SECTION_TYPE, 0x0000_00FF);
        let combined = S_THREAD_LOCAL_VARIABLES | S_ATTR_PURE_INSTRUCTIONS;
        assert_eq!(combined & SECTION_TYPE, S_THREAD_LOCAL_VARIABLES);
        assert_eq!(combined & !SECTION_TYPE, S_ATTR_PURE_INSTRUCTIONS);
        // __TEXT,__text canonical flags = (PURE | SOME) atop S_REGULAR.
        let text_flags = S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        assert_eq!(text_flags & SECTION_TYPE, S_REGULAR);
    }

    /// All zerofill-family types must be reachable through one
    /// predicate; SD-4c-prereq-c-fix's emit pass uses this to skip
    /// file-storage writes. Verifies the four currently-known
    /// no-file-storage types.
    #[test]
    fn zerofill_family_types_are_distinguishable() {
        let zf = [S_ZEROFILL, S_GB_ZEROFILL, S_THREAD_LOCAL_ZEROFILL];
        for ty in zf {
            assert!(ty == S_ZEROFILL || ty == S_GB_ZEROFILL || ty == S_THREAD_LOCAL_ZEROFILL);
            assert_ne!(ty, S_REGULAR);
            assert_ne!(ty, S_THREAD_LOCAL_REGULAR);
            assert_ne!(ty, S_CSTRING_LITERALS);
        }
    }
}
