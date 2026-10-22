//! Which lifted closures an identifier read may denote — the callback
//! spelled through a binding (`const show = (v: number) => …;
//! p.then(show)` / `xs.forEach(show)`), and a call through one.
//!
//! The callback wirings and the sentinel tables need the closure's
//! parameter list, and the call site only spells the binding. Mapping
//! the spelling program-wide either refused every name bound more than
//! once (two functions each with their own `cb`: the element width
//! never reached either closure's parameter, so `xs.map(cb)` read 1.5
//! back as 1) or let the first binding of the name win. Each read is
//! resolved instead in the body it sits in, to the binding it reads —
//! the same resolution the constraint walk uses, captures included.
//!
//! One binding key can still hold several closures: same-named `const`s
//! in sibling blocks of one function share their `Local` key. Such a
//! read answers every candidate, and the candidates' parameter and
//! return slots are joined so that wiring the first one wires them all.

use super::captures::{Body, CaptureMap};
use super::{Analysis, SlotKeyData, SlotKeyId, SlotKeys};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The closures a read may denote, sorted.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Candidates {
    pub(crate) fns: Vec<String>,
    /// Every declaration of every binding on the way is an immutable
    /// closure (or an alias of one), so the set is the whole story and
    /// may stand in for the value at a call site. The sentinel tables
    /// and the promise adapters, which only ever add, take a partial
    /// set too.
    pub(crate) exact: bool,
}

pub(super) type BoundClosures = HashMap<ExprId, Candidates>;

/// The closures each read may denote, and the reads of a function
/// declaration's name that a binding answers instead
/// ([`shadowing_reads`]).
pub(super) fn collect(
    ast: &Ast,
    bodies: &[Body<'_>],
    toplevel_lets: &HashSet<String>,
    captures: &CaptureMap,
    keys: &SlotKeys,
) -> (BoundClosures, HashSet<ExprId>) {
    // binding key → its declarations; `None` = a binding that is not a
    // `let` / `const` (a for-of element, a catch param)
    let mut decls: HashMap<SlotKeyId, Vec<Option<(ExprId, bool)>>> = HashMap::new();
    // read → the binding it resolves to; `None` once two bodies disagree
    let mut reads: HashMap<ExprId, Option<SlotKeyId>> = HashMap::new();
    for b in bodies {
        b.each_binding(&mut |n, decl| {
            let d = match decl {
                Some(Stmt::LetDecl {
                    init,
                    mutable,
                    is_var,
                    ..
                }) => Some((*init, !*mutable && !*is_var)),
                _ => None,
            };
            decls.entry(b.binding_key(keys, n)).or_default().push(d);
        });
        b.each_expr(ast, &mut |eid| {
            if let Expr::Ident(n) = ast.get_expr(eid)
                && let Some(k) = b.resolve(keys, toplevel_lets, captures, n)
            {
                let e = reads.entry(eid).or_insert(Some(k));
                if *e != Some(k) {
                    *e = None;
                }
            }
        });
    }
    // `const j = h` reads `h` through its own entry, so the map grows
    // one alias hop per pass; the sets only grow, bounded by the
    // closures in the program
    let mut out = BoundClosures::new();
    for pass in 0.. {
        debug_assert!(pass <= decls.len() + 1, "bound-closure fixpoint diverged");
        let mut by_key: HashMap<SlotKeyId, Candidates> = HashMap::new();
        for (k, ds) in &decls {
            let mut fns = BTreeSet::new();
            let mut exact = true;
            for d in ds {
                let Some((init, imm)) = d else {
                    exact = false;
                    continue;
                };
                exact &= *imm;
                match ast.get_expr(*init) {
                    Expr::Closure { fn_name, .. } => {
                        fns.insert(fn_name.clone());
                    }
                    Expr::Ident(_) if out.contains_key(init) => {
                        fns.extend(out[init].fns.iter().cloned());
                        exact &= out[init].exact;
                    }
                    _ => exact = false,
                }
            }
            if !fns.is_empty() {
                let fns = fns.into_iter().collect();
                by_key.insert(*k, Candidates { fns, exact });
            }
        }
        let next: BoundClosures = reads
            .iter()
            .filter_map(|(eid, k)| Some((*eid, by_key.get(&(*k)?)?.clone())))
            .collect();
        if next == out {
            break;
        }
        out = next;
    }
    (out, shadowing_reads(ast, keys, &reads))
}

/// The reads of a function declaration's name that resolve to a binding
/// of the same name in a function body: a parameter, a local, or a
/// capture of one. Lowering calls that binding (§9.1.2.1: the innermost
/// environment wins), so such a read names no declaration. A top-level
/// `let` keeps the declaration's reading, as the lowering's identifier
/// read does (`ssa_lower_ident::lower` takes the function before a
/// top-level data slot).
fn shadowing_reads(
    ast: &Ast,
    keys: &SlotKeys,
    reads: &HashMap<ExprId, Option<SlotKeyId>>,
) -> HashSet<ExprId> {
    let decls: HashSet<&str> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    reads
        .iter()
        .filter(|(eid, k)| {
            !matches!(k.map(|k| keys.data(k)), Some(SlotKeyData::Global(_)))
                && matches!(ast.get_expr(**eid), Expr::Ident(n) if decls.contains(n.as_str()))
        })
        .map(|(eid, _)| *eid)
        .collect()
}

/// The FnDecls an argument in callback position may name. An arrow is
/// lifted to a `__closure_N` before this analysis runs, so a spelled
/// binding resolves to every closure it may hold, and a binding
/// shadowing a declaration with none names no FnDecl.
pub(super) fn callback_fn_name(a: &Analysis<'_>, cb: ExprId) -> Vec<String> {
    match a.ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => vec![fn_name.clone()],
        Expr::Ident(n) => match a.bound_closures.get(&cb) {
            Some(c) => c.fns.clone(),
            None if a.shadowing_reads.contains(&cb) => Vec::new(),
            None => vec![n.clone()],
        },
        _ => Vec::new(),
    }
}

