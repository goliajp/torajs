//! The reach closure's graph in the form it walks: each expanded
//! node's out-edges decoded once per link (the closure's own visited
//! set and name answers are in `dead_strip_reach_seen`).
//!
//! An edge to a node of the same member depends only on the member
//! bytes, so it is decoded at the first expansion and kept for every
//! later closure of the link (the elide fix-point runs several). An
//! import the runtime image's symbol index holds becomes the row of its
//! name there; any other undefined name stays a name. Whether a user
//! fn, a dyld import or a required member answers either is the
//! closure's own question, asked once per symbol per closure.

use std::ops::Range;

use torajs_codegen::id_hash::HashMap;

use crate::archive::ArMember;
use crate::dead_strip_reach::MemberReach;
use crate::dead_strip_reach_cache::BytesKey;
use crate::dead_strip_reach_relocs::RelocIndex;
use crate::dead_strip_reach_resolve::{RelocTarget, decode_reloc};
use crate::dead_strip_reach_table::{Import, ImportRef};

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

#[derive(Default)]
pub(crate) struct EdgeIndex<'a> {
    relocs: RelocIndex<'a>,
    /// The slot in `members` of each member's bytes.
    slots: HashMap<BytesKey<'a>, u32>,
    /// Per member slot, per section.
    members: Vec<Vec<SectEdges>>,
    /// Every decoded list, one after another.
    arena: Vec<Edge>,
    ids: HashMap<&'a str, u32>,
    names: Vec<&'a str>,
}

impl<'a> EdgeIndex<'a> {
    /// The slot of the edges of `member`, whose tables are `r`.
    pub(crate) fn slot(&mut self, member: &ArMember<'a>, r: &MemberReach<'a>) -> u32 {
        let next = self.members.len() as u32;
        let slot = *self.slots.entry(BytesKey(member.data)).or_insert(next);
        if slot == next {
            self.members
                .push(vec![(Vec::new(), UNDECODED); r.sects.len()]);
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
        for e in self.relocs.in_range(member, sect, lo, hi)? {
            match decode_reloc(sect, e, member, r) {
                Some(RelocTarget::Local { sect, atom }) => {
                    self.arena.push(Edge::Local { sect, atom })
                }
                Some(RelocTarget::Undef(row, name)) => {
                    let sym = imports
                        .binary_search_by_key(&row, |i| i.row)
                        .ok()
                        .and_then(|k| match imports[k].to {
                            ImportRef::Sym(s) => Some(s),
                            ImportRef::Foreign(_) => None,
                        });
                    self.arena.push(match sym {
                        Some(s) => Edge::Sym(s),
                        None => {
                            let next = self.names.len() as u32;
                            let id = *self.ids.entry(name).or_insert(next);
                            if id == next {
                                self.names.push(name);
                            }
                            Edge::Name(id)
                        }
                    });
                }
                None => {}
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

fn span_u32(v: usize) -> Result<u32, String> {
    u32::try_from(v).map_err(|_| "reach edge arena overflow".to_string())
}
