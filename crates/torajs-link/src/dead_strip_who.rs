//! S2-5 who-references query — the reverse of the why-live chain.
//! Why-live shows the ONE first-recorded edge that made an atom
//! live; on a densely redundant graph that under-tells the story
//! (r490: cutting a "sole artery" priced at zero because sibling
//! edges kept every target alive). This census walks EVERY live
//! atom's relocs after the closure and lists ALL edges into atoms
//! whose defined symbols match a pattern — the complete in-edge
//! set an attack would have to sever. Diag-only, post-closure,
//! zero effect on emitted bytes.

use std::collections::BTreeSet;

use crate::archives_merge::{MergedArchives, RequiredMembers};
use crate::dead_strip_reach::{Node, ReachResult, node_is_cut, resolve_reloc_target};
use crate::exec::LinkConfig;
use crate::member_data_apply::parse_member_section_relocs;

/// All `(source, target)` edges from live atoms into pattern-matched
/// atoms. Sources are deduplicated per (source, target) pair.
pub(crate) fn who_census(
    cfg: &LinkConfig,
    merged: &MergedArchives<'_>,
    required: &RequiredMembers,
    extra_defined_syms: &BTreeSet<String>,
    r: &ReachResult<'_>,
    pats: &[String],
) -> Vec<(Node, Node)> {
    let defined_in_user: BTreeSet<&str> = cfg
        .funcs
        .iter()
        .map(|f| f.name.as_str())
        .chain(extra_defined_syms.iter().map(|s| s.as_str()))
        .collect();
    let mut edges: BTreeSet<(Node, Node)> = BTreeSet::new();
    let mut scratch: BTreeSet<String> = BTreeSet::new();
    // Target set first — cheap membership test per resolved reloc.
    let mut targets: BTreeSet<Node> = BTreeSet::new();
    for (&key, m) in &r.members {
        for (si, sa) in m.sects.iter().enumerate() {
            for ai in 0..sa.atoms.len() {
                let node = Node::Atom {
                    key,
                    sect: (si + 1) as u8,
                    atom: ai,
                };
                if node_is_cut(&r.members, node, pats) {
                    targets.insert(node);
                }
            }
        }
    }
    if targets.is_empty() {
        return Vec::new();
    }
    for (&key, m) in &r.members {
        let member = &merged.per_archive_members[key.0][key.1];
        for (si, sa) in m.sects.iter().enumerate() {
            if sa.no_file_storage {
                continue;
            }
            let sect_ord = (si + 1) as u8;
            let Ok(relocs) = parse_member_section_relocs(member, sect_ord) else {
                continue;
            };
            for e in &relocs {
                let addr = sa.addr + u64::from(e.r_address);
                let i = sa.atoms.partition_point(|&(s, _)| s <= addr);
                if i == 0 || !sa.live[i - 1] {
                    continue; // dead source atoms don't count
                }
                let src = Node::Atom {
                    key,
                    sect: sect_ord,
                    atom: i - 1,
                };
                if let Some(tgt) = resolve_reloc_target(
                    key,
                    sect_ord,
                    e,
                    member,
                    merged,
                    m,
                    &r.members,
                    &defined_in_user,
                    required,
                    &mut scratch,
                ) && targets.contains(&tgt)
                    && src != tgt
                {
                    edges.insert((src, tgt));
                }
            }
        }
    }
    edges.into_iter().collect()
}
