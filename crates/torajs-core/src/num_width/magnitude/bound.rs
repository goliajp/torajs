//! Settling the recorded bounds: every slot over the canonical edge
//! graph, one assignment cycle at a time, then every recorded operator.
//!
//! A counter is bounded two ways and keeps the tighter. By the horizon:
//! it starts within its largest entry and moves by at most its largest
//! step per execution, for at most [`COUNTER_HORIZON`] executions. By
//! its guards, when it only counts up and every step sits directly in a
//! loop whose test holds it below a bound: since the latest entry or
//! test, each such loop has run at most one pass of its steps, so it is
//! within the largest entry or bound plus one pass of every loop's
//! steps. The second holds for a binding each call has its own copy of,
//! or a top-level one stepped only by top-level loops; a recursive call
//! could otherwise step a shared one between a test and its step.

use super::{COUNTER_HORIZON, EXACT_INT_MAX, Guard, Lin, Mag, TOP, UNRECORDED_REACH, lin};
use crate::ast::{Expr, ExprId, Stmt};
use crate::num_width::container::{UnionFind, canon_key_frozen};
use crate::num_width::cycle::tarjan;
use crate::num_width::slot_keys::{SlotKeyData, SlotKeys};
use crate::num_width::{Analysis, SlotKeyId};
use std::collections::{HashMap, HashSet};

/// A write's bound and the innermost loop it runs in.
type Write = (Mag, Option<usize>);

/// Bound every slot, seed F64 every slot whose bound leaves the window,
/// and answer the operators whose bound stays inside it. Runs after
/// `container::canonicalize`.
pub(in crate::num_width) fn settle(a: &mut Analysis) -> HashSet<ExprId> {
    let record = std::mem::take(&mut a.mag);
    let getters: HashMap<String, Vec<SlotKeyId>> =
        crate::ast::accessor_lookup::getter_fns_by_prop(a.ast)
            .into_iter()
            .map(|(p, fns)| (p, fns.iter().map(|f| a.keys.ret(f)).collect()))
            .collect();
    let canon = |m: &Mag| canon_mag(&a.uf, &a.keys, &getters, m);
    let mut writes: HashMap<SlotKeyId, Vec<Write>> = HashMap::new();
    for (t, m, at) in &record.writes {
        let t = canon_key_frozen(&a.uf, &a.keys, *t);
        writes.entry(t).or_default().push((canon(m), *at));
    }
    // edges not marked growth carry a value through unchanged; the
    // growth ones come from recorded writes, bounded above, and a copy
    // of a slot into itself carries nothing
    for (src, dsts) in &a.edges {
        for (t, growth) in dsts {
            if !growth && t != src {
                writes.entry(*t).or_default().push((Mag::Slot(*src), None));
            }
        }
    }
    let guards: Vec<Option<Guard>> = record
        .guards
        .into_iter()
        .map(|g| {
            g.map(|g| Guard {
                fn_name: g.fn_name,
                key: canon_key_frozen(&a.uf, &a.keys, g.key),
                below: canon(&g.below),
            })
        })
        .collect();
    let bounds = bound_slots(&a.keys, &writes, &guards, &direct_only(a));
    for (k, (_, reach)) in &bounds {
        if *reach > EXACT_INT_MAX as u128 {
            a.seeds.push(*k);
        }
    }
    let read = |k: &SlotKeyId| Some(bounds.get(k).map_or(UNRECORDED_REACH, |b| b.0));
    let mut over: HashSet<ExprId> = HashSet::new();
    let mut seen: HashSet<ExprId> = HashSet::new();
    for (eid, m) in &record.arith {
        seen.insert(*eid);
        if lin(&canon(m), &read).k > EXACT_INT_MAX as u128 {
            over.insert(*eid);
        }
    }
    seen.retain(|e| !over.contains(e));
    seen
}

