//! The reach closure's graph in the form it walks: each expanded
//! node's out-edges decoded once per link (the closure's own visited
//! set and name answers are in `dead_strip_reach_seen`).
//!
//! An edge to a node of the same member depends only on the member
//! bytes, so it is decoded at the first expansion and kept for every
//! later closure of the link (the elide fix-point runs several); the
//! edges of an atom of a member the runtime image stores an out-edge
//! table for are read from that table instead of from the relocs
//! (`dead_strip_reach_table_edges`). An import the image's symbol
//! index holds becomes the row of its name there; any other undefined
//! name stays a name. Whether a user fn, a dyld import or a required
//! member answers either is the closure's own question, asked once per
//! symbol per closure.

use std::ops::Range;

use torajs_codegen::id_hash::HashMap;

use crate::archive::ArMember;
use crate::dead_strip_reach::MemberReach;
use crate::dead_strip_reach_cache::BytesKey;
use crate::dead_strip_reach_relocs::RelocIndex;
use crate::dead_strip_reach_resolve::{RelocTarget, decode_reloc};
use crate::dead_strip_reach_table::{Import, ImportRef};
use crate::dead_strip_reach_table_edges::{TableEdge, TableEdges};

/// One out-edge.
#[derive(Clone, Copy)]
pub(crate) enum Edge {
    /// `atom: None` is the whole section.
    Local { sect: u8, atom: Option<u32> },
    /// An import, by the row of its name in the image's symbol index.
    Sym(u32),
    /// Any other undefined name, by the id `EdgeIndex::name` reads
    /// back.
    Name(u32),
}

/// Where a decoded edge list sits in the arena: `(start, len)`.
type Span = (u32, u32);

/// The span of a list not decoded yet.
const UNDECODED: Span = (0, u32::MAX);

/// One section's decoded out-edges: per atom (empty until the
/// section's first atom is expanded), and the whole section's.
type SectEdges = (Vec<Span>, Span);

