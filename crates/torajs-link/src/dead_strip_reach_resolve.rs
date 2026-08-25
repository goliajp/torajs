//! Node resolution for the dead-strip reach graph — "which node
//! does this name / defined address / reloc entry make live" —
//! split from `dead_strip_reach.rs` under the 500-line file rule
//! (the reach fix-point is the consumer; this file owns the
//! resolver family and its atom-covering arithmetic).

use std::collections::{BTreeMap, BTreeSet};

use crate::archive::ArMember;
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
    let sym = merged.index.get(name)?;
    let key = (sym.archive_idx, sym.member_idx);
    let r = reach.get(&key)?;
    let &(n_sect, n_value) = r.defined.get(name)?;
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
    if let Some(sa) = sect_of(r, n_sect)
        && let Some(atom) = covering_atom(&sa.atoms, addr)
    {
        return Node::Atom {
            key,
            sect: n_sect,
            atom,
        };
    }
    Node::AllSect { key, sect: n_sect }
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
    defined_in_user: &BTreeSet<&str>,
    required: &RequiredMembers,
    unresolved: &mut BTreeSet<String>,
) -> Option<Node> {
    if e.r_extern == 1 {
        let entry = r.nlist.get(e.r_symbolnum as usize)?;
        if entry.n_type & N_TYPE == N_UNDF {
            let name = entry.name;
            if defined_in_user.contains(name) || required.dyld_imports.contains_key(name) {
                return None;
            }
            let node = node_for_global_name(name, merged, reach);
            if node.is_none() {
                unresolved.insert(name.to_string());
            }
            node
        } else if entry.n_type & N_TYPE == N_SECT {
            Some(node_for_addr(key, r, entry.n_sect, entry.n_value))
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
                return Some(node_for_addr(key, r, sect, addr));
            }
        }
        if std::env::var_os("TORAJS_LINK_REACH_FALLBACK_DIAG").is_some() {
            eprintln!(
                "[reach-fallback] member={} site_sect={site_sect} r_type={} r_len={} pcrel={} -> AllSect sect={sect}",
                member.name, e.r_type, e.r_length, e.r_pcrel
            );
        }
        Some(Node::AllSect { key, sect })
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
