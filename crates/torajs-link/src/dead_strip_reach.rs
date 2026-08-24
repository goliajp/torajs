//! S2 dead-strip reachability pass — the ld64-style atom closure
//! extracted from `dead_strip_diag` (RFC 20260824-s2-dead-strip,
//! blade 2a) so the strip blades and the diag renderer consume one
//! shared ground truth.
//!
//! Atom model: every rcgu.o carries `MH_SUBSECTIONS_VIA_SYMBOLS`,
//! so an atom is the byte range from one defined symbol (local or
//! extern — rustc's `l_anon.*` data labels anchor densely) to the
//! next, in EVERY section. Reachability edges are the member reloc
//! tables:
//!
//!   - `r_extern=1` → the referenced nlist entry: an undef resolves
//!     through the merged archive index to another member's atom; a
//!     defined local/extern resolves inside this member.
//!   - `r_extern=0` UNSIGNED (r_length=3, !pcrel) → the 8-byte
//!     addend AT the reloc site is the target's object-file address
//!     — read it and land on the covering atom of the section
//!     `r_symbolnum` names.
//!   - any other `r_extern=0` (no symbol anchor, no readable
//!     addend) → the whole target section goes live (counted, so
//!     consumers can see what this conservatism costs).
//!
//! Roots are the same extern seeds `compute_required_members` used
//! (user-fn reloc targets minus user-defined / link-defined /
//! dyld-resolved names).

use std::collections::{BTreeMap, BTreeSet};

use torajs_obj::{
    N_NO_DEAD_STRIP, N_SECT, N_TYPE, N_UNDF, NLIST_64_SIZE, S_ATTR_LIVE_SUPPORT,
    S_ATTR_NO_DEAD_STRIP, S_MOD_INIT_FUNC_POINTERS, S_TERM_FUNC_POINTERS, S_THREAD_LOCAL_VARIABLES,
    SECTION_TYPE,
};

use crate::archive::ArMember;
use crate::archives_merge::{MergedArchives, RequiredMembers, reloc_target_name};
use crate::exec::LinkConfig;
use crate::member_data_apply::parse_member_section_relocs;
use crate::member_reloc::MemberRelocEntry;
use crate::member_sections::{collect_member_sections, is_no_file_storage};
use crate::member_symtab::parse_member_symtab;

/// One parsed nlist row — just the fields reachability needs.
pub(crate) struct NlEntry<'a> {
    pub(crate) name: &'a str,
    pub(crate) n_type: u8,
    pub(crate) n_sect: u8,
    pub(crate) n_desc: u16,
    pub(crate) n_value: u64,
}

/// Atom table over one section: sorted `(start, end)` object-file
/// address ranges. A leading unnamed gap (section start → first
/// symbol) is its own atom so literal pools before the first label
/// stay addressable.
pub(crate) struct SectAtoms {
    pub(crate) addr: u64,
    pub(crate) size: u64,
    /// File offset of the section payload (meaningless when
    /// `no_file_storage`).
    pub(crate) file_off: u32,
    pub(crate) no_file_storage: bool,
    /// Raw `section_64.flags` (type + attribute bits) — the strip
    /// driver reads the type whitelist and no-dead-strip attributes
    /// off it.
    pub(crate) flags: u32,
    /// Raw 16-byte `sectname` slot — the diag aggregates live mass
    /// per section name for the S2-5 registration-reach accounting.
    pub(crate) sectname: [u8; 16],
    pub(crate) is_text: bool,
    pub(crate) atoms: Vec<(u64, u64)>,
    pub(crate) live: Vec<bool>,
    pub(crate) all_live: bool,
}

/// Per-member reachability state.
pub(crate) struct MemberReach<'a> {
    pub(crate) member_name: &'a str,
    /// Indexed by 1-based section ordinal − 1.
    pub(crate) sects: Vec<SectAtoms>,
    pub(crate) nlist: Vec<NlEntry<'a>>,
    /// Defined (N_SECT) symbols by name → (n_sect, n_value).
    pub(crate) defined: BTreeMap<&'a str, (u8, u64)>,
}

/// Worklist node.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Atom {
        key: (usize, usize),
        sect: u8,
        atom: usize,
    },
    AllSect {
        key: (usize, usize),
        sect: u8,
    },
}

/// Closure output: per-member atom liveness plus the names that
/// never resolved (should be empty on a healthy link).
pub(crate) struct ReachResult<'a> {
    pub(crate) members: BTreeMap<(usize, usize), MemberReach<'a>>,
    pub(crate) unresolved: BTreeSet<String>,
}

