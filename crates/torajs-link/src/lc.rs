//! Mach-O load command constants + writers that `exec.rs` composes
//! into the final `MH_EXECUTE` image. Extracted so `exec.rs` stays
//! within the file-size hard limit while keeping the per-command
//! wire format auditable against `<mach-o/loader.h>`.

// ---- Constants ----

/// `MH_EXECUTE` — `mach_header_64.filetype` for a runnable binary.
pub const MH_EXECUTE: u32 = 0x2;
/// `MH_NOUNDEFS` — every symbol resolved at link time.
pub const MH_NOUNDEFS: u32 = 0x1;
/// `MH_DYLDLINK` — binary uses dyld for loading.
pub const MH_DYLDLINK: u32 = 0x4;
/// `MH_TWOLEVEL` — two-level namespace lookups for dylib symbols.
pub const MH_TWOLEVEL: u32 = 0x80;
/// `MH_PIE` — position-independent executable.
pub const MH_PIE: u32 = 0x0020_0000;

/// `LC_REQ_DYLD` — flag bit signalling that dyld must understand
/// this load command type or refuse to load the image.
pub const LC_REQ_DYLD: u32 = 0x8000_0000;
/// `LC_LOAD_DYLIB` — bind a dynamic library at exec time.
pub const LC_LOAD_DYLIB: u32 = 0xC;
/// `LC_LOAD_DYLINKER` — points at `/usr/lib/dyld`.
pub const LC_LOAD_DYLINKER: u32 = 0xE;
/// `LC_DYSYMTAB` — symbol partition (local / extdef / undef).
pub const LC_DYSYMTAB: u32 = 0xB;
/// `LC_BUILD_VERSION` — platform + min OS + SDK.
pub const LC_BUILD_VERSION: u32 = 0x32;
/// `LC_MAIN` — entry point file offset.
pub const LC_MAIN: u32 = LC_REQ_DYLD | 0x28;

/// `PLATFORM_MACOS` — the platform tag for macOS in
/// `build_version_command.platform`.
pub const PLATFORM_MACOS: u32 = 1;

/// Apple Silicon page size — used for segment vmsize / fileoff
/// alignment. The kernel maps segments at this granularity.
pub const APPLE_SILICON_PAGE_SIZE: u64 = 0x4000;

/// Virtual base of `__TEXT` — sits directly after `__PAGEZERO`'s
/// 4 GiB null guard. macOS dyld randomises around this base for
/// PIE binaries; the loader treats it as a hint.
pub const TEXT_VMADDR_BASE: u64 = 0x1_0000_0000;

/// `__PAGEZERO` vmsize — 4 GiB; covers the full 32-bit address
/// range so any null-dereference traps cleanly.
pub const PAGEZERO_VMSIZE: u64 = 0x1_0000_0000;

pub(crate) const DYLINKER_PATH: &str = "/usr/lib/dyld";

/// `libSystem.B.dylib` path — single dyld substrate for SD-1
/// (libcurl etc. deferred to SD-1+).
pub(crate) const LIBSYSTEM_PATH: &str = "/usr/lib/libSystem.B.dylib";

/// libSystem.B.dylib `current_version`, encoded as nibbles
/// `X.Y.Z = (X<<16)|(Y<<8)|Z`. `1356.0.0` matches macOS 14+
/// (verified via `otool -L` of an `ssa_inkwell`-built production
/// binary at HEAD `cd546fa`).
pub const LIBSYSTEM_CURRENT_VERSION: u32 = 1356u32 << 16;

/// libSystem.B.dylib `compatibility_version` = `1.0.0` — the
/// published stable contract dyld validates the binding against.
pub const LIBSYSTEM_COMPAT_VERSION: u32 = 1u32 << 16;

