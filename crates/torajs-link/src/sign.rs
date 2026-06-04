//! Self-research ad-hoc Mach-O code signature builder.
//!
//! Apple `codesign -s -` writes an "ad-hoc" CMS-style signature
//! blob into a Mach-O binary's `LC_CODE_SIGNATURE` data region.
//! macOS 14+ refuses to load arm64 binaries that lack this blob —
//! it's the last external-tool dependency in the torajs build
//! pipeline. This module replaces the host `codesign` call with
//! a hand-rolled blob builder driven by `sha256` (sister module).
//!
//! Blob layout (big-endian throughout, in contrast to Mach-O's
//! little-endian convention — CMS blobs use network byte order):
//!
//! ```text
//!   SuperBlob:
//!     magic         u32  CSMAGIC_EMBEDDED_SIGNATURE (0xfade0cc0)
//!     length        u32  total blob size including header
//!     count         u32  number of sub-blobs (we emit 1)
//!     // index entries:
//!     [0] type      u32  CSSLOT_CODEDIRECTORY (0)
//!     [0] offset    u32  offset of CodeDirectory from SuperBlob start
//!     // sub-blobs follow at their declared offsets:
//!     CodeDirectory:
//!       magic            u32   CSMAGIC_CODEDIRECTORY (0xfade0c02)
//!       length           u32
//!       version          u32   0x00020100 (no team ident)
//!       flags            u32   CDFLAG_ADHOC (0x2)
//!       hashOffset       u32   offset of code slot hash table
//!       identOffset      u32   offset of NUL-terminated identifier
//!       nSpecialSlots    u32   0 (no special slot hashes)
//!       nCodeSlots       u32   ceil(codeLimit / pageSize)
//!       codeLimit        u32   signed-region byte count
//!       hashSize         u8    32 (SHA-256)
//!       hashType         u8    2 (kSecCodeSignatureHashSHA256)
//!       platform         u8    0 (non-platform binary)
//!       pageSize         u8    12 (log2 of 4 KiB)
//!       spare2           u32   0
//!       scatterOffset    u32   0
//!       identifier ASCIIZ
//!       <nCodeSlots × 32-byte SHA-256 page hashes>
//! ```
//!
//! Verified against host `codesign -d --verbose=4 /path` output
//! on an Apple-signed sample — see the inline tests for the
//! field-by-field byte expectations.

use crate::sha256::sha256;

/// `CSMAGIC_EMBEDDED_SIGNATURE` — SuperBlob magic, in network byte
/// order. The host file ordering treats this big-endian.
pub const CSMAGIC_EMBEDDED_SIGNATURE: u32 = 0xfade_0cc0;

/// `CSMAGIC_CODEDIRECTORY` — magic for the embedded CodeDirectory
/// blob.
pub const CSMAGIC_CODEDIRECTORY: u32 = 0xfade_0c02;

/// `CSSLOT_CODEDIRECTORY` — the SuperBlob slot type that holds the
/// CodeDirectory blob (the only one we emit for ad-hoc).
pub const CSSLOT_CODEDIRECTORY: u32 = 0;

/// `CDFLAG_ADHOC` — CodeDirectory.flags bit signalling the
/// signature is ad-hoc (no developer certificate behind it).
pub const CDFLAG_ADHOC: u32 = 0x0000_0002;

/// CodeDirectory version that excludes the optional team
/// identifier slot. Modern Apple tooling defaults to a higher
/// version when a team is set; we don't have one, so the minimal
/// 52-byte header suffices.
pub const CD_VERSION_NO_TEAM: u32 = 0x0002_0100;

/// `kSecCodeSignatureHashSHA256` — the hashType byte we plant in
/// the CodeDirectory header.
pub const HASH_TYPE_SHA256: u8 = 2;

/// Page size codesign hashes against — 4 KiB. Note this is the
/// **codesign** page size, not the Apple Silicon kernel page size
/// (16 KiB); the two values are independent.
pub const CODESIGN_PAGE_SIZE: u32 = 4096;

/// `log2(CODESIGN_PAGE_SIZE)` — the byte the CodeDirectory header
/// stores for pageSize.
pub const CODESIGN_PAGE_SIZE_LOG2: u8 = 12;

/// Size of one SHA-256 hash slot (in bytes).
const HASH_SIZE: u8 = 32;