/// Run the atom-granularity reachability closure over the required
/// members. Pure analysis — touches nothing the emit path reads.
pub(crate) fn compute_reachability<'a>(
    cfg: &LinkConfig,
    merged: &MergedArchives<'a>,
    required: &RequiredMembers,
    extra_defined_syms: &BTreeSet<String>,
) -> Result<ReachResult<'a>, String> {
    let mut reach: BTreeMap<(usize, usize), MemberReach<'a>> = BTreeMap::new();
    for &(a, m) in &required.members {
        let member = &merged.per_archive_members[a][m];
        reach.insert((a, m), build_member_reach(member)?);
    }

    // Roots: the same seed set compute_required_members walked from.
    let defined_in_user: BTreeSet<&str> = cfg
        .funcs
        .iter()
        .map(|f| f.name.as_str())
        .chain(extra_defined_syms.iter().map(|s| s.as_str()))
        .collect();
    let mut worklist: Vec<Node> = Vec::new();
    let mut unresolved: BTreeSet<String> = BTreeSet::new();
    for f in &cfg.funcs {
        for r in &f.relocs {
            if let Some(name) = reloc_target_name(&r.kind)
                && !defined_in_user.contains(name)
                && !required.dyld_imports.contains_key(name)
                && let Some(node) = node_for_global_name(name, merged, &reach)
            {
                worklist.push(node);
            }
        }
    }

    // ld64 dead-strip GC roots beyond the extern seeds: sections
    // flagged no-dead-strip / live-support, mod-init & term-func
    // pointer sections, TLV descriptor tables (the pipeline emits
    // their thunk slots unconditionally, and their entries point at
    // `__thread_data` initial values), and `#[used]`-style symbols
    // carrying N_NO_DEAD_STRIP in `n_desc`.
    for (&key, r) in &reach {
        for (i, sa) in r.sects.iter().enumerate() {
            let ty = sa.flags & SECTION_TYPE;
            if sa.flags & (S_ATTR_NO_DEAD_STRIP | S_ATTR_LIVE_SUPPORT) != 0
                || ty == S_MOD_INIT_FUNC_POINTERS
                || ty == S_TERM_FUNC_POINTERS
                || ty == S_THREAD_LOCAL_VARIABLES
            {
                worklist.push(Node::AllSect {
                    key,
                    sect: (i + 1) as u8,
                });
            }
        }
        for e in &r.nlist {
            if e.n_type & N_TYPE == N_SECT && e.n_desc & N_NO_DEAD_STRIP != 0 {
                worklist.push(node_for_addr(key, r, e.n_sect, e.n_value));
            }
        }
    }

    // Fixpoint.
    let mut visited: BTreeSet<Node> = BTreeSet::new();
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        expand_node(
            node,
            merged,
            &mut reach,
            &defined_in_user,
            required,
            &mut worklist,
            &mut unresolved,
        )?;
    }

    Ok(ReachResult {
        members: reach,
        unresolved,
    })
}

/// Parse one member into its reachability tables.
fn build_member_reach<'a>(member: &ArMember<'a>) -> Result<MemberReach<'a>, String> {
    let sections =
        collect_member_sections(member).map_err(|e| format!("{}: sections: {e:?}", member.name))?;

    let bytes = member.data;
    let mut nlist: Vec<NlEntry<'a>> = Vec::new();
    let mut defined: BTreeMap<&'a str, (u8, u64)> = BTreeMap::new();
    if bytes.len() >= 32 && !sections.is_empty() {
        let st = parse_member_symtab(member).map_err(|e| format!("{}: {e:?}", member.name))?;
        let symoff = st.symoff as usize;
        let stroff = st.stroff as usize;
        for i in 0..st.nsyms as usize {
            let off = symoff + i * NLIST_64_SIZE as usize;
            if bytes.len() < off + NLIST_64_SIZE as usize {
                return Err(format!("{}: nlist row {i} truncated", member.name));
            }
            let n_strx = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
            let n_type = bytes[off + 4];
            let n_sect = bytes[off + 5];
            let n_desc = u16::from_le_bytes(bytes[off + 6..off + 8].try_into().unwrap());
            let n_value = u64::from_le_bytes(bytes[off + 8..off + 16].try_into().unwrap());
            let name = if n_strx == 0 {
                ""
            } else {
                let s = stroff + n_strx;
                let end = bytes[s..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| s + p)
                    .unwrap_or(bytes.len());
                std::str::from_utf8(&bytes[s..end]).unwrap_or("")
            };
            if n_type & N_TYPE == N_SECT {
                defined.insert(name, (n_sect, n_value));
            }
            nlist.push(NlEntry {
                name,
                n_type,
                n_sect,
                n_desc,
                n_value,
            });
        }
    }

    // Per-section atom tables.
    let mut sects: Vec<SectAtoms> = Vec::with_capacity(sections.len());
    for s in &sections {
        let ord = s.section_index;
        let mut starts: Vec<u64> = nlist
            .iter()
            .filter(|e| e.n_type & N_TYPE == N_SECT && e.n_sect == ord)
            .map(|e| e.n_value)
            .collect();
        starts.sort_unstable();
        starts.dedup();
        // Leading unnamed gap becomes atom 0.
        if starts.first().is_none_or(|&f| f > s.member_addr) {
            starts.insert(0, s.member_addr);
        }
        let sect_end = s.member_addr + u64::from(s.size);
        let mut atoms: Vec<(u64, u64)> = Vec::with_capacity(starts.len());
        for (i, &st) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(sect_end);
            atoms.push((st, end));
        }
        sects.push(SectAtoms {
            addr: s.member_addr,
            size: u64::from(s.size),
            file_off: s.member_internal_offset,
            no_file_storage: is_no_file_storage(s.flags),
            flags: s.flags,
            sectname: s.sectname,
            is_text: trim16(&s.sectname) == b"__text" && trim16(&s.segname) == b"__TEXT",
            live: vec![false; atoms.len()],
            atoms,
            all_live: false,
        });
    }

    Ok(MemberReach {
        member_name: member.name,
        sects,
        nlist,
        defined,
    })
}