impl Analysis<'_> {
    /// The fn a callee or callback expression names: a closure literal,
    /// a binding that holds closures (their first — [`Self::join_bound_candidates`]
    /// made the rest share its slots), or a function declaration no
    /// binding shadows at `eid`.
    pub(super) fn callee_fn_name(&self, eid: ExprId) -> Option<String> {
        if let Some(c) = self.bound_closures.get(&eid)
            && c.exact
        {
            return c.fns.first().cloned();
        }
        match self.ast.get_expr(eid) {
            Expr::Ident(n)
                if self.fn_params.contains_key(n) && !self.shadowing_reads.contains(&eid) =>
            {
                Some(n.clone())
            }
            Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
            _ => None,
        }
    }

    /// The function a call at `eid` with callee `callee` lands on by
    /// name: a monomorphized instance, or a function declaration that no
    /// parameter, local or capture of the same name shadows there.
    pub(super) fn direct_callee(&self, eid: ExprId, callee: ExprId) -> Option<String> {
        if let Some(mono) = self.retargets.get(&eid) {
            return Some(mono.clone());
        }
        match self.ast.get_expr(callee) {
            Expr::Ident(n)
                if self.fn_params.contains_key(n) && !self.shadowing_reads.contains(&callee) =>
            {
                Some(n.clone())
            }
            _ => None,
        }
    }

    /// Join the parameter and return slots of the closures one binding
    /// may hold: a call through the binding reaches any of them with
    /// the same arguments and hands back whichever one ran.
    pub(super) fn join_bound_candidates(&mut self) {
        let sets: HashSet<Vec<String>> = self
            .bound_closures
            .values()
            .filter(|c| c.exact && c.fns.len() > 1)
            .map(|c| c.fns.clone())
            .collect();
        for fns in sets {
            let first = &fns[0];
            let ps0 = self.callback_params(first);
            for f in &fns[1..] {
                let (r0, r) = (self.keys.ret(first), self.keys.ret(f));
                self.uf.union(&self.keys, r0, r);
                for (p0, p) in ps0.iter().zip(self.callback_params(f)) {
                    let (a, b) = (self.keys.param(first, p0), self.keys.param(f, &p));
                    self.uf.union(&self.keys, a, b);
                }
            }
        }
    }
}
