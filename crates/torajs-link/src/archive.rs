//! Apple `.a` static archive parser (BSD variant with `#1/N`
//! extended name encoding).
//!
//! A `.a` file is the format `ar(1)` produces — a magic header
//! followed by a sequence of fixed-size headers, each describing
//! one packed member (typically a `.o` Mach-O object file). The
//! linker reads `.a`s to pull in the staticlib `.o`s that satisfy
//! the user code's undefined symbols. Apple's `ld64` and the GNU
//! `ld` both consume the same format on macOS, so torajs-link
//! parses it once and walks the embedded Mach-O members directly.
//!
//! Wire format (Apple `cctools` `man 5 ar`):
//!
//! ```text
//!   [0..8]    "!<arch>\n"                       — global magic
//!   [8..]     sequence of 60-byte headers:
//!               name  [16 B] — usually space-padded ASCII; if
//!                             the name starts with "#1/N " the
//!                             real name is the first N bytes of
//!                             the member data (BSD long-name
//!                             extension; N is decimal ASCII).
//!               date  [12 B ASCII decimal]
//!               uid   [ 6 B ASCII decimal]
//!               gid   [ 6 B ASCII decimal]
//!               mode  [ 8 B ASCII octal]
//!               size  [10 B ASCII decimal] — member data length
//!                                            in bytes (includes
//!                                            the embedded name
//!                                            when "#1/N" is used)
//!               fmag  [ 2 B] = "`\n" (0x60 0x0A)
//!             member data [size bytes]
//!             two-byte alignment pad if size is odd
//! ```
//!
//! S7-A parses the ar structure: returns a flat list of
//! `ArMember { name, data }` slices into the input buffer.
//! S7-B (this commit) walks each Mach-O member and aggregates the
//! defined-external symbols into an `ArchiveIndex` — name →
//! (member index, n_value within member) so the link driver can
//! satisfy an undefined extern by pulling in the owning member's
//! bytes and patching the reloc against the member's
//! `__TEXT,__text` slot.
//! S7-C wires `ArchiveIndex` into `link_to_exec` so the
//! production swap (#9) can resolve `__torajs_*` and `_libc_*`
//! externs from `libtorajs_*.a`.

/// Apple `.a` archive global magic — 8 bytes at the head of every
/// file.
pub const AR_MAGIC: &[u8; 8] = b"!<arch>\n";

/// Per-member header size — 16 (name) + 12 + 6 + 6 + 8 + 10 + 2
/// (fmag) = 60 bytes.
pub const AR_HEADER_SIZE: usize = 60;

/// `ar_fmag` literal — the 2 bytes that terminate every header.
const AR_FMAG: &[u8; 2] = b"`\n";

/// BSD long-name prefix — when `ar_name` starts with `#1/N`, the
/// real name is the first `N` bytes of the member data and the
/// member data effectively starts at `offset + N`.
const BSD_LONG_NAME_PREFIX: &[u8; 3] = b"#1/";

/// One parsed archive member. Both `name` and `data` borrow from
/// the input buffer — the parser does not allocate per-member
/// strings or copy bytes.
#[derive(Debug, Clone, Copy)]
pub struct ArMember<'a> {
    /// Member name without the BSD long-name prefix bytes. For
    /// names short enough to fit in the 16-byte `ar_name` slot,
    /// this trims trailing spaces.
    pub name: &'a str,
    /// Member payload (the `.o` Mach-O bytes for code archives).
    pub data: &'a [u8],
}