/// `LC_LOAD_DYLIB` cmdsize for the libSystem path:
///   header  = 24 bytes (cmd + cmdsize + name_off + timestamp +
///                       current_version + compatibility_version)
///   name    = 26 bytes "/usr/lib/libSystem.B.dylib" + 1 NUL = 27
///   total   = 51 bytes → pad to 8-byte boundary → 56
pub const LOAD_DYLIB_LIBSYSTEM_CMDSIZE: u32 = 56;

/// `LC_LOAD_DYLINKER` payload = 12 bytes (cmd + cmdsize + name
/// offset) + dylinker path (NUL-terminated) padded to 8-byte
/// boundary. For "/usr/lib/dyld\0" (14 B) → 12 + 14 = 26, round
/// up to 32.
pub const LOAD_DYLINKER_CMDSIZE: u32 = 32;

/// `LC_BUILD_VERSION` = 24 bytes when `ntools = 0` (we don't
/// declare a linker version).
pub const BUILD_VERSION_CMDSIZE: u32 = 24;

/// `LC_MAIN` = 24 bytes (entry_off u64 + stacksize u64).
pub const MAIN_CMDSIZE: u32 = 24;

/// `LC_DYSYMTAB` = 80 bytes fixed.
pub const DYSYMTAB_CMDSIZE: u32 = 80;

/// `LC_CODE_SIGNATURE` — `linkedit_data_command` pointing into
/// `__LINKEDIT` at the embedded codesign blob.
pub const LC_CODE_SIGNATURE: u32 = 0x1D;

/// `LC_DYLD_CHAINED_FIXUPS` — `linkedit_data_command` pointing
/// at the dyld chained-fixups blob in `__LINKEDIT`. Modern
/// macOS (≥ 12 on arm64) requires this LC for dyld to bind any
/// `__DATA,__la_symbol_ptr` slot referencing an LC_LOAD_DYLIB
/// import. Value carries the `LC_REQ_DYLD` flag so older dyld
/// implementations refuse the binary rather than silent-skip.
pub const LC_DYLD_CHAINED_FIXUPS: u32 = LC_REQ_DYLD | 0x34;

/// `linkedit_data_command` total size = 16 bytes (cmd + cmdsize +
/// dataoff + datasize).
pub const LINKEDIT_DATA_CMDSIZE: u32 = 16;

// ---- Writers ----

/// Append `LC_LOAD_DYLIB /usr/lib/libSystem.B.dylib` — the SD-1
/// dyld substrate. Bundles all libSystem symbols (malloc / pthread /
/// unwind / dyld helpers / syscalls); per-symbol stubs land in SD-2.
///
/// Timestamp is set to `2` to match what `ld64` historically emits
/// when no real build stamp is available; dyld ignores the field.
pub fn write_load_dylib_libsystem(buf: &mut Vec<u8>) {
    let start = buf.len();
    let name_bytes = LIBSYSTEM_PATH.as_bytes();
    buf.extend_from_slice(&LC_LOAD_DYLIB.to_le_bytes());
    buf.extend_from_slice(&LOAD_DYLIB_LIBSYSTEM_CMDSIZE.to_le_bytes());
    buf.extend_from_slice(&24u32.to_le_bytes()); // name field offset (after header)
    buf.extend_from_slice(&2u32.to_le_bytes()); // timestamp placeholder
    buf.extend_from_slice(&LIBSYSTEM_CURRENT_VERSION.to_le_bytes());
    buf.extend_from_slice(&LIBSYSTEM_COMPAT_VERSION.to_le_bytes());
    buf.extend_from_slice(name_bytes);
    buf.push(0); // NUL terminator
    while (buf.len() - start) < (LOAD_DYLIB_LIBSYSTEM_CMDSIZE as usize) {
        buf.push(0);
    }
    debug_assert_eq!(buf.len() - start, LOAD_DYLIB_LIBSYSTEM_CMDSIZE as usize);
}