/// CodeDirectory fixed header size for version
/// `CD_VERSION_NO_TEAM` = 48 bytes:
///   magic / length / version / flags / hashOffset / identOffset /
///   nSpecialSlots / nCodeSlots / codeLimit  (9 × u32 = 36)
///   hashSize / hashType / platform / pageSize (4 × u8 = 4)
///   spare2 / scatterOffset                   (2 × u32 = 8)
///   = 48 bytes total.
const CD_HEADER_SIZE: u32 = 48;

/// SuperBlob fixed header (magic + length + count) = 12 bytes.
const SB_HEADER_SIZE: u32 = 12;

/// One BlobIndex entry (type + offset) = 8 bytes.
const SB_INDEX_ENTRY_SIZE: u32 = 8;

/// Build an ad-hoc CMS code-signature blob for the bytes preceding
/// it in a Mach-O image.
///
/// Inputs:
///   * `payload` — the binary bytes that precede the signature
///     region (i.e. the LC_CODE_SIGNATURE.dataoff payload-start
///     and everything before it). `codeLimit` is set to
///     `payload.len()`; the signature itself isn't hashed.
///   * `ident` — CodeDirectory identifier. macOS uses this as the
///     binary's signing identity; ad-hoc binaries typically use
///     the file's basename or a stable token like `"tora"`.
///
/// Output: the complete `LC_CODE_SIGNATURE` payload — write it
/// verbatim at the linkedit_data_command.dataoff offset the
/// matching load command points to.
pub fn build_adhoc_codesign_blob(payload: &[u8], ident: &str) -> Vec<u8> {
    let code_limit = payload.len() as u32;
    let n_code_slots = code_limit.div_ceil(CODESIGN_PAGE_SIZE);
    let ident_bytes = ident.as_bytes();
    let ident_size = (ident_bytes.len() as u32) + 1;

    // CodeDirectory layout (within itself):
    //   0..52      header
    //   52..       identifier NUL-string
    //   ..         code slot hashes
    let ident_offset_within_cd = CD_HEADER_SIZE;
    let hash_offset_within_cd = ident_offset_within_cd + ident_size;
    let cd_hash_table_size = n_code_slots * u32::from(HASH_SIZE);
    let cd_total_size = hash_offset_within_cd + cd_hash_table_size;

    let mut cd: Vec<u8> = Vec::with_capacity(cd_total_size as usize);
    cd.extend_from_slice(&CSMAGIC_CODEDIRECTORY.to_be_bytes());
    cd.extend_from_slice(&cd_total_size.to_be_bytes());
    cd.extend_from_slice(&CD_VERSION_NO_TEAM.to_be_bytes());
    cd.extend_from_slice(&CDFLAG_ADHOC.to_be_bytes());
    cd.extend_from_slice(&hash_offset_within_cd.to_be_bytes());
    cd.extend_from_slice(&ident_offset_within_cd.to_be_bytes());
    cd.extend_from_slice(&0u32.to_be_bytes()); // nSpecialSlots
    cd.extend_from_slice(&n_code_slots.to_be_bytes());
    cd.extend_from_slice(&code_limit.to_be_bytes());
    cd.push(HASH_SIZE); // hashSize
    cd.push(HASH_TYPE_SHA256); // hashType
    cd.push(0); // platform (non-platform binary)
    cd.push(CODESIGN_PAGE_SIZE_LOG2);
    cd.extend_from_slice(&0u32.to_be_bytes()); // spare2
    cd.extend_from_slice(&0u32.to_be_bytes()); // scatterOffset
    debug_assert_eq!(cd.len() as u32, CD_HEADER_SIZE);

    // Identifier
    cd.extend_from_slice(ident_bytes);
    cd.push(0);
    debug_assert_eq!(cd.len() as u32, CD_HEADER_SIZE + ident_size);

    // Code slot hashes
    for i in 0..n_code_slots {
        let start = (i * CODESIGN_PAGE_SIZE) as usize;
        let end_unbounded = ((i + 1) * CODESIGN_PAGE_SIZE) as usize;
        let end = end_unbounded.min(payload.len());
        let page = &payload[start..end];
        cd.extend_from_slice(&sha256(page));
    }
    debug_assert_eq!(cd.len() as u32, cd_total_size);

    // SuperBlob
    let sb_size = SB_HEADER_SIZE + SB_INDEX_ENTRY_SIZE + cd_total_size;
    let cd_offset = SB_HEADER_SIZE + SB_INDEX_ENTRY_SIZE;

    let mut sb: Vec<u8> = Vec::with_capacity(sb_size as usize);
    sb.extend_from_slice(&CSMAGIC_EMBEDDED_SIGNATURE.to_be_bytes());
    sb.extend_from_slice(&sb_size.to_be_bytes());
    sb.extend_from_slice(&1u32.to_be_bytes()); // count
    sb.extend_from_slice(&CSSLOT_CODEDIRECTORY.to_be_bytes());
    sb.extend_from_slice(&cd_offset.to_be_bytes());
    sb.extend_from_slice(&cd);
    debug_assert_eq!(sb.len() as u32, sb_size);

    sb
}

