//! The clusters of a class's methods (F3) and what can run while one
//! of them is on the stack (F2).

use std::collections::{HashMap, HashSet};

use crate::ssa::{ArrId, FuncId, InstKind, Module, PinFact, StructId, Type};

use super::roots::{Root, Roots};
use super::{Watch, grow, stats};

const ADAPTER_PREFIX: &str = "__boxed_";

pub(super) struct Clusters<'a> {
    module: &'a Module,
    watch: &'a Watch,
    skip: &'a [bool],
    alloc: Option<FuncId>,
    groups: HashMap<StructId, Vec<Vec<FuncId>>>,
    /// Per function reached, the first instruction that can change an
    /// array.
    sites: HashMap<FuncId, Option<String>>,
}

impl<'a> Clusters<'a> {
    pub(super) fn new(
        module: &'a Module,
        watch: &'a Watch,
        skip: &'a [bool],
        alloc: Option<FuncId>,
    ) -> Self {
        Clusters {
            module,
            watch,
            skip,
            alloc,
            groups: HashMap::new(),
            sites: HashMap::new(),
        }
    }

    /// The clusters of `sid` that read its array field at `off` and
    /// change no array while they run.
    pub(super) fn pins(
        &mut self,
        sid: StructId,
        off: u64,
        arr: ArrId,
        label: &str,
    ) -> Vec<PinFact> {
        let mut out = Vec::new();
        for cluster in self.groups_of(sid) {
            let names = self.names(&cluster);
            let reads: usize = cluster.iter().map(|m| self.reads(*m, off)).sum();
            let why = if reads == 0 {
                Some("no read".to_string())
            } else {
                self.changes(&cluster)
            };
            if let Some(why) = why {
                if stats() {
                    eprintln!("[pin] {label}: cluster {{{names}}}: none: {why}");
                }
                continue;
            }
            let (entries, direct_only) = self.entries(&cluster);
            if stats() {
                let direct = if direct_only { "direct" } else { "taken" };
                let e = self.names(&entries);
                eprintln!(
                    "[pin] {label}: cluster {{{names}}} entries {{{e}}} reads {reads} {direct}"
                );
            }
            let quiet = self.quiet(&cluster);
            out.push(PinFact {
                sid,
                field_off: off,
                arr,
                cluster,
                entries,
                direct_only,
                quiet,
            });
        }
        out
    }

    /// Whether some cluster of `sid` reads its array field at `off` and
    /// changes no array while it runs: what [`Clusters::pins`] asks but
    /// F1.
    pub(super) fn any(&mut self, sid: StructId, off: u64) -> bool {
        (self.groups_of(sid).iter())
            .any(|c| c.iter().any(|m| self.reads(*m, off) > 0) && self.changes(c).is_none())
    }

    fn groups_of(&mut self, sid: StructId) -> Vec<Vec<FuncId>> {
        if let Some(g) = self.groups.get(&sid) {
            return g.clone();
        }
        let (g, out) = self.group(sid);
        if stats() && !out.is_empty() {
            let sid = sid.0;
            eprintln!(
                "[pin] sid {sid}: out of every cluster {{{}}}",
                self.names(&out)
            );
        }
        self.groups.insert(sid, g.clone());
        g
    }

    /// F3: the methods of `sid` that pass their own receiver on to the
    /// others they call, split into the groups those calls connect;
    /// and the methods put out by a call that passes another cell.
    pub(super) fn group(&self, sid: StructId) -> (Vec<Vec<FuncId>>, Vec<FuncId>) {
        let module = self.module;
        let cands = self.candidates(sid);
        let mut out_of: HashSet<usize> = HashSet::new();
        let mut edges = Vec::new();
        for &g in &cands {
            let roots = Roots::of(&module.funcs[g], self.alloc);
            for inst in module.funcs[g].blocks.iter().flat_map(|b| &b.insts) {
                let InstKind::Call(h, args) = &inst.kind else {
                    continue;
                };
                let h = h.0 as usize;
                if !cands.contains(&h) {
                    continue;
                }
                if args
                    .first()
                    .is_some_and(|a| roots.root(a) == Root::Receiver)
                {
                    edges.push((g, h));
                } else {
                    out_of.extend([g, h]);
                }
            }
        }
        let members = cands.iter().filter(|c| !out_of.contains(c)).copied();
        let groups = components(members.collect(), &edges);
        let mut out: Vec<usize> = out_of.into_iter().collect();
        out.sort_unstable();
        (groups, out.into_iter().map(|m| FuncId(m as u32)).collect())
    }