/// What one reloc entry reaches, as far as the member's own bytes and
/// imports tell.
#[derive(Clone, Copy)]
pub(crate) enum Decoded<'a> {
    Local {
        sect: u8,
        atom: Option<u32>,
    },
    /// An import the image's symbol index holds, by its row there.
    Sym(u32),
    /// An import the index lacks: its position among the member's
    /// imports, and its name.
    Foreign(usize, &'a str),
    /// An undefined row the member does not list as an import.
    Unlisted(&'a str),
}

/// Decode the relocs of section `sect` with `lo <= r_address < hi`
/// (section-relative), in table order. `imports` are the member's, in
/// nlist order.
pub(crate) fn decode_edges<'a>(
    relocs: &mut RelocIndex<'a>,
    member: &ArMember<'a>,
    r: &MemberReach<'a>,
    imports: &[Import<'a>],
    (sect, lo, hi): (u8, u64, u64),
    mut f: impl FnMut(Decoded<'a>),
) -> Result<(), String> {
    for e in relocs.in_range(member, sect, lo, hi)? {
        match decode_reloc(sect, e, member, r) {
            Some(RelocTarget::Local { sect, atom }) => f(Decoded::Local { sect, atom }),
            Some(RelocTarget::Undef(row, name)) => {
                let k = imports.binary_search_by_key(&row, |i| i.row).ok();
                f(match k.map(|k| (k, imports[k].to)) {
                    Some((_, ImportRef::Sym(s))) => Decoded::Sym(s),
                    Some((k, ImportRef::Foreign(_))) => Decoded::Foreign(k, name),
                    None => Decoded::Unlisted(name),
                });
            }
            None => {}
        }
    }
    Ok(())
}

/// The edges a member's image table stores, with its foreign imports
/// once an edge names one.
struct Stored<'a> {
    /// The member's out-edge table, not empty.
    tab: &'a [u8],
    /// `tab` read, from the first atom the closure expands on (until
    /// then the closure does not touch the table's pages).
    edges: Option<TableEdges<'a>>,
    /// Each foreign import's name and name id ([`NO_ID`] until an edge
    /// names it), in import order; empty until an edge names one.
    foreign: Vec<(&'a str, u32)>,
}

/// A foreign import's name id not entered yet.
const NO_ID: u32 = u32::MAX;

#[derive(Default)]
pub(crate) struct EdgeIndex<'a> {
    relocs: RelocIndex<'a>,
    /// The slot in `members` of each member's bytes.
    slots: HashMap<BytesKey<'a>, u32>,
    /// Per member slot, per section.
    members: Vec<Vec<SectEdges>>,
    /// Per member slot.
    stored: Vec<Option<Stored<'a>>>,
    /// Whether every list is decoded from the relocs, for the
    /// fallback diag (`None` until a slot asks).
    decode_all: Option<bool>,
    /// Every decoded list, one after another.
    arena: Vec<Edge>,
    ids: HashMap<&'a str, u32>,
    names: Vec<&'a str>,
}

impl<'a> EdgeIndex<'a> {
    /// The slot of the edges of `member`, whose tables are `r` and whose
    /// image out-edge table is `edges` (empty: none).
    pub(crate) fn slot(
        &mut self,
        member: &ArMember<'a>,
        r: &MemberReach<'a>,
        edges: &'a [u8],
    ) -> u32 {
        let next = self.members.len() as u32;
        let slot = *self.slots.entry(BytesKey(member.data)).or_insert(next);
        if slot == next {
            self.members
                .push(vec![(Vec::new(), UNDECODED); r.sects.len()]);
            let decode_all = *self.decode_all.get_or_insert_with(|| {
                std::env::var_os("TORAJS_LINK_REACH_FALLBACK_DIAG").is_some()
            });
            let stored = (!edges.is_empty() && !decode_all).then(|| Stored {
                tab: edges,
                edges: None,
                foreign: Vec::new(),
            });
            self.stored.push(stored);
        }
        slot
    }

    /// The out-edges of `atom` of section `sect` (`None` = every reloc
    /// of the section) of the member in `slot`, in reloc-table order,
    /// as positions [`Self::edge`] reads. `imports` are the member's
    /// imports, in nlist order.
    pub(crate) fn out_edges(
        &mut self,
        slot: u32,
        member: &ArMember<'a>,
        r: &MemberReach<'a>,
        imports: &[Import<'a>],
        sect: u8,
        atom: Option<u32>,
    ) -> Result<Range<usize>, String> {
        let sa = r.sects.get(sect as usize - 1).ok_or("bad sect")?;
        let (atoms, whole) = &self.members[slot as usize][sect as usize - 1];
        let span = match atom {
            Some(a) => atoms.get(a as usize).copied().unwrap_or(UNDECODED),
            None => *whole,
        };
        if span != UNDECODED {
            let start = span.0 as usize;
            return Ok(start..start + span.1 as usize);
        }
        // r_address is section-relative; atom ranges are object-file
        // addresses.
        let (lo, hi) = match atom {
            Some(a) => {
                let (s, e) = sa.atoms[a as usize];
                (s - sa.addr, e - sa.addr)
            }
            None => (0, sa.size),
        };
        let start = self.arena.len();
        match (atom, self.stored[slot as usize].as_mut()) {
            (Some(a), Some(st)) => {
                let (arena, ids, names) = (&mut self.arena, &mut self.ids, &mut self.names);
                let tab = st.tab;
                let edges = *st.edges.get_or_insert_with(|| {
                    TableEdges::of(tab).expect("a stored edge table is not empty")
                });
                for e in edges.atom(sect, a) {
                    arena.push(match e {
                        TableEdge::Local { sect, atom } => Edge::Local { sect, atom },
                        TableEdge::Sym(row) => Edge::Sym(row),
                        TableEdge::Foreign(k) => {
                            Edge::Name(foreign_id(&mut st.foreign, k, imports, ids, names))
                        }
                    });
                }
            }
            _ => {
                let (arena, ids, names) = (&mut self.arena, &mut self.ids, &mut self.names);
                decode_edges(&mut self.relocs, member, r, imports, (sect, lo, hi), |d| {
                    arena.push(match d {
                        Decoded::Local { sect, atom } => Edge::Local { sect, atom },
                        Decoded::Sym(s) => Edge::Sym(s),
                        Decoded::Foreign(_, name) | Decoded::Unlisted(name) => {
                            Edge::Name(name_id(ids, names, name))
                        }
                    })
                })?;
            }
        }
        let end = self.arena.len();
        let span = (span_u32(start)?, span_u32(end - start)?);
        let (atoms, whole) = &mut self.members[slot as usize][sect as usize - 1];
        match atom {
            Some(a) => {
                atoms.resize(sa.atoms.len(), UNDECODED);
                atoms[a as usize] = span;
            }
            None => *whole = span,
        }
        Ok(start..end)
    }

    /// The edge at position `k` of a list [`Self::out_edges`] gave.
    pub(crate) fn edge(&self, k: usize) -> Edge {
        self.arena[k]
    }

    pub(crate) fn name(&self, id: u32) -> &'a str {
        self.names[id as usize]
    }
}

/// The name id of the `k`-th of `imports` the image's symbol index
/// lacks, entered the first time an edge names it.
fn foreign_id<'a>(
    foreign: &mut Vec<(&'a str, u32)>,
    k: u32,
    imports: &[Import<'a>],
    ids: &mut HashMap<&'a str, u32>,
    names: &mut Vec<&'a str>,
) -> u32 {
    if foreign.is_empty() {
        foreign.extend(imports.iter().filter_map(|i| match i.to {
            ImportRef::Foreign(name) => Some((name, NO_ID)),
            ImportRef::Sym(_) => None,
        }));
    }
    let (name, id) = foreign[k as usize];
    if id != NO_ID {
        return id;
    }
    let id = name_id(ids, names, name);
    foreign[k as usize].1 = id;
    id
}

/// The id of undefined name `name`, `names` reading it back.
fn name_id<'a>(ids: &mut HashMap<&'a str, u32>, names: &mut Vec<&'a str>, name: &'a str) -> u32 {
    let next = names.len() as u32;
    let id = *ids.entry(name).or_insert(next);
    if id == next {
        names.push(name);
    }
    id
}

fn span_u32(v: usize) -> Result<u32, String> {
    u32::try_from(v).map_err(|_| "reach edge arena overflow".to_string())
}