/// `m` over class representatives, with each property read spelled as
/// the returns of the getters it may run.
fn canon_mag(
    uf: &UnionFind,
    keys: &SlotKeys,
    getters: &HashMap<String, Vec<SlotKeyId>>,
    m: &Mag,
) -> Mag {
    let b = |m: &Mag| Box::new(canon_mag(uf, keys, getters, m));
    match m {
        Mag::Const(c) => Mag::Const(*c),
        Mag::Slot(k) => Mag::Slot(canon_key_frozen(uf, keys, *k)),
        Mag::Sum(x, y) => Mag::Sum(b(x), b(y)),
        Mag::Diff(x, y) => Mag::Diff(b(x), b(y)),
        Mag::Prod(x, y) => Mag::Prod(b(x), b(y)),
        Mag::Min(x, y) => Mag::Min(b(x), b(y)),
        Mag::Max(v) => Mag::Max(v.iter().map(|m| canon_mag(uf, keys, getters, m)).collect()),
        Mag::Prop(p) => Mag::Max(
            getters
                .get(p)
                .into_iter()
                .flatten()
                .map(|k| Mag::Slot(canon_key_frozen(uf, keys, *k)))
                .collect(),
        ),
    }
}

fn slots_of(m: &Mag, out: &mut Vec<SlotKeyId>) {
    match m {
        Mag::Const(_) | Mag::Prop(_) => {}
        Mag::Slot(k) => out.push(*k),
        Mag::Sum(x, y) | Mag::Diff(x, y) | Mag::Prod(x, y) | Mag::Min(x, y) => {
            slots_of(x, out);
            slots_of(y, out);
        }
        Mag::Max(v) => v.iter().for_each(|m| slots_of(m, out)),
    }
}

/// The bound of every slot a write reaches or reads, dependencies first,
/// with what its writes compute on the way: a step reads the slot and
/// adds to it, which is one step past the slot's bound before the slot
/// holds it.
fn bound_slots(
    keys: &SlotKeys,
    writes: &HashMap<SlotKeyId, Vec<Write>>,
    guards: &[Option<Guard>],
    direct: &HashSet<String>,
) -> HashMap<SlotKeyId, (u128, u128)> {
    let mut index: HashMap<SlotKeyId, usize> = HashMap::new();
    let mut nodes: Vec<SlotKeyId> = Vec::new();
    let mut adj: Vec<Vec<(usize, bool)>> = Vec::new();
    let mut intern = |k: SlotKeyId, nodes: &mut Vec<SlotKeyId>, adj: &mut Vec<Vec<_>>| {
        *index.entry(k).or_insert_with(|| {
            nodes.push(k);
            adj.push(Vec::new());
            nodes.len() - 1
        })
    };
    let mut deps_of: HashMap<SlotKeyId, Vec<SlotKeyId>> = HashMap::new();
    for (t, ws) in writes {
        let deps = deps_of.entry(*t).or_default();
        ws.iter().for_each(|(m, _)| slots_of(m, deps));
    }
    for g in guards.iter().flatten() {
        if writes.contains_key(&g.key) {
            slots_of(&g.below, deps_of.entry(g.key).or_default());
        }
    }
    for (t, deps) in &deps_of {
        let ti = intern(*t, &mut nodes, &mut adj);
        for d in deps {
            let di = intern(*d, &mut nodes, &mut adj);
            adj[ti].push((di, false));
        }
    }
    let scc_of = tarjan(nodes.len(), &adj);
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (i, &c) in scc_of.iter().enumerate() {
        if members.len() <= c {
            members.resize(c + 1, Vec::new());
        }
        members[c].push(i);
    }
    // a component completes after every component it reads from, so
    // ascending ids put dependencies first
    let mut bound: Vec<(u128, u128)> = vec![(0, 0); nodes.len()];
    for (c, ms) in members.iter().enumerate() {
        let outside = |k: &SlotKeyId| {
            let i = index[k];
            (scc_of[i] != c).then(|| bound[i].0)
        };
        let ws = |i: usize| writes.get(&nodes[i]).into_iter().flatten();
        // a binding is written only where the walk sees; anything else
        // may also hold what the runtime hands it
        let binding = |i: &usize| match keys.data(nodes[*i]) {
            SlotKeyData::Local(..) | SlotKeyData::Global(_) => true,
            SlotKeyData::Param(f, _) => direct.contains(&*keys.name(f)),
            _ => false,
        };
        let floor = if ms.iter().all(binding) {
            0
        } else {
            UNRECORDED_REACH
        };
        let (mut entry, mut step, mut wild) = (floor, 0u128, false);
        for &i in ms {
            for (m, _) in ws(i) {
                let l = lin(m, &outside);
                match l.x {
                    0 => entry = entry.max(l.k),
                    1 => step = step.max(l.k),
                    _ => wild = true,
                }
            }
        }
        let mut b = if wild {
            TOP
        } else {
            entry.saturating_add(step.saturating_mul(COUNTER_HORIZON as u128))
        };
        if let [i] = ms[..]
            && step > 0
            && let Some(g) = guarded(keys, nodes[i], ws(i), guards, &outside)
        {
            b = b.min(entry.max(g.0).saturating_add(g.1));
        }
        for &i in ms {
            bound[i] = (b, b.saturating_add(step));
        }
    }
    nodes.into_iter().zip(bound).collect()
}

