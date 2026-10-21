//! Which archive members a program can ever pull into its link,
//! decided when the image is built so the rest is never embedded.
//!
//! The linker pulls a member only through its symbol closure: it looks
//! up each extern name a program's relocs name in the merged
//! first-archive-wins index, pulls the member that wins the name, then
//! does the same for that member's undefined externs. The names a
//! program can put in a reloc are globals the torajs runtime crates
//! define and unmangled names (libc shims, compiler-builtins helpers,
//! dyld imports). A Rust-mangled std / core name carries a crate hash
//! the compiler never sees, so no program names one directly. The
//! closure of those roots therefore holds every member any program can
//! pull, and a member outside it is never linked.

use std::collections::{BTreeMap, HashSet};

use crate::archive::{ArMember, ArchiveParseError, MemberSymtabError};
use crate::archive_undef::member_undef_extern_names;
use crate::member_name::MemberName;

/// `(archive_idx, member_idx, n_value)` of the member winning a name.
pub(crate) type Winners<'a> = BTreeMap<&'a str, (u32, u32, u64)>;

/// A member's symtab failed to parse, blamed on its archive.
#[derive(Debug)]
pub(crate) struct PruneError {
    pub archive_idx: usize,
    pub err: ArchiveParseError,
}

/// Whether a program's relocs can carry `sym`, given the member that
/// wins it: any global a torajs runtime crate's member
/// (`torajs_rc-<hash>.torajs_rc.<cgu>.rcgu.o`) defines, and any
/// unmangled global. A Rust-mangled name defined elsewhere (std, core,
/// alloc, ...) carries a crate hash the compiler never sees.
pub fn nameable_by_program(sym: &str, winner_member: MemberName<'_>) -> bool {
    winner_member.starts_with("torajs") || !is_rust_mangled(sym)
}

/// Legacy (`_ZN<len>...`) and v0 (`_R<path>`) Rust symbol manglings,
/// after the Mach-O leading underscore.
pub fn is_rust_mangled(sym: &str) -> bool {
    let Some(s) = sym.strip_prefix('_') else {
        return false;
    };
    let b = s.as_bytes();
    match b {
        [b'_', b'Z', b'N', c, ..] => c.is_ascii_digit(),
        [b'_', b'R', c, ..] => c.is_ascii_uppercase() || c.is_ascii_digit(),
        _ => false,
    }
}

/// `live[a][m]` = member `m` of archive `a` is in the closure of every
/// name a program can reference.
pub(crate) fn live_members(
    parsed: &[Vec<ArMember<'_>>],
    winners: &Winners<'_>,
) -> Result<Vec<Vec<bool>>, PruneError> {
    let mut live: Vec<Vec<bool>> = parsed.iter().map(|ms| vec![false; ms.len()]).collect();
    let mut worklist: Vec<&str> = winners
        .iter()
        .filter(|(n, (a, m, _))| nameable_by_program(n, parsed[*a as usize][*m as usize].name))
        .map(|(n, _)| *n)
        .collect();

    let mut seen: HashSet<&str> = HashSet::new();
    while let Some(name) = worklist.pop() {
        if !seen.insert(name) {
            continue;
        }
        let Some(&(a, m, _)) = winners.get(name) else {
            continue;
        };
        let (a, m) = (a as usize, m as usize);
        if live[a][m] {
            continue;
        }
        live[a][m] = true;
        let member = &parsed[a][m];
        let undefs = member_undef_extern_names(member).map_err(|e| blame(a, m, member, e))?;
        worklist.extend(undefs);
    }
    Ok(live)
}

fn blame(
    archive_idx: usize,
    member_idx: usize,
    m: &ArMember<'_>,
    kind: MemberSymtabError,
) -> PruneError {
    PruneError {
        archive_idx,
        err: ArchiveParseError {
            member_idx,
            member_name: m.name.to_string(),
            kind,
        },
    }
}
