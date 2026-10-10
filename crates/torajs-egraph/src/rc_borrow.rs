//! Array-element borrowing — Swift SemanticARCOpts' `load [copy] →
//! load_borrow` for `any[]` reads.
//!
//! `const s = arr[i]` lowers as an element load, a retain of the box
//! read (`rc_inc_any`), and a release when the binding's scope ends
//! (`rc_dec_any`). While the array is alive and its slot is not
//! written, the slot's own share keeps the element alive, so the pair
//! buys nothing. This pass removes it where that is provable and
//! leaves every other path of the same binding exactly as it was.
//!
//! The unit is a *web*: the any values joined by the `copy any`
//! multi-defs mem2reg left behind (one binding, every path into it,
//! every path out to a release). A web member is a *chain node* (all
//! its defs are copies) or an *origin* (one non-copy def, or a
//! parameter). A chain def `%v = copy %s` is an *edge* `s → v`.
//!
//! 1. Every `rc_inc_any` on the web is normalized onto the edges or
//!    origin it covers: a retain of chain node `n` moves to each def
//!    of `n`, which is sound when each def's block jumps straight to
//!    the retain's block, those are all its predecessors, and only
//!    retain-transparent instructions sit between (the same window
//!    predicate as `rc_peephole`). Any other placement leaves the web
//!    alone.
//! 2. An origin is *borrowable* when it is an element read of a
//!    dense `any[]` — `load_dyn any` through `load ptr, arr +32`,
//!    entered only through the header test `(word & mask) ==
//!    FLAG_ARR_ANY` on that same `arr` — and `rc_borrow_check` proves
//!    no holder of the web is live across anything that could write
//!    the slot or lower a count: a call (other than the read-only
//!    print intrinsics), a heap store, a release of anything. A
//!    `__torajs_any_length_get` on a holder (a Proxy trap may run
//!    user code) is allowed as the holder's *last* use and gets its
//!    own retain/release around the call.
//! 3. A retain on an edge from a borrowable origin is deleted. Each
//!    chain node gets a bool flag mirroring its copies — `false` on
//!    that edge, `true` on an edge whose retain stays or whose source
//!    was owned already, the source's flag through a plain copy — and
//!    every release of a chain node runs only when its flag is set.
//!    Unchanged paths release exactly as before; the borrowed path
//!    neither retains nor releases.
//!
//! Soundness rests on two facts: a path's share count after the
//! rewrite equals the original minus one exactly when the flag is
//! clear, and the flag is clear only on paths whose element the array
//! keeps alive until the last use (the hazard check), so neither a
//! release nor a use can reach a freed cell.
//!
//! The hazard check reads the instruction order this pass sees. The
//! passes after it (UNROLL_JAM, UNROLL_PAIR, LOOP_ROTATE, CONCAT_LICM,
//! block layout) must not move a call, a heap store or a release from
//! outside a borrowed span into it: cloning a block keeps its order,
//! and hoisting an instruction that already sat in a span keeps it
//! in that span.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId};

use crate::float_demote::push_value;
use crate::rc_borrow_check::{Env, check_uses};
use crate::rc_borrow_origin::is_array_elem_read;
use crate::rc_borrow_rewrite::{Plan, apply};
use crate::rc_peephole::{READONLY_INTRINSICS, collect_unescaped_slots, find_func};

/// The one opaque call a borrowed holder may be handed: answers an
/// owned box, never the receiver.
const LENGTH_GET: &str = "__torajs_any_length_get";

/// (block index, instruction index).
pub(crate) type Site = (usize, usize);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RcBorrowStats {
    /// Webs rewritten (one per binding shape).
    pub webs_borrowed: u32,
    /// Retains deleted across all of them.
    pub incs_elided: u32,
}

