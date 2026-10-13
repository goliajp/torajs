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
use torajs_obj::{
    N_NO_DEAD_STRIP, N_SECT, N_TYPE, S_ATTR_LIVE_SUPPORT, S_ATTR_NO_DEAD_STRIP,
    S_MOD_INIT_FUNC_POINTERS, S_TERM_FUNC_POINTERS, S_THREAD_LOCAL_VARIABLES, SECTION_TYPE,
};

use crate::archive_link_extra_syms::{ExtraDefined, UserDefined};
use crate::archives_merge::{MergedArchives, RequiredMembers, reloc_target_name};
pub(crate) use crate::dead_strip_reach_build::trim16;
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_reach_edges::{Edge, EdgeIndex, Seen, UndefMemo};
use crate::phase_timer::PhaseTimer;

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
    /// Defined (N_SECT) symbols by name → (n_sect, n_value).
    pub(crate) defined: Rc<BTreeMap<&'a str, (u8, u64)>>,
    /// Sorted names `defined` holds only the last row of (more than
    /// one `N_SECT` row carries them).
    pub(crate) dup_defined: Rc<[&'a str]>,
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
    let mut reach: BTreeMap<(usize, usize), MemberReach<'a>> = BTreeMap::new();
    for &(a, m) in &required.members {
        let member = &merged.per_archive_members[a][m];
        reach.insert((a, m), cache.member_reach(member)?);
    }
    t.tick("reach_build_members");

    // Roots: the same seed set compute_required_members walked from.
    let defined_in_user = extra_defined_syms.with_user_fns(user_funcs);
    let mut worklist: Vec<Node> = Vec::new();
    let mut unresolved: BTreeSet<String> = BTreeSet::new();
    let mut preds: BTreeMap<Node, Pred> = BTreeMap::new();
    for f in user_funcs {
        for r in &f.relocs {
            if let Some(name) = reloc_target_name(&r.kind)
                && !defined_in_user.contains(name)
                && !required.dyld_imports.contains_key(name)
                && let Some(node) = node_for_global_name(name, merged, &reach)
            {
                if record_preds {
                    preds
                        .entry(node)
                        .or_insert_with(|| Pred::UserFn(format!("{name} (in {})", f.name)));
                }
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
            // `__LD,__compact_unwind` is link-only input: ld64 folds
            // it into `__TEXT,__unwind_info` and it never reaches a
            // final binary; this pipeline builds no unwind_info at
            // all (panic=abort substrate), so its LIVE_SUPPORT flag
            // must not root the per-fn unwind rows — treating it as
            // a hard root kept every unwind-covered fn (whole
            // members: date, arr, str…) alive with no consumer.
            // Equivalent to ld64's `-no_compact_unwind`.
            // `__TEXT,__eh_frame` is the same kind of input: its FDEs
            // describe functions and are LIVE_SUPPORT (live only if
            // what they describe is), and this pipeline registers no
            // unwind tables either. As a root, the iter archives' std
            // FDEs kept almost all of std — sockets, process spawn,
            // threads — and 179 libSystem imports that dyld binds on
            // every launch of an iter-built program.
            if matches!(trim16(&sa.sectname), b"__compact_unwind" | b"__eh_frame") {
                continue;
            }
            if sa.flags & (S_ATTR_NO_DEAD_STRIP | S_ATTR_LIVE_SUPPORT) != 0
                || ty == S_MOD_INIT_FUNC_POINTERS
                || ty == S_TERM_FUNC_POINTERS
                || ty == S_THREAD_LOCAL_VARIABLES
            {
                let node = Node::AllSect {
                    key,
                    sect: (i + 1) as u8,
                };
                if record_preds {
                    preds.entry(node).or_insert(Pred::FlagRoot);
                }
                worklist.push(node);
            }
        }
        for e in r.nlist.iter() {
            if e.n_type & N_TYPE == N_SECT && e.n_desc & N_NO_DEAD_STRIP != 0 {
                let node = node_for_addr(key, r, e.n_sect, e.n_value);
                if record_preds {
                    preds.entry(node).or_insert(Pred::FlagRoot);
                }
                worklist.push(node);
            }
        }
    }

    // Fixpoint.
    let mut user_refs: BTreeSet<String> = BTreeSet::new();
    let mut visited = Seen::new(&reach);
    let mut undefs = UndefMemo::default();
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        // S2-5 pricing probes (diag-only; the strip path passes
        // None, so the what-if closures never shape bytes).
        if let Some(p) = probes {
            // cut_in: the definition is treated as absent — not
            // live, no propagation.
            if !p.cut_ins.is_empty() && node_is_cut(&reach, node, &p.cut_ins) {
                continue;
            }
            // cut: live but stubbed — outgoing edges gone.
            if !p.cuts.is_empty() && node_is_cut(&reach, node, &p.cuts) {
                if let Node::Atom { key, sect, atom } = node
                    && let Some(r) = reach.get_mut(&key)
                    && let Some(sa) = r.sects.get_mut(sect as usize - 1)
                {
                    sa.live[atom] = true;
                }
                continue;
            }
        }
        expand_node(
            node,
            merged,
            &mut reach,
            &defined_in_user,
            required,
            &mut worklist,
            &mut unresolved,
            &mut user_refs,
            record_preds.then_some(&mut preds),
            &mut cache.edges,
            &mut undefs,
        )?;
    }

    t.tick("reach_closure");

    Ok(ReachResult {
        members: reach,
        unresolved,
        user_refs,
        preds,
    })
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
    let Node::Atom { key, sect, atom } = node else {
        return false;
    };
    let Some(r) = reach.get(&key) else {
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

/// Mark a node live and enqueue everything it references.
#[allow(clippy::too_many_arguments)]
fn expand_node<'a>(
    node: Node,
    merged: &MergedArchives<'a>,
    reach: &mut BTreeMap<(usize, usize), MemberReach<'a>>,
    defined_in_user: &UserDefined<'_>,
    required: &RequiredMembers,
    worklist: &mut Vec<Node>,
    unresolved: &mut BTreeSet<String>,
    user_refs: &mut BTreeSet<String>,
    mut preds: Option<&mut BTreeMap<Node, Pred>>,
    edges: &mut EdgeIndex<'a>,
    undefs: &mut UndefMemo,
) -> Result<(), String> {
    let (key, sect_ord, atom) = match node {
        Node::Atom { key, sect, atom } => {
            let r = reach.get_mut(&key).ok_or("member vanished")?;
            let sa = r.sects.get_mut(sect as usize - 1).ok_or("bad sect")?;
            sa.live[atom] = true;
            (key, sect, Some(atom as u32))
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
    let r = reach.get(&key).ok_or("member vanished")?;
    if r.sects
        .get(sect_ord as usize - 1)
        .ok_or("bad sect")?
        .no_file_storage
    {
        return Ok(()); // zerofill: no payload, no relocs
    }
    for &edge in edges.out_edges(member, r, sect_ord, atom)?.iter() {
        let next = match edge {
            Edge::Local { sect, atom } => local_node(key, sect, atom),
            Edge::Undef(id) => {
                let name = edges.name(id);
                let target = undefs.get_or(id, || {
                    if defined_in_user.contains(name) {
                        user_refs.insert(name.to_string());
                    }
                    resolve_undef(name, merged, reach, defined_in_user, required, unresolved)
                });
                let Some(next) = target else {
                    continue;
                };
                next
            }
        };
        if let Some(p) = preds.as_deref_mut() {
            p.entry(next).or_insert(Pred::Node(node));
        }
        worklist.push(next);
    }
    Ok(())
}

pub(crate) use crate::dead_strip_reach_resolve::{
    local_node, node_for_addr, node_for_global_name, resolve_reloc_target, resolve_undef,
};