/// For a counter `k` stepped only upward, directly in loops whose test
/// holds it below a bound: the largest such bound, and one pass of
/// every loop's steps.
fn guarded<'w>(
    keys: &SlotKeys,
    k: SlotKeyId,
    ws: impl Iterator<Item = &'w Write>,
    guards: &[Option<Guard>],
    outside: &impl Fn(&SlotKeyId) -> Option<u128>,
) -> Option<(u128, u128)> {
    let own = matches!(
        keys.data(k),
        SlotKeyData::Local(..) | SlotKeyData::Param(..)
    );
    let mut passes: HashMap<usize, u128> = HashMap::new();
    for (m, at) in ws {
        if lin(m, outside).x == 0 {
            continue;
        }
        let up = match m {
            Mag::Slot(s) if *s == k => 0,
            Mag::Sum(x, y) => match (&**x, &**y) {
                (Mag::Slot(s), Mag::Const(c)) | (Mag::Const(c), Mag::Slot(s)) if *s == k => *c,
                _ => return None,
            },
            _ => return None,
        };
        let g = guards.get((*at)?)?.as_ref()?;
        if g.key != k || !(own || g.fn_name.is_empty()) {
            return None;
        }
        let pass = passes.entry((*at)?).or_default();
        *pass = pass.saturating_add(up);
    }
    let mut below = 0u128;
    for l in passes.keys() {
        let Lin { x: 0, k: b } = lin(&guards[*l].as_ref()?.below, outside) else {
            return None;
        };
        below = below.max(b);
    }
    Some((below, passes.values().fold(0, |s, p| s.saturating_add(*p))))
}

/// Functions every argument of which a call the walk recorded spells:
/// declared at the top level without a default, and named nowhere but
/// as the callee of a call. Any other function may be handed values the
/// walk never sees: by the runtime as a callback, through `call` or
/// `apply`, or by a call that leaves a defaulted parameter out.
fn direct_only(a: &Analysis) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    for s in &a.ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            type_params,
            ..
        } = s
            && !name.starts_with("__")
            && type_params.is_empty()
            && params.iter().all(|p| p.default.is_none() && !p.is_rest)
        {
            out.insert(name.clone());
        }
    }
    let mut callees: HashSet<ExprId> = HashSet::new();
    for i in 0..a.ast.exprs.len() {
        if let Expr::Call { callee, .. } = a.ast.get_expr(ExprId(i as u32)) {
            callees.insert(*callee);
        }
    }
    for i in 0..a.ast.exprs.len() {
        let eid = ExprId(i as u32);
        match a.ast.get_expr(eid) {
            Expr::Ident(n) if !callees.contains(&eid) => {
                out.remove(n);
            }
            Expr::Closure { fn_name, .. } => {
                out.remove(fn_name);
            }
            _ => {}
        }
    }
    for f in a.retargets.values() {
        out.remove(f);
    }
    out
}