pub fn borrow_array_elems(module: &mut Module) -> RcBorrowStats {
    let mut stats = RcBorrowStats::default();
    let readonly: HashSet<u32> = READONLY_INTRINSICS
        .iter()
        .filter_map(|name| find_func(module, name))
        .collect();
    let length_get = find_func(module, LENGTH_GET);
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let env = Env {
            slots: collect_unescaped_slots(func),
            readonly: readonly.clone(),
            length_get,
        };
        let reads = elem_reads(func);
        if reads.is_empty() {
            continue;
        }
        // a rewrite adds bool flags and moves retains of members, so
        // it joins no webs; only the sites shift
        let webs: Vec<Vec<ValueId>> = copy_webs(func)
            .into_iter()
            .filter(|w| w.iter().any(|v| reads.contains(v)))
            .collect();
        let mut shape: Option<Shape> = None;
        for web in webs {
            let s = shape.get_or_insert_with(|| Shape {
                defs: collect_defs(func),
                preds: predecessors(func),
            });
            if let Some(plan) = plan_web(func, &web, &env, s) {
                stats.webs_borrowed += 1;
                stats.incs_elided += plan.incs_elided;
                apply(func, plan);
                shape = None;
            }
        }
    }
    stats
}

/// Def sites and predecessors of the function as it stands.
struct Shape {
    defs: HashMap<ValueId, Vec<Site>>,
    preds: Vec<Vec<usize>>,
}

/// Values a `load_dyn any` defines: the only shape a borrowable
/// origin has, so a web without one is skipped unread.
fn elem_reads(func: &Function) -> HashSet<ValueId> {
    let mut out = HashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let (
                Some(r),
                InstKind::LoadDyn(Type::Any, ..) | InstKind::LoadDynScaled8(Type::Any, ..),
            ) = (inst.result, &inst.kind)
            {
                out.insert(r);
            }
        }
    }
    out
}

/// The any values joined by `copy any` defs, plus every retained any,
/// grouped into connected components; each sorted, in order of their
/// first member.
fn copy_webs(func: &Function) -> Vec<Vec<ValueId>> {
    let mut parent: HashMap<ValueId, ValueId> = HashMap::default();
    fn find(parent: &mut HashMap<ValueId, ValueId>, v: ValueId) -> ValueId {
        let p = *parent.entry(v).or_insert(v);
        if p == v {
            return v;
        }
        let root = find(parent, p);
        parent.insert(v, root);
        root
    }
    for block in &func.blocks {
        for inst in &block.insts {
            match (&inst.kind, inst.result) {
                (InstKind::Copy(Type::Any, Operand::Value(s)), Some(r)) => {
                    let (a, b) = (find(&mut parent, r), find(&mut parent, *s));
                    parent.insert(a, b);
                }
                (InstKind::RcIncAny(Operand::Value(v)), _) => {
                    find(&mut parent, *v);
                }
                _ => {}
            }
        }
    }
    let keys: Vec<ValueId> = parent.keys().copied().collect();
    let mut groups: HashMap<ValueId, Vec<ValueId>> = HashMap::default();
    for v in keys {
        let root = find(&mut parent, v);
        groups.entry(root).or_default().push(v);
    }
    let mut webs: Vec<Vec<ValueId>> = groups.into_values().collect();
    for w in webs.iter_mut() {
        w.sort_by_key(|v| v.0);
    }
    webs.sort_by_key(|w| w[0].0);
    webs
}

/// How a web member is defined.
enum Node {
    /// Every def is `copy any`; the sites.
    Chain(Vec<Site>),
    /// One non-copy def, or `None` for a parameter.
    Origin(Option<Site>),
}

fn classify_nodes(
    func: &Function,
    web: &[ValueId],
    defs: &HashMap<ValueId, Vec<Site>>,
) -> Option<HashMap<ValueId, Node>> {
    let mut nodes = HashMap::default();
    for &m in web {
        let sites = defs.get(&m).map(Vec::as_slice).unwrap_or(&[]);
        let node = if sites.is_empty() {
            func.params.contains(&m).then_some(Node::Origin(None))?
        } else if sites
            .iter()
            .all(|&(b, i)| matches!(func.blocks[b].insts[i].kind, InstKind::Copy(Type::Any, _)))
        {
            Node::Chain(sites.to_vec())
        } else if let [site] = sites {
            Node::Origin(Some(*site))
        } else {
            return None;
        };
        nodes.insert(m, node);
    }
    Some(nodes)
}

