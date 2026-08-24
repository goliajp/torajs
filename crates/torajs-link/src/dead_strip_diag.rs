//! S2 dead-strip blade 1 — symbol-granularity reachability
//! ACCOUNTING (RFC 20260824-s2-dead-strip). Zero effect on emitted
//! bytes: this module only *measures* how much of each required
//! member is actually reachable from the user program, so the strip
//! blades (2/3) have a ground-truth go/no-go number before any
//! layout change lands.
//!
//! Gated on `TORAJS_LINK_DEADSTRIP_DIAG` — when unset (production),
//! `compute_archive_layout_with_merged` never calls in here.
//!
//! Atom model (ld64 `-dead_strip` semantics): every rcgu.o carries
//! `MH_SUBSECTIONS_VIA_SYMBOLS`, so an atom is the byte range from
//! one defined symbol (local or extern — rustc's `l_anon.*` data
//! labels anchor densely) to the next, in EVERY section, not just
//! `__text`. Reachability edges are the member reloc tables:
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
//!     the report shows what this conservatism costs).
//!
//! Roots are the same extern seeds `compute_required_members` used
//! (user-fn reloc targets minus user-defined / link-defined /
//! dyld-resolved names).

use std::collections::{BTreeMap, BTreeSet};

use torajs_obj::{N_SECT, N_TYPE, N_UNDF, NLIST_64_SIZE};

use crate::archive::ArMember;
use crate::archives_merge::{MergedArchives, RequiredMembers, reloc_target_name};
use crate::exec::LinkConfig;
use crate::member_data_apply::parse_member_section_relocs;
use crate::member_reloc::MemberRelocEntry;
use crate::member_sections::{collect_member_sections, is_no_file_storage};
use crate::member_symtab::parse_member_symtab;

/// One parsed nlist row — just the fields reachability needs.
struct NlEntry<'a> {
    name: &'a str,
    n_type: u8,
    n_sect: u8,
    n_value: u64,
}

/// Atom table over one section: sorted `(start, end)` object-file
/// address ranges. A leading unnamed gap (section start → first
/// symbol) is its own atom so literal pools before the first label
/// stay addressable.
struct SectAtoms {
    addr: u64,
    size: u64,
    /// File offset of the section payload (meaningless when
    /// `no_file_storage`).
    file_off: u32,
    no_file_storage: bool,
    is_text: bool,
    atoms: Vec<(u64, u64)>,
    live: Vec<bool>,
    all_live: bool,
}

/// Per-member reachability state.
struct MemberReach<'a> {
    member_name: &'a str,
    /// Indexed by 1-based section ordinal − 1.
    sects: Vec<SectAtoms>,
    nlist: Vec<NlEntry<'a>>,
    /// Defined (N_SECT) symbols by name → (n_sect, n_value).
    defined: BTreeMap<&'a str, (u8, u64)>,
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

/// Entry point — called (env-gated) by
/// `compute_archive_layout_with_merged` after the member closure.
/// Diagnostic only: internal failures print one warning line and
/// return, they never fail the link.
pub(crate) fn report(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
    required: &RequiredMembers,
    extra_defined_syms: &BTreeSet<String>,
) {
    match run(cfg, merged, required, extra_defined_syms) {
        Ok(lines) => eprint!("{lines}"),
        Err(e) => eprintln!("[deadstrip-diag] FAILED: {e}"),
    }
}

fn run(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
    required: &RequiredMembers,
    extra_defined_syms: &BTreeSet<String>,
) -> Result<String, String> {
    let mut reach: BTreeMap<(usize, usize), MemberReach<'_>> = BTreeMap::new();
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

    Ok(render_report(&reach, &unresolved))
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

/// Format the accounting report.
fn render_report(
    reach: &BTreeMap<(usize, usize), MemberReach<'_>>,
    unresolved: &BTreeSet<String>,
) -> String {
    use std::fmt::Write;
    let (mut t_tot, mut t_live, mut d_tot, mut d_live) = (0u64, 0u64, 0u64, 0u64);
    let mut fallbacks = 0usize;
    let mut rows: Vec<(u64, u64, &str)> = Vec::new();
    for r in reach.values() {
        let (mut mt_tot, mut mt_live) = (0u64, 0u64);
        for sa in &r.sects {
            let live: u64 = sa
                .atoms
                .iter()
                .zip(&sa.live)
                .filter(|&(_, &l)| l)
                .map(|(&(s, e), _)| e - s)
                .sum();
            fallbacks += usize::from(sa.all_live);
            if sa.is_text {
                mt_tot += sa.size;
                mt_live += live;
            } else {
                d_tot += sa.size;
                d_live += live;
            }
        }
        t_tot += mt_tot;
        t_live += mt_live;
        rows.push((mt_tot - mt_live, mt_tot, r.member_name));
    }
    rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let mut out = String::new();
    let pct = |l: u64, t: u64| if t > 0 { l * 100 / t } else { 0 };
    let _ = writeln!(
        out,
        "[deadstrip-diag] members={} text_live={}/{} ({}%) other_live={}/{} ({}%) \
         sect_fallbacks={} unresolved={}",
        reach.len(),
        t_live,
        t_tot,
        pct(t_live, t_tot),
        d_live,
        d_tot,
        pct(d_live, d_tot),
        fallbacks,
        unresolved.len(),
    );
    let _ = writeln!(out, "[deadstrip-diag] top dead members (text dead/total):");
    for (dead, sz, name) in rows.iter().take(20) {
        let _ = writeln!(out, "[deadstrip-diag]   {dead:>9} / {sz:>9}  {name}");
    }
    for u in unresolved.iter().take(10) {
        let _ = writeln!(out, "[deadstrip-diag]   unresolved: {u}");
    }
    out
}

/// Trim trailing NUL/space padding off a raw 16-byte name slot.
fn trim16(raw: &[u8; 16]) -> &[u8] {
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
