//! Node resolution for the dead-strip reach graph — "which node
//! does this name / defined address / reloc entry make live" —
//! split from `dead_strip_reach.rs` under the 500-line file rule
//! (the reach fix-point is the consumer; this file owns the
//! resolver family and its atom-covering arithmetic).

use std::collections::{BTreeMap, BTreeSet};

use crate::archive::ArMember;
use crate::archive_link_extra_syms::UserDefined;
use crate::archives_merge::{MergedArchives, RequiredMembers};
use crate::dead_strip_reach::{MemberReach, Node, SectAtoms};
use crate::member_reloc::MemberRelocEntry;
use torajs_obj::{N_SECT, N_TYPE, N_UNDF};

/// Resolve a global symbol name to its owning node via the merged
/// index. `None` when the name isn't in any required member.
pub(crate) fn node_for_global_name(
    name: &str,
    merged: &MergedArchives<'_>,
    reach: &BTreeMap<(usize, usize), MemberReach<'_>>,
) -> Option<Node> {
    let sym = merged.symbol(name)?;
    let key = (sym.archive_idx, sym.member_idx);
    let r = reach.get(&key)?;
    let (n_sect, n_value) = r.defined(name)?;
    Some(node_for_addr(key, r, n_sect, n_value))
}

/// Defined address inside a known member → node (atom if covered,
/// whole section otherwise).
pub(crate) fn node_for_addr(
    key: (usize, usize),
    r: &MemberReach<'_>,
    n_sect: u8,
    addr: u64,
) -> Node {
    local_node(key, n_sect, atom_at(r, n_sect, addr))
}

/// The node for a section-local target: the atom when one covers the
/// address, the whole section otherwise.
pub(crate) fn local_node(key: (usize, usize), sect: u8, atom: Option<u32>) -> Node {
    match atom {
        Some(atom) => Node::Atom {
            key,
            sect,
            atom: atom as usize,
        },
        None => Node::AllSect { key, sect },
    }
}

fn atom_at(r: &MemberReach<'_>, n_sect: u8, addr: u64) -> Option<u32> {
    let sa = sect_of(r, n_sect)?;
    covering_atom(&sa.atoms, addr).map(|a| a as u32)
}

fn sect_of<'r, 'a>(r: &'r MemberReach<'a>, ord: u8) -> Option<&'r SectAtoms> {
    if ord == 0 {
        return None;
    }
    r.sects.get(ord as usize - 1)
}

/// Index of the atom whose `[start, end)` covers `addr`.
fn covering_atom(atoms: &[(u64, u64)], addr: u64) -> Option<usize> {
    let i = atoms.partition_point(|&(s, _)| s <= addr);
    if i == 0 {
        return None;
    }
    let (s, e) = atoms[i - 1];
    (addr >= s && addr < e).then_some(i - 1)
}

/// What one reloc entry points at as far as the member's own bytes
/// tell: a node of the same member, or an undefined name only the
/// closure's roots and required set can resolve.
#[derive(Clone, Copy)]
pub(crate) enum RelocTarget<'a> {
    /// `atom: None` is the whole section.
    Local {
        sect: u8,
        atom: Option<u32>,
    },
    Undef(&'a str),
}

