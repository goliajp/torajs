//! The promise value points a promise can reach by a path the walk
//! does not follow.
//!
//! A reader of a `Promise<number>` (an `await`, a `.then` handler's
//! parameter, a combinator's result element) takes the settled value
//! at the width of its value point, the `value` field of the promise's
//! slot, and the runtime converts an f64 into an i64 reader without a
//! check (`torajs-promise` `settled_form`). The point is integral only
//! if every promise that can reach it settles with an integer. The walk
//! joins every promise it follows into the slots it reaches; what it
//! cannot follow is a promise that comes from outside the program's
//! types: one held by a value typed `any` (or holding `any` under an
//! array, a promise, a union, an object type), or one written into a
//! container through an `any` alias of it.
//!
//! So the point is not integral by default whenever its slot's class
//! holds such a source: every value the walk keys while its type holds
//! `any` is a source, as is every slot an `any` face was declared on
//! (`escape.rs`'s seeds). The sources close over the alias classes and
//! the alias pairs the walk recorded, an untracked container's elements
//! and program-named fields being untracked too, and every `value`
//! point of a promise of a number held by a reached class widens. A
//! point only typed promises reach keeps the width its writers give it.
//!
//! The walk keys a value wherever it is written into a slot (an
//! initializer, an assignment, an argument, a return, a container
//! method's operand, a literal's element or field), through
//! `container_key_of`; that is where the sources are noted.

use super::container::canon_key_frozen;
use super::{Analysis, SlotKey};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;
use std::collections::{HashMap, HashSet};

/// Does a value of type `t` come from, or hold, a value whose type the
/// program did not fix? A function value does not by itself: its
/// parameters and its result are expressions of their own.
fn untracked(t: &Type) -> bool {
    match t {
        Type::Any | Type::TypeVar(_) => true,
        Type::Array(e) | Type::Promise(e) | Type::Nullable(e) | Type::MapIter(e) => untracked(e),
        Type::Rest(e) => untracked(e),
        Type::Map(k, v) | Type::Dict(k, v) => untracked(k) || untracked(v),
        Type::Union(ts) => (ts.iter()).any(|t| matches!(t, Type::Promise(_)) || untracked(t)),
        Type::Struct(fs) => fs.iter().any(|(_, t)| untracked(t)),
        _ => false,
    }
}

/// Does the program read a number out of a promise: an expression of
/// type `Promise<number>`, or a handler declaring a `number` parameter
/// (a promise of `any` hands it whatever it settles with)? A program
/// that does not has no value point at stake.
pub(super) fn reads_number_promises(ast: &Ast, expr_types: &HashMap<ExprId, Type>) -> bool {
    let number = |t: &Type| matches!(t, Type::Promise(t) if **t == Type::Number);
    let number_param = |a: &ExprId| matches!(expr_types.get(a), Some(Type::Function(ps, _)) if ps.first() == Some(&Type::Number));
    let handler = |e: &Expr| match e {
        Expr::Call { callee, args } => {
            matches!(ast.get_expr(*callee), Expr::Member { name, .. } if name == "then" || name == "catch")
                && args.iter().any(number_param)
        }
        _ => false,
    };
    (expr_types.values()).any(number) || ast.exprs.iter().any(handler)
}

/// Is the holder `h` of a promise value point in an untracked class:
/// reached itself, or the element or program-named field of one that
/// is?
pub(super) fn holder_untracked(
    tainted: &HashSet<SlotKey>,
    uf: &super::container::UnionFind,
    h: &SlotKey,
) -> bool {
    if tainted.is_empty() {
        return false;
    }
    if tainted.contains(&canon_key_frozen(uf, h)) {
        return true;
    }
    match h {
        SlotKey::Elem(g) => holder_untracked(tainted, uf, g),
        SlotKey::Field(g, f) if !f.lossy().starts_with("__") => holder_untracked(tainted, uf, g),
        _ => false,
    }
}

impl Analysis<'_> {
    /// Note `k`, the key the walk gave the value expression `eid`, as
    /// a source the walk does not follow when `eid`'s type holds `any`,
    /// and as the slot of a promise of a number when it is one.
    pub(super) fn note_untracked_value(&mut self, eid: ExprId, k: &SlotKey) {
        if !self.promises {
            return;
        }
        let unions = |e| self.union_types.get(e).is_some_and(untracked);
        let ty = self.expr_types.get(&eid);
        if ty.is_some_and(untracked) || unions(&eid) {
            self.untracked_values.insert(k.clone());
        }
        if let Some(Type::Promise(t)) = ty
            && **t == Type::Number
        {
            self.number_promises.insert(k.clone());
        }
    }
}

