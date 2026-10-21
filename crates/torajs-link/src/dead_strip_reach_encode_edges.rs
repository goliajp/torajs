//! A member's out-edge table (layout in
//! [`crate::dead_strip_reach_table_edges`]): every atom's edges decoded
//! from the member's relocs by the walk's own decoder, built when `tr`
//! is compiled.

use torajs_codegen::id_hash::HashSet;

use crate::archive::ArMember;
use crate::dead_strip_reach_edges::decode_edges;
use crate::dead_strip_reach_relocs::RelocIndex;
use crate::dead_strip_reach_table::{ImportRef, reach_from_table, table_imports};
use crate::dead_strip_reach_table_edges::{NO_EDGES, encode_edge};

/// The out-edge table of `member`, whose reachability table is `tab`;
/// empty when its edges do not all pack.
pub(crate) fn encode_member_edges(member: &ArMember<'_>, tab: &[u8]) -> Vec<u8> {
    let Some(cols) = edge_columns(member, tab) else {
        return Vec::new();
    };
    let header = [cols.sects.len(), cols.edges.len()].map(|n| n as u32);
    let mut out = Vec::with_capacity(4 * (2 + cols.sects.len() + cols.edges.len() + cols.at.len()));
    for v in header
        .iter()
        .chain(&cols.sects)
        .chain(&cols.edges)
        .chain(&cols.at)
    {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// The `edge_sect`, `edges` and `edge_at` columns.
#[derive(Default)]
struct Columns {
    sects: Vec<u32>,
    edges: Vec<u32>,
    at: Vec<u32>,
}

/// The out-edge columns, `None` when an edge does not pack or a
/// section's relocs do not decode (a link meets that only if it expands
/// the atom, so it decodes the member itself).
fn edge_columns(member: &ArMember<'_>, tab: &[u8]) -> Option<Columns> {
    let r = reach_from_table(member, tab);
    let imports = table_imports(member, tab);
    let mut k = 0;
    let foreign_k: Vec<u32> = imports
        .iter()
        .map(|i| match i.to {
            ImportRef::Foreign(_) => {
                k += 1;
                k - 1
            }
            ImportRef::Sym(_) => u32::MAX,
        })
        .collect();
    let mut relocs = RelocIndex::default();
    let mut cols = Columns::default();
    let (mut bounds, mut list) = (Vec::new(), Vec::new());
    for (i, sa) in r.sects.iter().enumerate() {
        let sect = u8::try_from(i + 1).ok()?;
        let first = cols.edges.len();
        bounds.clear();
        for &(s, e) in sa.atoms.iter() {
            bounds.push(u32::try_from(cols.edges.len()).ok()?);
            // a zerofill section is never expanded past its liveness
            if sa.no_file_storage {
                continue;
            }
            let mut packs = true;
            let range = (sect, s - sa.addr, e - sa.addr);
            list.clear();
            decode_edges(
                &mut relocs,
                member,
                &r,
                &imports,
                range,
                |d| match encode_edge(d, &foreign_k) {
                    Some(v) => list.push(v),
                    None => packs = false,
                },
            )
            .ok()?;
            if !packs {
                return None;
            }
            keep_last(&list, &mut cols.edges);
        }
        if cols.edges.len() == first {
            cols.sects.push(NO_EDGES);
            continue;
        }
        bounds.push(u32::try_from(cols.edges.len()).ok()?);
        cols.sects.push(u32::try_from(cols.at.len()).ok()?);
        cols.at.extend_from_slice(&bounds);
    }
    Some(cols)
}

/// Append `list` to `out` with every edge but the last copy of each
/// dropped. The walk pushes an atom's edges in list order and pops the
/// last pushed first, so the copy pushed last is the one it expands; an
/// earlier copy is popped once its node is visited and skipped. Every
/// copy names the same node and records the same predecessor, so the
/// walk visits the same nodes in the same order without them.
pub(crate) fn keep_last(list: &[u32], out: &mut Vec<u32>) {
    let start = out.len();
    let mut seen = HashSet::default();
    out.extend(list.iter().rev().filter(|&&v| seen.insert(v)));
    out[start..].reverse();
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::archives_merge::merge_archive_indexes;
    use crate::dead_strip_reach_edges::{Decoded, decode_edges};
    use crate::dead_strip_reach_relocs::RelocIndex;
    use crate::dead_strip_reach_table::{ImportRef, image_table, reach_from_table, table_imports};
    use crate::dead_strip_reach_table_edges::{TableEdge, TableEdges, image_edges};
    use crate::runtime_prestrip::prestrip_image;

    /// An edge by what it names.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    enum Key<'a> {
        Local(u8, Option<u32>),
        Sym(u32),
        Name(&'a str),
    }

    #[test]
    fn only_the_last_copy_of_an_edge_stays() {
        let mut out = vec![9];
        super::keep_last(&[1, 2, 1, 3, 2, 2], &mut out);
        assert_eq!(out, [9, 1, 3, 2]);
    }

    /// Every atom of every member of the embedded runtime's image: the
    /// edges its table stores are the last copies of the ones its
    /// relocs decode to, in order.
    #[test]
    fn the_image_stores_what_the_relocs_decode_to() {
        let image = prestrip_image(torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE).unwrap();
        let merged = merge_archive_indexes(&image).unwrap();
        let (mut members, mut stored, mut decoded, mut edges) = (0, 0, 0, 0);
        for a in 0..merged.archive_count() {
            for m in 0..merged.member_count(a) {
                let member = merged.member(a, m);
                let tab = image_table(&merged, (a, m));
                members += 1;
                let Some(t) = TableEdges::of(image_edges(&merged, (a, m))) else {
                    continue;
                };
                stored += 1;
                let r = reach_from_table(&member, tab);
                let imports = table_imports(&member, tab);
                let names: Vec<&str> = imports
                    .iter()
                    .filter_map(|i| match i.to {
                        ImportRef::Foreign(n) => Some(n),
                        ImportRef::Sym(_) => None,
                    })
                    .collect();
                let mut relocs = RelocIndex::default();
                for (i, sa) in r.sects.iter().enumerate() {
                    let sect = (i + 1) as u8;
                    for (k, &(s, e)) in sa.atoms.iter().enumerate() {
                        let got: Vec<Key<'_>> = t
                            .atom(sect, k as u32)
                            .map(|e| match e {
                                TableEdge::Local { sect, atom } => Key::Local(sect, atom),
                                TableEdge::Sym(row) => Key::Sym(row),
                                TableEdge::Foreign(f) => Key::Name(names[f as usize]),
                            })
                            .collect();
                        if sa.no_file_storage {
                            assert!(got.is_empty(), "{}: zerofill edges", member.name);
                            continue;
                        }
                        let mut want = Vec::new();
                        let range = (sect, s - sa.addr, e - sa.addr);
                        decode_edges(&mut relocs, &member, &r, &imports, range, |d| {
                            want.push(match d {
                                Decoded::Local { sect, atom } => Key::Local(sect, atom),
                                Decoded::Sym(row) => Key::Sym(row),
                                Decoded::Foreign(_, name) => Key::Name(name),
                                Decoded::Unlisted(name) => panic!("unlisted import {name}"),
                            })
                        })
                        .unwrap();
                        decoded += want.len();
                        let mut seen = HashSet::new();
                        let mut last: Vec<Key<'_>> =
                            want.into_iter().rev().filter(|k| seen.insert(*k)).collect();
                        last.reverse();
                        assert_eq!(got, last, "{} sect {sect} atom {k}", member.name);
                        edges += got.len();
                    }
                }
            }
        }
        assert!(
            members > 100 && stored == members,
            "{stored} of {members} members"
        );
        assert!(
            edges > 10_000 && decoded > edges,
            "{edges} of {decoded} edges"
        );
    }
}