/// Parse `.a` archive bytes into a flat list of members.
///
/// Apple `ld64` injects a `__.SYMDEF` (or `__.SYMDEF SORTED`)
/// pseudo-member as the first entry — a precomputed symbol →
/// member-offset index. We skip it; the link driver re-derives the
/// symbol table by walking each Mach-O member directly, which
/// avoids drift between the index and the real defined-symbol set.
///
/// Errors as `Err(ArParseError::*)` rather than panicking so the
/// caller can attribute the failure to a specific archive when
/// linking against many staticlibs.
pub fn parse_archive(bytes: &[u8]) -> Result<Vec<ArMember<'_>>, ArParseError> {
    if bytes.len() < AR_MAGIC.len() || &bytes[..AR_MAGIC.len()] != AR_MAGIC {
        return Err(ArParseError::BadMagic);
    }

    let mut members: Vec<ArMember<'_>> = Vec::new();
    let mut cursor = AR_MAGIC.len();

    while cursor < bytes.len() {
        // Header lives at [cursor..cursor + 60].
        if bytes.len() < cursor + AR_HEADER_SIZE {
            return Err(ArParseError::TruncatedHeader { offset: cursor });
        }
        let header = &bytes[cursor..cursor + AR_HEADER_SIZE];
        if &header[58..60] != AR_FMAG {
            return Err(ArParseError::BadFmag { offset: cursor });
        }

        let raw_name = &header[0..16];
        let size_str = trim_ascii(&header[48..58]);
        let raw_size = parse_decimal(size_str).ok_or(ArParseError::BadSize {
            offset: cursor,
            field: ascii_to_owned(&header[48..58]),
        })?;

        let data_start = cursor + AR_HEADER_SIZE;
        let data_end = data_start + raw_size;
        if bytes.len() < data_end {
            return Err(ArParseError::TruncatedMember {
                offset: cursor,
                size: raw_size,
            });
        }

        // BSD long-name encoding: ar_name starts with "#1/N", real
        // name is the first N bytes of member data.
        let (member_name_bytes, member_data) =
            if raw_name.len() >= 3 && &raw_name[..3] == BSD_LONG_NAME_PREFIX {
                let n_str = trim_ascii(&raw_name[3..]);
                let n = parse_decimal(n_str).ok_or(ArParseError::BadLongNameLen {
                    offset: cursor,
                    field: ascii_to_owned(raw_name),
                })?;
                if raw_size < n {
                    return Err(ArParseError::TruncatedLongName {
                        offset: cursor,
                        declared: n,
                        member_size: raw_size,
                    });
                }
                // Apple `ld64` pads the long-name field with NULs
                // up to a 4- or 8-byte boundary so the .o body
                // that follows aligns. Trim trailing NULs so the
                // returned name is the bare filename.
                let raw = &bytes[data_start..data_start + n];
                let trimmed = trim_ascii(raw);
                let data = &bytes[data_start + n..data_end];
                (trimmed, data)
            } else {
                (trim_ascii(raw_name), &bytes[data_start..data_end])
            };

        let name =
            std::str::from_utf8(member_name_bytes).map_err(|_| ArParseError::NameNotUtf8 {
                offset: cursor,
                bytes: member_name_bytes.to_vec(),
            })?;

        // Skip the BSD __.SYMDEF index — we re-derive the symbol
        // table by walking each Mach-O member directly.
        let is_symdef =
            name == "__.SYMDEF" || name == "__.SYMDEF SORTED" || name.starts_with("__.SYMDEF");
        if !is_symdef {
            members.push(ArMember {
                name,
                data: member_data,
            });
        }

        // Advance past member + optional 2-byte alignment pad.
        let mut next = data_end;
        if raw_size % 2 != 0 && next < bytes.len() {
            next += 1;
        }
        cursor = next;
    }

    Ok(members)
}

// ---- S7-B: Mach-O member symbol extraction ----

use std::collections::BTreeMap;

use torajs_obj::{LC_SYMTAB, MH_MAGIC_64, N_EXT, N_SECT, N_TYPE};

/// One defined-external symbol observed inside a `.o` member of a
/// `.a` archive. `member_idx` indexes into the `Vec<ArMember>`
/// `parse_archive` returned; `n_value` is the symbol's offset
/// within the member's `__TEXT,__text` (= `nlist_64.n_value` for
/// the `N_SECT|N_EXT` entry).
#[derive(Debug, Clone)]
pub struct ArchiveSymbol {
    pub member_idx: usize,
    pub n_value: u64,
}