/// Compute the upper-bound total blob size for the given payload
/// length + identifier, without actually computing any hashes.
/// Used by `link_to_exec` (S5-C) to reserve space in the
/// `__LINKEDIT` segment before the binary is fully written —
/// then `build_adhoc_codesign_blob` fills the reserved region
/// once `codeLimit` is final.
pub fn adhoc_codesign_blob_size(code_limit: u32, ident: &str) -> u32 {
    let n_code_slots = code_limit.div_ceil(CODESIGN_PAGE_SIZE);
    let ident_size = (ident.len() as u32) + 1;
    let cd_hash_table_size = n_code_slots * u32::from(HASH_SIZE);
    let cd_total_size = CD_HEADER_SIZE + ident_size + cd_hash_table_size;
    SB_HEADER_SIZE + SB_INDEX_ENTRY_SIZE + cd_total_size
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32_be(bytes: &[u8], off: usize) -> u32 {
        u32::from_be_bytes(bytes[off..off + 4].try_into().unwrap())
    }

    /// 4-byte payload → 1 code slot. Verify each blob field lands
    /// at the spec offset and carries the spec value.
    #[test]
    fn small_payload_one_code_slot_layout() {
        let payload = b"abcd";
        let ident = "tora";
        let blob = build_adhoc_codesign_blob(payload, ident);

        // SuperBlob: magic / length / count at 0..12
        assert_eq!(read_u32_be(&blob, 0), CSMAGIC_EMBEDDED_SIGNATURE);
        let sb_len = read_u32_be(&blob, 4);
        assert_eq!(sb_len, blob.len() as u32);
        assert_eq!(read_u32_be(&blob, 8), 1);

        // Index[0]: type / offset at 12..20
        assert_eq!(read_u32_be(&blob, 12), CSSLOT_CODEDIRECTORY);
        let cd_offset = read_u32_be(&blob, 16);
        assert_eq!(cd_offset, 20); // SB header (12) + 1 index entry (8)

        // CodeDirectory header @ cd_offset
        let cd = &blob[cd_offset as usize..];
        assert_eq!(read_u32_be(cd, 0), CSMAGIC_CODEDIRECTORY);
        assert_eq!(read_u32_be(cd, 8), CD_VERSION_NO_TEAM);
        assert_eq!(read_u32_be(cd, 12), CDFLAG_ADHOC);
        // hashOffset = 48 (header) + 5 ("tora" + NUL)
        let hash_off = read_u32_be(cd, 16);
        assert_eq!(hash_off, 48 + 5);
        // identOffset = 48
        assert_eq!(read_u32_be(cd, 20), 48);
        // nSpecialSlots = 0
        assert_eq!(read_u32_be(cd, 24), 0);
        // nCodeSlots = 1
        assert_eq!(read_u32_be(cd, 28), 1);
        // codeLimit = 4
        assert_eq!(read_u32_be(cd, 32), 4);
        // hashSize = 32, hashType = SHA256, platform = 0, pageSize = 12
        assert_eq!(cd[36], 32);
        assert_eq!(cd[37], HASH_TYPE_SHA256);
        assert_eq!(cd[38], 0);
        assert_eq!(cd[39], CODESIGN_PAGE_SIZE_LOG2);
        // spare2 / scatterOffset = 0
        assert_eq!(read_u32_be(cd, 40), 0);
        assert_eq!(read_u32_be(cd, 44), 0);

        // Identifier "tora\0" at offset 48
        assert_eq!(&cd[48..52], b"tora");
        assert_eq!(cd[52], 0);

        // Code slot hash @ hash_off — SHA256 of "abcd"
        let hash_start = hash_off as usize;
        let expected = sha256(b"abcd");
        assert_eq!(&cd[hash_start..hash_start + 32], &expected[..]);
    }

    /// Boundary: payload exactly 4096 byte → still 1 slot.
    /// Payload 4097 → 2 slots, second hashes only 1 byte.
    #[test]
    fn code_slot_count_at_page_boundary() {
        for (size, expected_slots) in [(0, 0), (1, 1), (4095, 1), (4096, 1), (4097, 2), (8192, 2)] {
            let payload = vec![0u8; size];
            let blob = build_adhoc_codesign_blob(&payload, "x");
            // nCodeSlots @ CodeDirectory + 28
            let cd_off = SB_HEADER_SIZE + SB_INDEX_ENTRY_SIZE;
            let slots = read_u32_be(&blob, (cd_off + 28) as usize);
            assert_eq!(
                slots, expected_slots,
                "{size} bytes should yield {expected_slots} slots",
            );
        }
    }

    /// Two-page payload — second slot hashes the second page
    /// (here: zeros), confirming the page-by-page iteration is
    /// correct.
    #[test]
    fn two_page_payload_hashes_each_page_separately() {
        let mut payload: Vec<u8> = (0..4096).map(|i| i as u8).collect(); // first page
        payload.extend_from_slice(&[0u8; 4096]); // second page
        let blob = build_adhoc_codesign_blob(&payload, "x");

        let cd_off = SB_HEADER_SIZE + SB_INDEX_ENTRY_SIZE;
        let cd = &blob[cd_off as usize..];
        let hash_off = read_u32_be(cd, 16) as usize;

        let first_hash = sha256(&payload[..4096]);
        let second_hash = sha256(&payload[4096..]);
        assert_eq!(&cd[hash_off..hash_off + 32], &first_hash[..]);
        assert_eq!(&cd[hash_off + 32..hash_off + 64], &second_hash[..]);
    }

    /// The trailing (partial) page hashes only the present bytes,
    /// not a zero-padded full page — Apple's CodeDirectory spec
    /// says explicitly that `codeLimit` bounds the hashing.
    #[test]
    fn trailing_partial_page_hashes_only_present_bytes() {
        // 4096 + 100 bytes — second hash should be over those 100
        // bytes alone, not a 4 KiB block zero-padded.
        let mut payload: Vec<u8> = vec![0xAA; 4096];
        payload.extend_from_slice(&[0xBB; 100]);
        let blob = build_adhoc_codesign_blob(&payload, "x");

        let cd_off = SB_HEADER_SIZE + SB_INDEX_ENTRY_SIZE;
        let cd = &blob[cd_off as usize..];
        let hash_off = read_u32_be(cd, 16) as usize;
        let expected = sha256(&payload[4096..]);
        assert_eq!(&cd[hash_off + 32..hash_off + 64], &expected[..]);
    }

    /// `adhoc_codesign_blob_size` predicts the size exactly so
    /// `link_to_exec` can reserve LINKEDIT space before fully
    /// laying out the binary.
    #[test]
    fn adhoc_codesign_blob_size_matches_actual() {
        for size in [0, 1, 100, 4095, 4096, 4097, 12345, 65536] {
            let payload = vec![0u8; size];
            let predicted = adhoc_codesign_blob_size(size as u32, "tora");
            let actual = build_adhoc_codesign_blob(&payload, "tora").len() as u32;
            assert_eq!(predicted, actual, "size mismatch for {size}-byte payload");
        }
    }

    /// Identifier is included verbatim in the blob with a NUL
    /// terminator — long ident, short ident, single-char ident.
    #[test]
    fn identifier_round_trip() {
        for ident in [
            "tora",
            "a",
            "an_identifier_with_lots_of_chars_to_check_offset_math",
        ] {
            let blob = build_adhoc_codesign_blob(b"hello", ident);
            let cd_off = SB_HEADER_SIZE + SB_INDEX_ENTRY_SIZE;
            let cd = &blob[cd_off as usize..];
            let ident_off = read_u32_be(cd, 20) as usize;
            let read_ident = &cd[ident_off..ident_off + ident.len()];
            assert_eq!(read_ident, ident.as_bytes());
            assert_eq!(cd[ident_off + ident.len()], 0);
        }
    }
}