/// After `canonicalize`: take the frozen classes of the untracked
/// sources, close them over every alias pair the walk recorded (`alias_pairs`,
/// joined or not: a guarded pair joins only on container evidence, and
/// a promise slot has none, yet the two sides hold the same promise),
/// then seed every `value` point they hold that a number is read
/// from: only a number's width is at stake, and a value point a slot
/// of another type joins (a handler's `any` parameter, an iterator
/// step's `value`, a container) must keep its form. Answers the
/// reached class representatives, which the table keeps for the
/// lowering's own `await` reads (`promise_value_is_f64`); none in a
/// program that has no promise to read.
pub(super) fn seed_untracked_points(
    a: &mut Analysis,
    alias_pairs: &[(SlotKey, SlotKey)],
) -> HashSet<SlotKey> {
    if !a.promises {
        return HashSet::new();
    }
    // a slot the desugar spelled carries what the compiler routes
    // through it, in the form it was handed (`escape_propagate`)
    let sources = (a.any_seeds.iter())
        .chain(&a.escape_only_seeds)
        .chain(&a.untracked_values)
        .filter(|k| !super::escape_propagate::synthetic(k));
    let mut tainted: HashSet<SlotKey> = sources.map(|k| canon_key_frozen(&a.uf, k)).collect();
    if tainted.is_empty() {
        return tainted;
    }
    close_over_pairs(&mut tainted, &a.uf, alias_pairs);
    let value = |k: &SlotKey| SlotKey::Field(Box::new(k.clone()), "value".into());
    // a class that also holds a container (a rejection's `Error` shares
    // the point with the value it would have fulfilled with) is that
    // container's too, and keeps its form
    let held: HashSet<SlotKey> = (a.containerish.iter())
        .map(|k| canon_key_frozen(&a.uf, k))
        .collect();
    let numbers: HashSet<SlotKey> = (a.number_promises.iter())
        .map(|k| canon_key_frozen(&a.uf, &value(k)))
        .filter(|k| !held.contains(k))
        .collect();
    let points: Vec<SlotKey> = (a.uf.keys())
        .filter_map(|k| match k {
            SlotKey::Field(h, n) if n.lossy() == "value" => Some((h, canon_key_frozen(&a.uf, k))),
            _ => None,
        })
        .filter(|(h, c)| numbers.contains(c) && holder_untracked(&tainted, &a.uf, h))
        .map(|(_, c)| c)
        .collect();
    a.seeds.extend(points);
    tainted
}

/// The canonical keys whose class taints `k`: its own, and those of
/// the arrays and program-named fields holding it.
fn holder_chain(uf: &super::container::UnionFind, k: &SlotKey) -> Vec<SlotKey> {
    let mut out = vec![canon_key_frozen(uf, k)];
    let mut h = k;
    while let SlotKey::Elem(g) | SlotKey::Field(g, _) = h {
        if matches!(h, SlotKey::Field(_, f) if f.lossy().starts_with("__")) {
            break;
        }
        out.push(canon_key_frozen(uf, g));
        h = g;
    }
    out
}

/// Close `tainted` over the alias pairs: a pair one side of which is
/// held by a tainted class taints the other side's class. Each pair is
/// filed under every key whose taint reaches it, so each newly tainted
/// class is visited once.
fn close_over_pairs(
    tainted: &mut HashSet<SlotKey>,
    uf: &super::container::UnionFind,
    pairs: &[(SlotKey, SlotKey)],
) {
    let mut by_holder: HashMap<SlotKey, Vec<SlotKey>> = HashMap::new();
    for (x, y) in pairs {
        for (from, to) in [(x, y), (y, x)] {
            let to = canon_key_frozen(uf, to);
            for c in holder_chain(uf, from) {
                by_holder.entry(c).or_default().push(to.clone());
            }
        }
    }
    let mut work: Vec<SlotKey> = tainted.iter().cloned().collect();
    while let Some(k) = work.pop() {
        for to in by_holder.get(&k).into_iter().flatten() {
            if tainted.insert(to.clone()) {
                work.push(to.clone());
            }
        }
    }
}