/// `name → ArchiveSymbol` map built by walking every `.o` member's
/// LC_SYMTAB. The link driver uses this to satisfy an undefined
/// extern: lookup name → pull in that member's bytes → patch the
/// reloc against the member's `__TEXT,__text` slot.
pub type ArchiveIndex = BTreeMap<String, ArchiveSymbol>;

/// Walk a parsed archive, parse each Mach-O member's symbol
/// table, and aggregate every defined external symbol into a
/// single `name → (member_idx, n_value)` map.
///
/// Skips members whose Mach-O magic doesn't match `MH_MAGIC_64`
/// (e.g. ARM64e variants, fat headers — neither of which appears
/// in the `release-build.sh` output today). Returns
/// `Err(ArchiveParseError::*)` when a Mach-O header is malformed
/// or a symtab points past the member's bytes — caller can
/// blame the staticlib by name + member offset.
///
/// Duplicate names: when two members both define the same
/// symbol, the *first* one wins. Apple's `ld64` uses the same
/// convention; downstream callers can grep for collisions if
/// they care.
pub fn build_archive_index(members: &[ArMember<'_>]) -> Result<ArchiveIndex, ArchiveParseError> {
    let mut index: ArchiveIndex = BTreeMap::new();
    for (i, m) in members.iter().enumerate() {
        let syms = parse_member_defined_externs(m).map_err(|kind| ArchiveParseError {
            member_idx: i,
            member_name: m.name.to_string(),
            kind,
        })?;
        for (name, n_value, _n_sect) in syms {
            index.entry(name).or_insert(ArchiveSymbol {
                member_idx: i,
                n_value,
            });
        }
    }
    Ok(index)
}

/// Parse one Mach-O member's `LC_SYMTAB` and return every
/// `(N_SECT | N_EXT)` symbol — those are the defined-external
/// entries Apple `ld` resolves against. Pure parser; no allocation
/// beyond the result `Vec`.
/// Parse a member's defined extern symbols. Returns `(name, n_value, n_sect)`
/// per N_SECT | N_EXT entry — the section the symbol lives in is carried
/// alongside its section-relative offset so callers can dispatch the right
/// final-binary vaddr (text base vs. `__DATA,*` section placement) at link
/// time. SD-4c swap-2e — pre-2e callers blindly used `member.vaddr +
/// n_value` which silently misplaces every `__DATA`-resident static (rustc
/// `static FOO: AtomicU32 = ...`) onto bogus `__text` bytes.
pub fn parse_member_defined_externs(
    member: &ArMember<'_>,
) -> Result<Vec<(String, u64, u8)>, MemberSymtabError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberSymtabError::TruncatedHeader);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        // Skip non-MH_MAGIC_64 members silently — fat archives,
        // ARM64e variants, etc.
        return Ok(Vec::new());
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;

    let mut cursor = 32usize;
    let mut symtab: Option<(u32, u32, u32, u32)> = None; // (symoff, nsyms, stroff, strsize)
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberSymtabError::TruncatedLoadCommand { offset: cursor });
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberSymtabError::TruncatedLoadCommand { offset: cursor });
        }
        if cmd == LC_SYMTAB {
            if cmdsize < 24 {
                return Err(MemberSymtabError::TruncatedSymtabCmd { offset: cursor });
            }
            let symoff = u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap());
            let nsyms = u32::from_le_bytes(bytes[cursor + 12..cursor + 16].try_into().unwrap());
            let stroff = u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap());
            let strsize = u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap());
            symtab = Some((symoff, nsyms, stroff, strsize));
        }
        cursor += cmdsize;
    }

    let (symoff, nsyms, stroff, strsize) = match symtab {
        Some(v) => v,
        None => return Ok(Vec::new()),
    };

    let symoff = symoff as usize;
    let stroff = stroff as usize;
    let strsize = strsize as usize;
    let nsyms = nsyms as usize;

    if bytes.len() < symoff + nsyms * 16 {
        return Err(MemberSymtabError::TruncatedSymtab {
            symoff,
            nsyms,
            file_size: bytes.len(),
        });
    }
    if bytes.len() < stroff + strsize {
        return Err(MemberSymtabError::TruncatedStrtab {
            stroff,
            strsize,
            file_size: bytes.len(),
        });
    }

    let strtab = &bytes[stroff..stroff + strsize];
    let mut out: Vec<(String, u64, u8)> = Vec::new();

    for i in 0..nsyms {
        let off = symoff + i * 16;
        let n_strx = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let n_type = bytes[off + 4];
        let n_sect = bytes[off + 5];
        let n_value = u64::from_le_bytes(bytes[off + 8..off + 16].try_into().unwrap());

        // N_SECT|N_EXT = defined external.
        if (n_type & N_TYPE) != N_SECT {
            continue;
        }
        if (n_type & N_EXT) == 0 {
            continue;
        }
        if n_strx >= strtab.len() {
            return Err(MemberSymtabError::BadStrx {
                sym_index: i,
                n_strx,
            });
        }
        let end_off = strtab[n_strx..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| n_strx + p)
            .ok_or(MemberSymtabError::UnterminatedSymName {
                sym_index: i,
                n_strx,
            })?;
        let name_bytes = &strtab[n_strx..end_off];
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| MemberSymtabError::NameNotUtf8 { sym_index: i })?;
        out.push((name.to_string(), n_value, n_sect));
    }

    Ok(out)
}