fn collect_defs(func: &Function) -> HashMap<ValueId, Vec<Site>> {
    let mut defs: HashMap<ValueId, Vec<Site>> = HashMap::default();
    for (b, block) in func.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            if let Some(r) = inst.result {
                defs.entry(r).or_default().push((b, i));
            }
        }
    }
    defs
}

fn predecessors(func: &Function) -> Vec<Vec<usize>> {
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); func.blocks.len()];
    for (b, block) in func.blocks.iter().enumerate() {
        for s in crate::throw_check_fold::successors(&block.term) {
            preds[s.0 as usize].push(b);
        }
    }
    preds
}

/// Where a retain of the web is attached once normalized.
#[derive(Default)]
struct Retains {
    /// Def site of a chain node → the retain's own site.
    edge: HashMap<Site, Site>,
    /// Origin → the retain's site (same block as the def).
    origin: HashMap<ValueId, Site>,
}

/// Attach every retain of the web to the edges / origins it covers,
/// or answer `None` when one cannot be placed.
fn normalize_retains(
    func: &Function,
    web: &HashSet<ValueId>,
    nodes: &HashMap<ValueId, Node>,
    preds: &[Vec<usize>],
    env: &Env,
) -> Option<Retains> {
    let mut out = Retains::default();
    for (b, block) in func.blocks.iter().enumerate() {
        for (k, inst) in block.insts.iter().enumerate() {
            let InstKind::RcIncAny(Operand::Value(n)) = &inst.kind else {
                continue;
            };
            if !web.contains(n) {
                continue;
            }
            match nodes.get(n)? {
                Node::Chain(sites) => {
                    for site in edges_under(func, sites, (b, k), preds, env)? {
                        if out.edge.insert(site, (b, k)).is_some() {
                            return None;
                        }
                    }
                }
                Node::Origin(Some(d)) => {
                    if d.0 != b || d.1 >= k || !transparent_between(func, b, d.1 + 1, k, env) {
                        return None;
                    }
                    if out.origin.insert(*n, (b, k)).is_some() {
                        return None;
                    }
                }
                Node::Origin(None) => return None,
            }
        }
    }
    Some(out)
}

/// The def sites a retain at `at` of a chain node with defs `sites`
/// moves onto: the one def earlier in the same block, or every def
/// when each sits in a block that only jumps to `at`'s block and
/// those blocks are all of its predecessors.
fn edges_under(
    func: &Function,
    sites: &[Site],
    at: Site,
    preds: &[Vec<usize>],
    env: &Env,
) -> Option<Vec<Site>> {
    let (b, k) = at;
    if let [(db, di)] = sites
        && *db == b
    {
        return (*di < k && transparent_between(func, b, di + 1, k, env)).then(|| sites.to_vec());
    }
    let mut blocks: Vec<usize> = sites.iter().map(|s| s.0).collect();
    blocks.sort_unstable();
    let mut ps = preds[b].clone();
    ps.sort_unstable();
    if blocks != ps || !transparent_between(func, b, 0, k, env) {
        return None;
    }
    for &(p, i) in sites {
        let block = &func.blocks[p];
        if !matches!(block.term, Terminator::Br(t) if t.0 as usize == b)
            || !transparent_between(func, p, i + 1, block.insts.len(), env)
        {
            return None;
        }
    }
    Some(sites.to_vec())
}

/// Can a retain move backward over `insts[from..to]` of block `b`?
fn transparent_between(func: &Function, b: usize, from: usize, to: usize, env: &Env) -> bool {
    func.blocks[b].insts[from..to].iter().all(|i| {
        crate::rc_peephole::rc_transparent(&i.kind, &env.readonly, &env.slots)
            || matches!(
                i.kind,
                InstKind::GlobalRef(_) | InstKind::StaticStrRef(_) | InstKind::StringRef(_)
            )
    })
}

