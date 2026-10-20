//! S2 dead-strip reachability pass — the ld64-style atom closure
//! extracted from `dead_strip_diag` (RFC 20260824-s2-dead-strip,
//! blade 2a) so the strip blades and the diag renderer consume one
//! shared ground truth.
//!
//! Atom model: every rcgu.o carries `MH_SUBSECTIONS_VIA_SYMBOLS`,
//! so an atom is the byte range from one defined symbol (local or
//! extern — rustc's `l_anon.*` data labels anchor densely) to the
//! next, in EVERY section. A member without the flag (assembly
//! objects) has one atom per section. Reachability edges are the member reloc
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
use std::rc::Rc;

use torajs_codegen::CompiledFunction;
use torajs_obj::{N_SECT, N_TYPE};

use crate::archive_link_extra_syms::ExtraDefined;
use crate::archives_merge::{MergedArchives, RequiredMembers};
pub(crate) use crate::dead_strip_reach_build::trim16;
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_reach_walk::{Walk, Walker};
use crate::phase_timer::PhaseTimer;

/// One parsed nlist row — just the fields reachability needs.
pub(crate) struct NlEntry<'a> {
    pub(crate) name: &'a str,
    pub(crate) n_type: u8,
    pub(crate) n_sect: u8,
    pub(crate) n_value: u64,
}

/// Atom table over one section: sorted `(start, end)` object-file
/// address ranges. A leading unnamed gap (section start → first
/// symbol) is its own atom so literal pools before the first label
/// stay addressable.
#[derive(Clone)]
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
    pub(crate) atoms: Rc<[(u64, u64)]>,
    pub(crate) live: Vec<bool>,
    pub(crate) all_live: bool,
}

/// Per-member reachability state. Everything but the liveness bits
/// is shared with the link's [`ReachCache`].
#[derive(Clone)]
pub(crate) struct MemberReach<'a> {
    pub(crate) member_name: &'a str,
    /// Indexed by 1-based section ordinal − 1.
    pub(crate) sects: Vec<SectAtoms>,
    pub(crate) nlist: Rc<[NlEntry<'a>]>,
    /// Defined (N_SECT) symbols by name → (n_sect, n_value), sorted.
    pub(crate) defined: Rc<[(&'a str, u8, u64)]>,
    /// Sorted names `defined` holds only the last row of (more than
    /// one `N_SECT` row carries them).
    pub(crate) dup_defined: Rc<[&'a str]>,
    pub(crate) no_dead_strip: Rc<[u32]>,
}

/// Worklist node.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Node {
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

/// Why a node went live — recorded (first edge wins) only when the
/// why-live diag asks for it. First-wins makes every chain terminate:
/// a node's pred was visited strictly before the node itself.
pub(crate) enum Pred {
    /// Seeded by a user-function reloc naming this symbol: the symbol
    /// and the user fn the reloc sits in.
    UserFn(String),
    /// Seeded by a section flag root (no-dead-strip / mod-init / TLV).
    FlagRoot,
    Node(Node),
}

/// Closure output: per-member atom liveness plus the names that
/// never resolved (should be empty on a healthy link).
pub(crate) struct ReachResult<'a> {
    pub(crate) members: BTreeMap<(usize, usize), MemberReach<'a>>,
    pub(crate) unresolved: BTreeSet<String>,
    /// User-`.o` symbols some LIVE atom imports (an undef row the
    /// user definition satisfies) — the link judgment's evidence
    /// that a shadowing stub is referenced at all (r500).
    pub(crate) user_refs: BTreeSet<String>,
    /// First-recorded liveness edge per node; empty unless
    /// `record_preds` was set (the strip path never pays for it).
    pub(crate) preds: BTreeMap<Node, Pred>,
}

/// S2-5 diag-only what-if probes (never set on the strip path):
/// `cuts` keeps matching atoms live without propagating ("this fn
/// is specialized/stubbed"); `cut_ins` drops matching atoms from
/// the graph entirely ("this definition does not exist" — prices a
/// decoupling that removes every in-edge at once).
#[derive(Default)]
pub(crate) struct ReachProbes {
    pub(crate) cuts: Vec<String>,
    pub(crate) cut_ins: Vec<String>,
}

/// Run the atom-granularity reachability closure over the required
/// members. Pure analysis — touches nothing the emit path reads.
/// Member parses come from (and land in) `cache`.
pub(crate) fn compute_reachability<'a>(
    user_funcs: &[CompiledFunction],
    merged: &MergedArchives<'a>,
    required: &RequiredMembers,
    extra_defined_syms: &ExtraDefined,
    record_preds: bool,
    probes: Option<&ReachProbes>,
    cache: &mut ReachCache<'a>,
) -> Result<ReachResult<'a>, String> {
    let mut t = PhaseTimer::start(4);
    let walk = Walk::new(merged, required, cache)?;
    t.tick("reach_build_members");

    // Roots: the same seed set compute_required_members walked from,
    // then the section-flag roots.
    let defined_in_user = extra_defined_syms.with_user_fns(user_funcs);
    let mut w = Walker::new(merged, &defined_in_user, required, walk, record_preds);
    let mut worklist = w.user_roots(user_funcs, &mut cache.sym_loc);
    w.flag_roots(&mut worklist);
    w.close(worklist, probes, cache)?;
    t.tick("reach_closure");
    Ok(w.finish())
}

/// Does any defined symbol inside this atom's range match a
/// pattern (substring, same convention as the why-live query)?
/// Only `Node::Atom` can match — section-flag roots keep full
/// propagation so a what-if never under-counts real roots. Shared
/// with the who-census query (`dead_strip_who`).
pub(crate) fn node_is_cut(
    reach: &BTreeMap<(usize, usize), MemberReach<'_>>,
    node: Node,
    cuts: &[String],
) -> bool {
    let Node::Atom { key, .. } = node else {
        return false;
    };
    reach.get(&key).is_some_and(|r| atom_is_cut(r, node, cuts))
}

/// [`node_is_cut`] for a node of member `r`.
pub(crate) fn atom_is_cut(r: &MemberReach<'_>, node: Node, cuts: &[String]) -> bool {
    let Node::Atom { sect, atom, .. } = node else {
        return false;
    };
    let Some(sa) = r.sects.get(sect as usize - 1) else {
        return false;
    };
    let (s, e) = sa.atoms[atom];
    r.nlist.iter().any(|en| {
        en.n_type & N_TYPE == N_SECT
            && en.n_sect == sect
            && en.n_value >= s
            && en.n_value < e
            && cuts.iter().any(|c| en.name.contains(c.as_str()))
    })
}

pub(crate) use crate::dead_strip_reach_resolve::resolve_reloc_target;