/// Decode one reloc entry of section `site_sect` (`None` = no edge).
pub(crate) fn decode_reloc<'a>(
    site_sect: u8,
    e: &MemberRelocEntry,
    member: &ArMember<'_>,
    r: &MemberReach<'a>,
) -> Option<RelocTarget<'a>> {
    if e.r_extern == 1 {
        let entry = r.nlist.get(e.r_symbolnum as usize)?;
        if entry.n_type & N_TYPE == N_UNDF {
            Some(RelocTarget::Undef(entry.name))
        } else if entry.n_type & N_TYPE == N_SECT {
            Some(RelocTarget::Local {
                sect: entry.n_sect,
                atom: atom_at(r, entry.n_sect, entry.n_value),
            })
        } else {
            None
        }
    } else {
        // ARM64_RELOC_ADDEND is a MODIFIER row: it precedes its
        // mate (BRANCH26 / PAGE21 / PAGEOFF12) and its r_symbolnum
        // carries the ADDEND VALUE, not a section ordinal. Treating
        // it as a section-keyed reloc turned common addends (4, 8,
        // 16…) into AllSect fallbacks over arbitrary sections —
        // whole live-section sweeps with no real edge (the mate row
        // itself resolves the target). Skip it.
        if e.r_type == torajs_obj::ARM64_RELOC_ADDEND {
            return None;
        }
        let sect = e.r_symbolnum as u8;
        // UNSIGNED (8-byte, non-pcrel) section-keyed reloc: the
        // addend stored at the site IS the target object-file
        // address — read it and land on the covering atom.
        if e.r_type == 0
            && e.r_length == 3
            && e.r_pcrel == 0
            && let Some(site_sa) = sect_of(r, site_sect)
            && !site_sa.no_file_storage
        {
            let off = site_sa.file_off as usize + e.r_address as usize;
            if member.data.len() >= off + 8 {
                let addr = u64::from_le_bytes(member.data[off..off + 8].try_into().unwrap());
                return Some(RelocTarget::Local {
                    sect,
                    atom: atom_at(r, sect, addr),
                });
            }
        }
        if std::env::var_os("TORAJS_LINK_REACH_FALLBACK_DIAG").is_some() {
            eprintln!(
                "[reach-fallback] member={} site_sect={site_sect} r_type={} r_len={} pcrel={} -> AllSect sect={sect}",
                member.name, e.r_type, e.r_length, e.r_pcrel
            );
        }
        Some(RelocTarget::Local { sect, atom: None })
    }
}

/// An undefined name → the node it makes live: none when a user fn
/// defines it or dyld binds it; recorded as unresolved when no
/// required member defines it.
pub(crate) fn resolve_undef(
    name: &str,
    merged: &MergedArchives<'_>,
    reach: &BTreeMap<(usize, usize), MemberReach<'_>>,
    defined_in_user: &UserDefined<'_>,
    required: &RequiredMembers,
    unresolved: &mut BTreeSet<String>,
) -> Option<Node> {
    if defined_in_user.contains(name) || required.dyld_imports.contains_key(name) {
        return None;
    }
    let node = node_for_global_name(name, merged, reach);
    if node.is_none() {
        unresolved.insert(name.to_string());
    }
    node
}

/// One reloc entry → the node it makes live, if any.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_reloc_target(
    key: (usize, usize),
    site_sect: u8,
    e: &MemberRelocEntry,
    member: &ArMember<'_>,
    merged: &MergedArchives<'_>,
    r: &MemberReach<'_>,
    reach: &BTreeMap<(usize, usize), MemberReach<'_>>,
    defined_in_user: &UserDefined<'_>,
    required: &RequiredMembers,
    unresolved: &mut BTreeSet<String>,
) -> Option<Node> {
    match decode_reloc(site_sect, e, member, r)? {
        RelocTarget::Local { sect, atom } => Some(local_node(key, sect, atom)),
        RelocTarget::Undef(name) => {
            resolve_undef(name, merged, reach, defined_in_user, required, unresolved)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::covering_atom;

    #[test]
    fn covering_atom_boundaries() {
        let atoms = [(0u64, 8u64), (8, 24), (24, 40)];
        assert_eq!(covering_atom(&atoms, 0), Some(0));
        assert_eq!(covering_atom(&atoms, 7), Some(0));
        assert_eq!(covering_atom(&atoms, 8), Some(1));
        assert_eq!(covering_atom(&atoms, 39), Some(2));
        assert_eq!(covering_atom(&atoms, 40), None);
    }

    #[test]
    fn covering_atom_empty() {
        assert_eq!(covering_atom(&[], 0), None);
    }
}