/// Decide the rewrite of one web, or `None` when nothing is borrowed.
fn plan_web(func: &mut Function, web: &[ValueId], env: &Env, shape: &Shape) -> Option<Plan> {
    let (defs, preds) = (&shape.defs, &shape.preds);
    let nodes = classify_nodes(func, web, defs)?;
    let members: HashSet<ValueId> = web.iter().copied().collect();
    let retains = normalize_retains(func, &members, &nodes, preds, env)?;
    if retains.edge.is_empty() && retains.origin.is_empty() {
        return None;
    }
    let borrowable: HashSet<ValueId> = nodes
        .iter()
        .filter_map(|(m, n)| match n {
            Node::Origin(Some(site)) if is_array_elem_read(func, *site, defs, preds) => Some(*m),
            _ => None,
        })
        .collect();
    if borrowable.is_empty() {
        return None;
    }
    let uses = check_uses(func, &members, env)?;
    // an origin retained where it is defined is owned from there on;
    // a borrowable one gives that retain up, and then none of its
    // edges may carry a second
    let owned = |o: ValueId| !(borrowable.contains(&o) && retains.origin.contains_key(&o));
    let mut edges: Vec<(Site, Edge)> = Vec::new();
    for node in nodes.values() {
        let Node::Chain(sites) = node else {
            continue;
        };
        for &site in sites {
            let InstKind::Copy(_, src) = &func.blocks[site.0].insts[site.1].kind else {
                return None;
            };
            let edge = match (retains.edge.get(&site), src) {
                (Some(&at), Operand::Value(o)) if borrowable.contains(o) => {
                    if retains.origin.contains_key(o) {
                        return None;
                    }
                    Edge::Borrowed(at)
                }
                (Some(&at), _) => Edge::Retained(at),
                (None, Operand::Value(o)) if matches!(nodes[o], Node::Chain(_)) => {
                    Edge::Through(*o)
                }
                (None, Operand::Value(o)) => Edge::Const(owned(*o)),
                (None, _) => Edge::Const(true),
            };
            edges.push((site, edge));
        }
    }
    let mut plan = Plan::default();
    for (&o, &at) in &retains.origin {
        if borrowable.contains(&o) {
            plan.remove.push(at);
            plan.incs_elided += 1;
        }
    }
    plan.incs_elided += edges
        .iter()
        .filter(|(_, e)| matches!(e, Edge::Borrowed(_)))
        .count() as u32;
    if plan.incs_elided == 0 {
        return None;
    }
    let mut flag_of: HashMap<ValueId, ValueId> = HashMap::default();
    for (&m, node) in &nodes {
        if matches!(node, Node::Chain(_)) {
            flag_of.insert(m, push_value(&mut func.values, Type::Bool));
        }
    }
    for (site, edge) in edges {
        let node = func.blocks[site.0].insts[site.1].result?;
        let flag = match edge {
            Edge::Borrowed(at) => {
                plan.remove.push(at);
                Operand::ConstBool(false)
            }
            Edge::Retained(at) => {
                plan.remove.push(at);
                let inst = func.blocks[at.0].insts[at.1].clone();
                plan.after.push((site, inst));
                Operand::ConstBool(true)
            }
            Edge::Through(o) => Operand::Value(flag_of[&o]),
            Edge::Const(b) => Operand::ConstBool(b),
        };
        plan.after.push((
            site,
            Inst {
                result: Some(flag_of[&node]),
                kind: InstKind::Copy(Type::Bool, flag),
                origin: None,
            },
        ));
    }
    for (site, m) in uses.releases {
        match flag_of.get(&m) {
            Some(f) => plan.cond_decs.push((site, *f)),
            None if owned(m) => {}
            None => plan.remove.push(site),
        }
    }
    plan.wraps = uses.wraps;
    Some(plan)
}

/// What a chain def's flag copies, once the retains are placed.
enum Edge {
    /// Its retain came from a borrowable origin: deleted, flag clear.
    Borrowed(Site),
    /// Its retain stays, moved onto the def: flag set.
    Retained(Site),
    /// A plain copy of another chain node: that node's flag.
    Through(ValueId),
    /// A copy of an origin or a constant: whether that is owned.
    Const(bool),
}

#[cfg(test)]
mod tests;