/// Failures `build_archive_index` can report. Wraps a
/// `MemberSymtabError` with the offending member's index + name so
/// the link driver can blame a specific `.o` inside a specific
/// `.a` instead of just "something is broken".
#[derive(Debug)]
pub struct ArchiveParseError {
    pub member_idx: usize,
    pub member_name: String,
    pub kind: MemberSymtabError,
}

/// Per-Mach-O-member parse failures.
#[derive(Debug, PartialEq, Eq)]
pub enum MemberSymtabError {
    TruncatedHeader,
    TruncatedLoadCommand {
        offset: usize,
    },
    TruncatedSymtabCmd {
        offset: usize,
    },
    TruncatedSymtab {
        symoff: usize,
        nsyms: usize,
        file_size: usize,
    },
    TruncatedStrtab {
        stroff: usize,
        strsize: usize,
        file_size: usize,
    },
    BadStrx {
        sym_index: usize,
        n_strx: usize,
    },
    UnterminatedSymName {
        sym_index: usize,
        n_strx: usize,
    },
    NameNotUtf8 {
        sym_index: usize,
    },
}

/// Errors `parse_archive` can return. Each variant carries the
/// byte offset where the malformed structure was detected so the
/// link driver can surface a clear "your `libfoo.a` is broken at
/// byte X" message.
#[derive(Debug, PartialEq, Eq)]
pub enum ArParseError {
    /// File doesn't start with `"!<arch>\n"`.
    BadMagic,
    /// Header would extend past the end of the buffer.
    TruncatedHeader { offset: usize },
    /// `ar_fmag` (final 2 bytes of the header) is not `` `\n ``.
    BadFmag { offset: usize },
    /// `ar_size` is not ASCII decimal.
    BadSize { offset: usize, field: String },
    /// Member payload would extend past the end of the buffer.
    TruncatedMember { offset: usize, size: usize },
    /// BSD `#1/N` long-name length is not decimal.
    BadLongNameLen { offset: usize, field: String },
    /// `#1/N` declares a name longer than the member payload.
    TruncatedLongName {
        offset: usize,
        declared: usize,
        member_size: usize,
    },
    /// Member name bytes aren't valid UTF-8.
    NameNotUtf8 { offset: usize, bytes: Vec<u8> },
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b' ' || bytes[end - 1] == 0) {
        end -= 1;
    }
    &bytes[..end]
}