/// Append `LC_LOAD_DYLINKER` carrying `/usr/lib/dyld`. cmdsize is
/// fixed at 32 so the trailing NUL + zero padding land at 8-byte
/// alignment, matching what `clang -o` emits.
pub fn write_load_dylinker(buf: &mut Vec<u8>) {
    let start = buf.len();
    let name_bytes = DYLINKER_PATH.as_bytes();
    buf.extend_from_slice(&LC_LOAD_DYLINKER.to_le_bytes());
    buf.extend_from_slice(&LOAD_DYLINKER_CMDSIZE.to_le_bytes());
    buf.extend_from_slice(&12u32.to_le_bytes()); // name field at offset 12
    buf.extend_from_slice(name_bytes);
    buf.push(0); // NUL terminator
    while (buf.len() - start) < (LOAD_DYLINKER_CMDSIZE as usize) {
        buf.push(0);
    }
    debug_assert_eq!(buf.len() - start, LOAD_DYLINKER_CMDSIZE as usize);
}

/// Append `LC_BUILD_VERSION` declaring macOS 14.0 + SDK 14.0. The
/// version is encoded as nibbles `X.Y.Z = (X<<16)|(Y<<8)|Z` per
/// `<mach-o/loader.h>::build_version_command`.
pub fn write_build_version(buf: &mut Vec<u8>) {
    let start = buf.len();
    buf.extend_from_slice(&LC_BUILD_VERSION.to_le_bytes());
    buf.extend_from_slice(&BUILD_VERSION_CMDSIZE.to_le_bytes());
    buf.extend_from_slice(&PLATFORM_MACOS.to_le_bytes());
    buf.extend_from_slice(&((14u32 << 16) | (0u32 << 8) | 0u32).to_le_bytes()); // minos 14.0
    buf.extend_from_slice(&((14u32 << 16) | (0u32 << 8) | 0u32).to_le_bytes()); // sdk 14.0
    buf.extend_from_slice(&0u32.to_le_bytes()); // ntools = 0
    debug_assert_eq!(buf.len() - start, BUILD_VERSION_CMDSIZE as usize);
}

/// Append `LC_MAIN` carrying the `_main` file offset. stacksize=0
/// means dyld uses the default 8 MiB main-thread stack.
pub fn write_lc_main(buf: &mut Vec<u8>, entry_off: u64) {
    let start = buf.len();
    buf.extend_from_slice(&LC_MAIN.to_le_bytes());
    buf.extend_from_slice(&MAIN_CMDSIZE.to_le_bytes());
    buf.extend_from_slice(&entry_off.to_le_bytes());
    buf.extend_from_slice(&0u64.to_le_bytes());
    debug_assert_eq!(buf.len() - start, MAIN_CMDSIZE as usize);
}

/// Append `LC_CODE_SIGNATURE` pointing at the codesign blob in
/// `__LINKEDIT`. The blob payload is written separately at
/// `dataoff` by the link driver; this load command just records
/// the location and size so dyld can find it.
pub fn write_code_signature(buf: &mut Vec<u8>, dataoff: u32, datasize: u32) {
    let start = buf.len();
    buf.extend_from_slice(&LC_CODE_SIGNATURE.to_le_bytes());
    buf.extend_from_slice(&LINKEDIT_DATA_CMDSIZE.to_le_bytes());
    buf.extend_from_slice(&dataoff.to_le_bytes());
    buf.extend_from_slice(&datasize.to_le_bytes());
    debug_assert_eq!(buf.len() - start, LINKEDIT_DATA_CMDSIZE as usize);
}

/// Append `LC_DYLD_CHAINED_FIXUPS` pointing at the chained-fixups
/// blob in `__LINKEDIT`. Same wire shape as `write_code_signature`
/// — both are `linkedit_data_command` (16 bytes total). The blob
/// payload itself is built by `chained_fixups::build_chained_fixups`
/// and written separately at `dataoff` by the link driver.
pub fn write_dyld_chained_fixups(buf: &mut Vec<u8>, dataoff: u32, datasize: u32) {
    let start = buf.len();
    buf.extend_from_slice(&LC_DYLD_CHAINED_FIXUPS.to_le_bytes());
    buf.extend_from_slice(&LINKEDIT_DATA_CMDSIZE.to_le_bytes());
    buf.extend_from_slice(&dataoff.to_le_bytes());
    buf.extend_from_slice(&datasize.to_le_bytes());
    debug_assert_eq!(buf.len() - start, LINKEDIT_DATA_CMDSIZE as usize);
}