/// Resolve a global symbol name to its owning node via the merged
/// index. `None` when the name isn't in any required member.
fn node_for_global_name(
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
fn node_for_addr(key: (usize, usize), r: &MemberReach<'_>, n_sect: u8, addr: u64) -> Node {
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

/// Mark a node live and enqueue everything it references.
fn expand_node(
    node: Node,
    merged: &MergedArchives<'_>,
    reach: &mut BTreeMap<(usize, usize), MemberReach<'_>>,
    defined_in_user: &BTreeSet<&str>,
    required: &RequiredMembers,
    worklist: &mut Vec<Node>,
    unresolved: &mut BTreeSet<String>,
) -> Result<(), String> {
    let (key, sect_ord, range) = match node {
        Node::Atom { key, sect, atom } => {
            let r = reach.get_mut(&key).ok_or("member vanished")?;
            let sa = r.sects.get_mut(sect as usize - 1).ok_or("bad sect")?;
            sa.live[atom] = true;
            (key, sect, Some(sa.atoms[atom]))
        }
        Node::AllSect { key, sect } => {
            if sect == 0 {
                return Ok(());
            }
            let r = reach.get_mut(&key).ok_or("member vanished")?;
            let Some(sa) = r.sects.get_mut(sect as usize - 1) else {
                return Ok(());
            };
            sa.all_live = true;
            for b in sa.live.iter_mut() {
                *b = true;
            }
            (key, sect, None)
        }
    };

    let member = &merged.per_archive_members[key.0][key.1];
    let (no_file, sect_addr, sect_size) = {
        let r = reach.get(&key).ok_or("member vanished")?;
        let sa = r.sects.get(sect_ord as usize - 1).ok_or("bad sect")?;
        (sa.no_file_storage, sa.addr, sa.size)
    };
    if no_file {
        return Ok(()); // zerofill: no payload, no relocs
    }
    let relocs = parse_member_section_relocs(member, sect_ord)
        .map_err(|e| format!("sect {sect_ord} relocs: {e:?}"))?;
    // r_address is section-relative; atom ranges are object-file
    // addresses.
    let (lo, hi) = match range {
        Some((s, e)) => (s - sect_addr, e - sect_addr),
        None => (0, sect_size),
    };
    for e in relocs
        .iter()
        .filter(|e| u64::from(e.r_address) >= lo && u64::from(e.r_address) < hi)
    {
        let r = reach.get(&key).ok_or("member vanished")?;
        if let Some(next) = resolve_reloc_target(
            key,
            sect_ord,
            e,
            member,
            merged,
            r,
            reach,
            defined_in_user,
            required,
            unresolved,
        ) {
            worklist.push(next);
        }
    }
    Ok(())
}

/// One reloc entry → the node it makes live, if any.
#[allow(clippy::too_many_arguments)]
fn resolve_reloc_target(
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
        Some(Node::AllSect { key, sect })
    }
}

/// Trim trailing NUL/space padding off a raw 16-byte name slot.
pub(crate) fn trim16(raw: &[u8; 16]) -> &[u8] {
    let end = raw
        .iter()
        .position(|&b| b == 0 || b == b' ')
        .unwrap_or(raw.len());
    &raw[..end]
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
