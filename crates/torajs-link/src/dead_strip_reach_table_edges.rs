//! A member's out-edges as one flat byte table: the cold table the
//! runtime image stores for every member it holds, built when `tr` was
//! compiled (`dead_strip_reach_encode_edges`). An atom's out-edges are
//! what the reach closure decodes from the member's relocs at the
//! atom's first expansion, so a link reads them here instead. They sit
//! apart from the member's reachability table: a link reads that table
//! for every member it pulls, and these only for the atoms it expands.
//!
//! Layout (little-endian u32):
//!
//! ```text
//!   header    n_sects n_edges
//!   edge_sect n_sects × where the section's bounds start in `edge_at`,
//!             or NO_EDGES when no atom of the section has an edge
//!   edges     n_edges × one out-edge of an atom, in reloc-table order,
//!             as [`encode_edge`] packs it
//!   edge_at   per section with edges, atom_count + 1 bounds into
//!             `edges`: atom k's edges run from bound k to bound k + 1
//! ```
//!
//! An empty table stores no edges: the member has relocs that do not
//! all pack (an undefined row it does not import, an atom index or a
//! symbol row too large), and a link decodes them as for an image
//! built without tables.

use crate::archives_merge::MergedArchives;
use crate::dead_strip_reach_edges::Decoded;

pub(crate) const HEADER_LEN: usize = 2 * 4;

/// The `edge_sect` entry of a section without edges.
pub(crate) const NO_EDGES: u32 = u32::MAX;

/// An edge's kind sits in its top two bits.
const KIND_SHIFT: u32 = 30;
const PAYLOAD: u32 = (1 << KIND_SHIFT) - 1;
/// A local atom edge: the section in bits 22..30, the atom below.
const ATOM_BITS: u32 = 22;
const KIND_ATOM: u32 = 0;
const KIND_SECT: u32 = 1;
const KIND_SYM: u32 = 2;
const KIND_FOREIGN: u32 = 3;

/// One stored out-edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableEdge {
    /// `atom: None` is the whole section.
    Local { sect: u8, atom: Option<u32> },
    /// An import, by the row of its name in the image's symbol index.
    Sym(u32),
    /// The k-th import the index lacks.
    Foreign(u32),
}

/// `d` packed as one `edges` entry; `foreign_k` gives each import's
/// position among the member's foreign imports. `None` when it does
/// not pack.
pub(crate) fn encode_edge(d: Decoded<'_>, foreign_k: &[u32]) -> Option<u32> {
    let (kind, payload) = match d {
        Decoded::Local {
            sect,
            atom: Some(a),
        } => (
            KIND_ATOM,
            (a < 1 << ATOM_BITS).then(|| u32::from(sect) << ATOM_BITS | a)?,
        ),
        Decoded::Local { sect, atom: None } => (KIND_SECT, u32::from(sect)),
        Decoded::Sym(row) => (KIND_SYM, (row <= PAYLOAD).then_some(row)?),
        Decoded::Foreign(i, _) => (KIND_FOREIGN, foreign_k[i]),
        Decoded::Unlisted(_) => return None,
    };
    Some(kind << KIND_SHIFT | payload)
}

fn decode_edge(v: u32) -> TableEdge {
    let payload = v & PAYLOAD;
    match v >> KIND_SHIFT {
        KIND_ATOM => TableEdge::Local {
            sect: (payload >> ATOM_BITS) as u8,
            atom: Some(payload & ((1 << ATOM_BITS) - 1)),
        },
        KIND_SECT => TableEdge::Local {
            sect: payload as u8,
            atom: None,
        },
        KIND_SYM => TableEdge::Sym(payload),
        _ => TableEdge::Foreign(payload),
    }
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// The image's out-edge table of member `m` of archive `a`; empty when
/// the image stores none.
pub(crate) fn image_edges<'a>(merged: &MergedArchives<'a>, (a, m): (usize, usize)) -> &'a [u8] {
    merged.image.member_cold_table(a, m)
}

/// The out-edge lists a member's edge table stores.
#[derive(Clone, Copy)]
pub(crate) struct TableEdges<'a> {
    tab: &'a [u8],
    edges_off: usize,
    at_off: usize,
}

impl<'a> TableEdges<'a> {
    /// The out-edges `tab` stores, `None` when it is empty.
    pub(crate) fn of(tab: &'a [u8]) -> Option<Self> {
        if tab.is_empty() {
            return None;
        }
        let edges_off = HEADER_LEN + 4 * u32_at(tab, 0) as usize;
        Some(TableEdges {
            tab,
            edges_off,
            at_off: edges_off + 4 * u32_at(tab, 4) as usize,
        })
    }

    /// The out-edges of atom `atom` of section `sect` (1-based).
    pub(crate) fn atom(&self, sect: u8, atom: u32) -> impl Iterator<Item = TableEdge> + 'a {
        let (tab, edges_off) = (self.tab, self.edges_off);
        let first = u32_at(tab, HEADER_LEN + 4 * (sect as usize - 1));
        let (from, to) = if first == NO_EDGES {
            (0, 0)
        } else {
            let at = self.at_off + 4 * (first as usize + atom as usize);
            (u32_at(tab, at) as usize, u32_at(tab, at + 4) as usize)
        };
        (from..to).map(move |k| decode_edge(u32_at(tab, edges_off + 4 * k)))
    }
}