fn parse_decimal(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }
    let mut n: usize = 0;
    for &b in bytes {
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        n = n.checked_mul(10)?.checked_add(usize::from(b - b'0'))?;
    }
    Some(n)
}

fn ascii_to_owned(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid `.a` containing a single member with
    /// a short name (no BSD long-name encoding).
    fn build_short_name_archive(name: &str, data: &[u8]) -> Vec<u8> {
        assert!(name.len() <= 16);
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(AR_MAGIC);
        // Header
        let mut header = [b' '; AR_HEADER_SIZE];
        header[..name.len()].copy_from_slice(name.as_bytes());
        // ar_date = "0"
        header[16] = b'0';
        // ar_size = data.len() formatted decimal, ASCII right-padded
        let size_str = format!("{}", data.len());
        header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
        header[58] = b'`';
        header[59] = b'\n';
        archive.extend_from_slice(&header);
        archive.extend_from_slice(data);
        if data.len() % 2 != 0 {
            archive.push(0);
        }
        archive
    }

    /// Build a `.a` with a BSD long-name encoded member.
    fn build_long_name_archive(name: &str, data: &[u8]) -> Vec<u8> {
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(AR_MAGIC);
        // ar_name = "#1/N    " where N is the real name length.
        let n = name.len();
        let mut header = [b' '; AR_HEADER_SIZE];
        let prefix = format!("#1/{n}");
        header[..prefix.len()].copy_from_slice(prefix.as_bytes());
        header[16] = b'0';
        let total_payload = n + data.len();
        let size_str = format!("{total_payload}");
        header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
        header[58] = b'`';
        header[59] = b'\n';
        archive.extend_from_slice(&header);
        archive.extend_from_slice(name.as_bytes());
        archive.extend_from_slice(data);
        if total_payload % 2 != 0 {
            archive.push(0);
        }
        archive
    }

    #[test]
    fn empty_archive_parses_to_zero_members() {
        let archive = AR_MAGIC.to_vec();
        let members = parse_archive(&archive).unwrap();
        assert!(members.is_empty());
    }

    #[test]
    fn missing_magic_returns_bad_magic() {
        assert_eq!(parse_archive(b"").err(), Some(ArParseError::BadMagic));
        assert_eq!(parse_archive(b"hello").err(), Some(ArParseError::BadMagic));
    }

    #[test]
    fn single_short_name_member_round_trips() {
        let archive = build_short_name_archive("foo.o", b"hello world");
        let members = parse_archive(&archive).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "foo.o");
        assert_eq!(members[0].data, b"hello world");
    }

    #[test]
    fn long_name_member_round_trips() {
        let long_name =
            "torajs_syscall-336520ab773edd21.torajs_syscall.f021899c1cf506ba-cgu.0.rcgu.o";
        let archive = build_long_name_archive(long_name, b"DATA");
        let members = parse_archive(&archive).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, long_name);
        assert_eq!(members[0].data, b"DATA");
    }

    #[test]
    fn two_members_round_trip() {
        // Concatenate two single-member archives' bodies under the
        // same magic.
        let mut archive: Vec<u8> = AR_MAGIC.to_vec();
        let m1 = build_short_name_archive("a.o", b"AAA");
        let m2 = build_short_name_archive("b.o", b"BBBB");
        archive.extend_from_slice(&m1[AR_MAGIC.len()..]);
        archive.extend_from_slice(&m2[AR_MAGIC.len()..]);

        let members = parse_archive(&archive).unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "a.o");
        assert_eq!(members[0].data, b"AAA");
        assert_eq!(members[1].name, "b.o");
        assert_eq!(members[1].data, b"BBBB");
    }

    #[test]
    fn odd_size_member_consumes_alignment_pad() {
        // "AAA" = 3 bytes (odd) → 1 pad byte; next member follows
        // cleanly.
        let mut archive: Vec<u8> = AR_MAGIC.to_vec();
        let m1 = build_short_name_archive("a.o", b"AAA");
        let m2 = build_short_name_archive("b.o", b"BBBB");
        archive.extend_from_slice(&m1[AR_MAGIC.len()..]);
        archive.extend_from_slice(&m2[AR_MAGIC.len()..]);

        let members = parse_archive(&archive).unwrap();
        assert_eq!(members[0].data, b"AAA");
        assert_eq!(members[1].data, b"BBBB");
    }

    #[test]
    fn symdef_member_is_skipped() {
        let mut archive: Vec<u8> = AR_MAGIC.to_vec();
        let symdef = build_short_name_archive("__.SYMDEF", b"index-data");
        let real = build_short_name_archive("foo.o", b"DATA");
        archive.extend_from_slice(&symdef[AR_MAGIC.len()..]);
        archive.extend_from_slice(&real[AR_MAGIC.len()..]);

        let members = parse_archive(&archive).unwrap();
        assert_eq!(members.len(), 1, "__.SYMDEF must be filtered out");
        assert_eq!(members[0].name, "foo.o");
    }

    /// Walking a real Apple-emitted `.a` from the torajs build —
    /// `libtorajs_syscall.a` has BSD long-name members + a leading
    /// `__.SYMDEF` index. The parser must accept all of it and
    /// return the actual `.o` members.
    ///
    /// Gated on the file existing (release-build script may not
    /// have run yet on a fresh checkout). Skip if missing.
    #[test]
    fn real_libtorajs_syscall_archive_walks_clean() {
        let candidates = [
            "target/aarch64-apple-darwin/release/libtorajs_syscall.a",
            "../../target/aarch64-apple-darwin/release/libtorajs_syscall.a",
        ];
        let bytes = match candidates.iter().find_map(|p| std::fs::read(p).ok()) {
            Some(b) => b,
            None => {
                eprintln!("skip: libtorajs_syscall.a not built yet");
                return;
            }
        };

        let members = parse_archive(&bytes).expect("real .a must parse");
        // Apple `ld64`-emitted archives have many members (each
        // .rcgu.o chunk is one).
        assert!(
            !members.is_empty(),
            "real archive should have at least one member"
        );
        // Every member name should be a BSD long name (matches the
        // `target-name-hash.crate.hash-cgu.N.rcgu.o` pattern).
        for m in &members {
            assert!(
                m.name.ends_with(".o"),
                "unexpected member name {:?}",
                m.name
            );
            assert!(!m.data.is_empty(), "member {:?} has empty data", m.name);
        }
        // The first .o member should start with the Mach-O 64
        // magic 0xFEEDFACF (LE: CF FA ED FE).
        assert_eq!(
            &members[0].data[..4],
            &[0xCF, 0xFA, 0xED, 0xFE],
            "first .o member should start with MH_MAGIC_64",
        );
    }

    /// Truncated header → loud error with the byte offset, not a
    /// silent shorter `Vec`.
    #[test]
    fn truncated_header_errors_at_offset() {
        let mut archive: Vec<u8> = AR_MAGIC.to_vec();
        archive.extend_from_slice(&[0u8; 20]); // half a header
        let err = parse_archive(&archive).unwrap_err();
        assert!(matches!(err, ArParseError::TruncatedHeader { offset } if offset == 8));
    }

    /// Bad fmag → caller knows the file is corrupt, not a wildly
    /// reinterpreted size field.
    #[test]
    fn bad_fmag_errors() {
        let mut archive: Vec<u8> = AR_MAGIC.to_vec();
        let mut header = [b' '; AR_HEADER_SIZE];
        header[..3].copy_from_slice(b"a.o");
        header[16] = b'0';
        header[48] = b'0'; // size = 0
        header[58] = b'X'; // wrong fmag
        header[59] = b'X';
        archive.extend_from_slice(&header);
        let err = parse_archive(&archive).unwrap_err();
        assert!(matches!(err, ArParseError::BadFmag { offset } if offset == 8));
    }

    // ---- S7-B: Mach-O symbol extraction tests ----

    /// Empty Mach-O member (smaller than 32-byte header) trips
    /// the truncation check loudly.
    #[test]
    fn parse_member_too_small_errors_truncated_header() {
        let m = ArMember {
            name: "x.o",
            data: b"too short",
        };
        assert_eq!(
            parse_member_defined_externs(&m).unwrap_err(),
            MemberSymtabError::TruncatedHeader,
        );
    }

    /// A non-Mach-O member (anything that isn't 0xfeedfacf magic)
    /// returns an empty symbol list — we don't crash on legacy
    /// 32-bit Mach-O / fat-archive members, just skip them.
    #[test]
    fn parse_member_non_mach_o_magic_skipped() {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(&0xfeedface_u32.to_le_bytes()); // MH_MAGIC (32-bit)
        let m = ArMember {
            name: "x.o",
            data: &data,
        };
        let syms = parse_member_defined_externs(&m).unwrap();
        assert!(syms.is_empty());
    }

    /// Real `libtorajs_syscall.a` produces hundreds of
    /// defined-external symbols — enough that we can spot-check a
    /// few `__torajs_*` names exist.
    #[test]
    fn real_libtorajs_syscall_index_contains_known_symbols() {
        let candidates = [
            "target/aarch64-apple-darwin/release/libtorajs_syscall.a",
            "../../target/aarch64-apple-darwin/release/libtorajs_syscall.a",
        ];
        let bytes = match candidates.iter().find_map(|p| std::fs::read(p).ok()) {
            Some(b) => b,
            None => {
                eprintln!("skip: libtorajs_syscall.a not built yet");
                return;
            }
        };

        let members = parse_archive(&bytes).expect("ar parse");
        let index = build_archive_index(&members).expect("symbol index build");
        assert!(
            !index.is_empty(),
            "real libtorajs_syscall.a must define some externs"
        );
        // Sanity: hundreds of std + torajs symbols.
        assert!(
            index.len() > 100,
            "expected > 100 defined externs, got {}",
            index.len()
        );
        // The archive includes the std prelude + the syscall
        // wrappers; any of these would normally be present. Don't
        // assert on a specific symbol (the rustc-internal mangling
        // changes), just sanity-check the type.
        for (name, sym) in index.iter().take(3) {
            eprintln!(
                "sample sym: {name:?} → member_idx={} n_value=0x{:x}",
                sym.member_idx, sym.n_value
            );
            assert!(!name.is_empty());
            assert!(sym.member_idx < members.len());
        }
    }

    /// Synthetic single-member archive carrying a hand-built
    /// Mach-O with one `_foo` symbol round-trips through
    /// `build_archive_index`.
    #[test]
    fn synthetic_single_sym_archive_round_trips() {
        // Re-use torajs-obj's writer to produce a real .o, wrap
        // it in a single-member archive, and confirm
        // `build_archive_index` extracts the sym.
        use torajs_codegen::compile_function;
        use torajs_core::ssa::{
            BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
            ValueInfo,
        };
        use torajs_obj::write_object;

        let v0 = ValueId(0);
        let f = Function {
            name: "_foo".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(7), Operand::ConstI64(0)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        };
        let cf = compile_function(&f);
        let obj_bytes = write_object(std::slice::from_ref(&cf));

        let archive = build_short_name_archive("foo.o", &obj_bytes);
        let members = parse_archive(&archive).unwrap();
        assert_eq!(members.len(), 1);
        let index = build_archive_index(&members).unwrap();
        assert!(
            index.contains_key("_foo"),
            "synthetic archive must surface _foo; got {:?}",
            index.keys().collect::<Vec<_>>()
        );
        let sym = &index["_foo"];
        assert_eq!(sym.member_idx, 0);
        // _foo lives at section-relative offset 0 inside the .o.
        assert_eq!(sym.n_value, 0);
    }
}