    /// The functions of the module that may join a cluster of `sid`:
    /// the bodies that run with a cell of `sid` as their first
    /// parameter, but for those something else enters on its own
    /// terms. A constructor (any `__ctor`, its specializations
    /// `__ctor$$…` included) runs on a cell under construction; an
    /// adapter, a suspended frame's entries and a release fn are
    /// entered by the runtime. The name test is wider than the
    /// constructors: a method whose own name holds `__ctor` is left out
    /// too, which only takes a member away.
    fn candidates(&self, sid: StructId) -> Vec<usize> {
        let module = self.module;
        let frames: HashSet<FuncId> = (module.suspend_fns.iter())
            .flat_map(|s| [s.resume, s.drop, s.trace])
            .collect();
        let candidate = |i: usize| {
            let f = &module.funcs[i];
            self.watch.methods[i]
                && !self.skip[i]
                && !f.is_declaration()
                && f.value_type(f.params[0]) == Type::Obj(sid)
                && !(f.name.starts_with("__cm_") && f.name.contains("__ctor"))
                && !f.name.starts_with(ADAPTER_PREFIX)
                && !frames.contains(&FuncId(i as u32))
                && !module.class_drop_fns.contains(&FuncId(i as u32))
        };
        (0..module.funcs.len()).filter(|i| candidate(*i)).collect()
    }

    /// The functions that never run (`skip`) and call a member of
    /// `cluster` or take its address.
    fn quiet(&self, cluster: &[FuncId]) -> Vec<FuncId> {
        let named = |f: &crate::ssa::Function| {
            (f.blocks.iter().flat_map(|b| &b.insts)).any(|i| match &i.kind {
                InstKind::Call(t, _) | InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) => {
                    cluster.contains(t)
                }
                _ => false,
            })
        };
        (self.module.funcs.iter().enumerate())
            .filter(|(i, f)| self.skip[*i] && named(f))
            .map(|(i, _)| FuncId(i as u32))
            .collect()
    }

    /// The reads of the field at `off` of `m`'s receiver.
    fn reads(&self, m: FuncId, off: u64) -> usize {
        let f = &self.module.funcs[m.0 as usize];
        let roots = Roots::of(f, self.alloc);
        (f.blocks.iter().flat_map(|b| &b.insts))
            .filter(|inst| {
                matches!(&inst.kind, InstKind::Load(_, base, o)
                    if *o == off && roots.root(base) == Root::Receiver)
            })
            .count()
    }

    /// F2: the first function the cluster's members reach, themselves
    /// included, that can change an array, and how.
    fn changes(&mut self, cluster: &[FuncId]) -> Option<String> {
        let module = self.module;
        let mut seen: HashSet<FuncId> = cluster.iter().copied().collect();
        let mut stack: Vec<FuncId> = cluster.to_vec();
        while let Some(fid) = stack.pop() {
            let f = &module.funcs[fid.0 as usize];
            let site = (self.sites.entry(fid)).or_insert_with(|| grow::site(module, f));
            if let Some(why) = site {
                return Some(format!("{} in {}", why, f.name));
            }
            for inst in f.blocks.iter().flat_map(|b| &b.insts) {
                if let InstKind::Call(t, _) = &inst.kind
                    && !module.funcs[t.0 as usize].is_declaration()
                    && seen.insert(*t)
                {
                    stack.push(*t);
                }
            }
        }
        None
    }

    /// The members called from outside the cluster or whose address is
    /// taken, and whether nothing but a direct call enters any member.
    fn entries(&self, cluster: &[FuncId]) -> (Vec<FuncId>, bool) {
        let inside: HashSet<FuncId> = cluster.iter().copied().collect();
        let rows: HashSet<FuncId> = (self.module.vtable_globals.iter())
            .flat_map(|v| v.fn_ids.iter().flatten().copied())
            .collect();
        let entries = (cluster.iter().copied())
            .filter(|m| {
                self.watch.taken.contains(m)
                    || (self.watch.calls.iter()).any(|(c, t)| t == m && !inside.contains(c))
            })
            .collect();
        let direct_only =
            (cluster.iter()).all(|m| !self.watch.taken.contains(m) && !rows.contains(m));
        (entries, direct_only)
    }

    fn names(&self, fids: &[FuncId]) -> String {
        let short = |f: &FuncId| {
            let name = &self.module.funcs[f.0 as usize].name;
            let rest = name.strip_prefix("__cm_").and_then(|r| r.split_once("__"));
            rest.map_or(name.as_str(), |(_, m)| m).to_string()
        };
        fids.iter().map(short).collect::<Vec<_>>().join(" ")
    }
}

/// The groups `edges` connect among `members` (sorted, each group in
/// order of its first member); an edge with an end outside `members`
/// connects nothing.
fn components(members: Vec<usize>, edges: &[(usize, usize)]) -> Vec<Vec<FuncId>> {
    let mut parent: HashMap<usize, usize> = members.iter().map(|m| (*m, *m)).collect();
    fn find(p: &mut HashMap<usize, usize>, x: usize) -> usize {
        let up = p[&x];
        if up == x {
            return x;
        }
        let r = find(p, up);
        p.insert(x, r);
        r
    }
    for &(g, h) in edges {
        if parent.contains_key(&g) && parent.contains_key(&h) {
            let (a, b) = (find(&mut parent, g), find(&mut parent, h));
            parent.insert(a.max(b), a.min(b));
        }
    }
    let mut groups: Vec<Vec<FuncId>> = Vec::new();
    let mut at: HashMap<usize, usize> = HashMap::new();
    let mut sorted = members;
    sorted.sort_unstable();
    for m in sorted {
        let r = find(&mut parent, m);
        let i = *at.entry(r).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[i].push(FuncId(m as u32));
    }
    groups
}