/// Append `LC_DYSYMTAB`. We emit only the extdef partition (every
/// defined function is external); local + undef counts stay 0.
/// `iundefsym = nextdefsym` means the undef partition starts
/// directly after the extdefs (and is empty).
pub fn write_dysymtab(buf: &mut Vec<u8>, nextdefsym: u32) {
    let start = buf.len();
    buf.extend_from_slice(&LC_DYSYMTAB.to_le_bytes());
    buf.extend_from_slice(&DYSYMTAB_CMDSIZE.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // ilocalsym
    buf.extend_from_slice(&0u32.to_le_bytes()); // nlocalsym
    buf.extend_from_slice(&0u32.to_le_bytes()); // iextdefsym
    buf.extend_from_slice(&nextdefsym.to_le_bytes());
    buf.extend_from_slice(&nextdefsym.to_le_bytes()); // iundefsym
    buf.extend_from_slice(&0u32.to_le_bytes()); // nundefsym
    // Remaining 12 fields = toc / modtab / extrefsym / indirectsym
    // / extrel / locrel — six (off, count) u32 pairs, all zero
    // because we emit nothing in those tables. Cmd + cmdsize + 6
    // partition fields + 12 trailing fields = 20 u32 = 80 bytes.
    for _ in 0..12 {
        buf.extend_from_slice(&0u32.to_le_bytes());
    }
    debug_assert_eq!(buf.len() - start, DYSYMTAB_CMDSIZE as usize);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_dylib_libsystem_layout_matches_spec() {
        let mut buf = Vec::new();
        write_load_dylib_libsystem(&mut buf);
        assert_eq!(buf.len(), LOAD_DYLIB_LIBSYSTEM_CMDSIZE as usize);
        // cmd
        assert_eq!(
            u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            LC_LOAD_DYLIB
        );
        // cmdsize = 56
        assert_eq!(u32::from_le_bytes(buf[4..8].try_into().unwrap()), 56);
        // name field offset = 24 (skip the 24-byte header)
        assert_eq!(u32::from_le_bytes(buf[8..12].try_into().unwrap()), 24);
        // timestamp placeholder = 2
        assert_eq!(u32::from_le_bytes(buf[12..16].try_into().unwrap()), 2);
        // current_version = 1356.0.0 = 0x054C_0000
        assert_eq!(
            u32::from_le_bytes(buf[16..20].try_into().unwrap()),
            LIBSYSTEM_CURRENT_VERSION
        );
        assert_eq!(LIBSYSTEM_CURRENT_VERSION, 0x054C_0000);
        // compatibility_version = 1.0.0 = 0x0001_0000
        assert_eq!(
            u32::from_le_bytes(buf[20..24].try_into().unwrap()),
            LIBSYSTEM_COMPAT_VERSION
        );
        assert_eq!(LIBSYSTEM_COMPAT_VERSION, 0x0001_0000);
        // path at offset 24 …
        assert_eq!(
            &buf[24..24 + LIBSYSTEM_PATH.len()],
            LIBSYSTEM_PATH.as_bytes()
        );
        // … NUL-terminated …
        assert_eq!(buf[24 + LIBSYSTEM_PATH.len()], 0);
        // … and zero-padded to the 8-byte boundary.
        for off in (24 + LIBSYSTEM_PATH.len() + 1)..(LOAD_DYLIB_LIBSYSTEM_CMDSIZE as usize) {
            assert_eq!(buf[off], 0);
        }
    }

    #[test]
    fn load_dylib_libsystem_cmdsize_is_8_byte_aligned() {
        assert_eq!(LOAD_DYLIB_LIBSYSTEM_CMDSIZE % 8, 0);
        // dylib_command header (24 B) + path "/usr/lib/libSystem.B.dylib" (26 B)
        // + NUL (1 B) = 51 → round up to 56.
        assert!(LOAD_DYLIB_LIBSYSTEM_CMDSIZE as usize >= 24 + LIBSYSTEM_PATH.len() + 1);
    }

    #[test]
    fn load_dylinker_is_32_bytes_with_path_at_offset_12() {
        let mut buf = Vec::new();
        write_load_dylinker(&mut buf);
        assert_eq!(buf.len(), LOAD_DYLINKER_CMDSIZE as usize);
        // cmd
        assert_eq!(
            u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            LC_LOAD_DYLINKER
        );
        // cmdsize
        assert_eq!(u32::from_le_bytes(buf[4..8].try_into().unwrap()), 32);
        // name_offset = 12
        assert_eq!(u32::from_le_bytes(buf[8..12].try_into().unwrap()), 12);
        // path starts at offset 12, NUL-terminated
        assert_eq!(&buf[12..12 + DYLINKER_PATH.len()], DYLINKER_PATH.as_bytes());
        assert_eq!(buf[12 + DYLINKER_PATH.len()], 0);
    }

    #[test]
    fn build_version_pins_macos_14() {
        let mut buf = Vec::new();
        write_build_version(&mut buf);
        assert_eq!(buf.len(), BUILD_VERSION_CMDSIZE as usize);
        assert_eq!(
            u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            LC_BUILD_VERSION
        );
        assert_eq!(u32::from_le_bytes(buf[4..8].try_into().unwrap()), 24);
        assert_eq!(
            u32::from_le_bytes(buf[8..12].try_into().unwrap()),
            PLATFORM_MACOS
        );
        assert_eq!(
            u32::from_le_bytes(buf[12..16].try_into().unwrap()),
            14u32 << 16
        ); // minos 14.0
        assert_eq!(
            u32::from_le_bytes(buf[16..20].try_into().unwrap()),
            14u32 << 16
        ); // sdk 14.0
        assert_eq!(u32::from_le_bytes(buf[20..24].try_into().unwrap()), 0); // ntools
    }

    #[test]
    fn lc_main_carries_entry_offset() {
        let mut buf = Vec::new();
        write_lc_main(&mut buf, 0x4000);
        assert_eq!(buf.len(), MAIN_CMDSIZE as usize);
        assert_eq!(u32::from_le_bytes(buf[0..4].try_into().unwrap()), LC_MAIN);
        assert_eq!(u32::from_le_bytes(buf[4..8].try_into().unwrap()), 24);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 0x4000);
        assert_eq!(u64::from_le_bytes(buf[16..24].try_into().unwrap()), 0); // stacksize default
    }

    #[test]
    fn dysymtab_lays_out_partition_indices() {
        let mut buf = Vec::new();
        write_dysymtab(&mut buf, 3);
        assert_eq!(buf.len(), DYSYMTAB_CMDSIZE as usize);
        assert_eq!(
            u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            LC_DYSYMTAB
        );
        assert_eq!(u32::from_le_bytes(buf[4..8].try_into().unwrap()), 80);
        // ilocalsym=0 / nlocalsym=0 / iextdefsym=0 / nextdefsym=3 /
        // iundefsym=3 / nundefsym=0
        assert_eq!(u32::from_le_bytes(buf[8..12].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(buf[12..16].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(buf[16..20].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(buf[20..24].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(buf[24..28].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(buf[28..32].try_into().unwrap()), 0);
        // Remaining 12 u32 fields are zero (cmd+cmdsize+6 partition+12 trailing = 80 B).
        for off in (32..80).step_by(4) {
            assert_eq!(u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()), 0);
        }
    }
}
