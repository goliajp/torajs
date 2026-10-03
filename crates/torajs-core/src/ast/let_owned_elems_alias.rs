//! The alias classes of the owned-elements census
//! ([`super::let_owned_elems`]): which bindings only ever hand a
//! split product to each other, and so may all carry it as views.
//!
//! `let Y = X` and `Y = X` — with `X` bare, through the forms
//! `is_bare` sees through — used to count as an escape of `X` and a
//! rebind of `Y`, and both were materialized (`last = parts` in a
//! loop copied every row's views into owned strings). Neither side
//! needs it when the two slots agree on the layout: the assignment is
//! a share of the same block. The census therefore connects the two
//! names by an edge and rules on the whole class at once.
//!
//! A class holds views when every member is a candidate (its init is
//! a one-argument `.split(sep)` call, an empty `[]`, or a bare
//! alias of other members, under no annotation or an owned-string
//! array one), no member is written or escapes other than along an
//! edge, and at least one member's init is a split call — otherwise
//! there is nothing to keep as views and the class keeps today's
//! verdicts. Poison is a class property: one owned member makes every
//! edge into it a real escape again.
//!
//! Names are program-wide, not scoped — the same bias as the window
//! walk. Two bindings spelled alike are one class, and a name any
//! other binder also declares (a parameter, a loop variable, a catch
//! parameter, an import, a function or class) is never connected,
//! since a slot resolved to that binder is typed by its own
//! declaration. A top-level binding a named function body reads may
//! be promoted to a data global typed by the annotation lane, so it
//! is poisoned too. The binding lane still asks the checker's types
//! before trusting a class ([`crate::ssa_lower_stmt_let_decl_view`]):
//! a split on an `any` receiver answers something other than views,
//! and the census cannot see types.

use std::collections::{HashMap, HashSet};

pub(super) use super::let_owned_elems_names::{AliasNames, collect_names};
use super::{Ast, BinOp, Expr, ExprId};

/// The bare names a value answers one of unchanged — the forms
/// `is_bare` sees through — or None when some leaf is anything else.
fn alias_leaves<'a>(ast: &'a Ast, eid: ExprId, out: &mut Vec<&'a str>) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => {
            out.push(n);
            true
        }
        Expr::As { expr, .. } => alias_leaves(ast, *expr, out),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => alias_leaves(ast, *then_branch, out) && alias_leaves(ast, *else_branch, out),
        Expr::Nullish { lhs, rhs } => alias_leaves(ast, *lhs, out) && alias_leaves(ast, *rhs, out),
        Expr::BinOp {
            op: BinOp::LAnd | BinOp::LOr,
            left,
            right,
        } => alias_leaves(ast, *left, out) && alias_leaves(ast, *right, out),
        Expr::Sequence { right, .. } => alias_leaves(ast, *right, out),
        _ => false,
    }
}

/// `target = value` / `let target = value` is an alias edge when the
/// target and every leaf of the value are connectable names.
pub(super) fn alias_edge<'a>(
    ast: &'a Ast,
    names: &AliasNames,
    target: &str,
    value: ExprId,
) -> Option<Vec<&'a str>> {
    if !names.aliasable(target) {
        return None;
    }
    let mut leaves = Vec::new();
    (alias_leaves(ast, value, &mut leaves) && leaves.iter().all(|n| names.aliasable(n)))
        .then_some(leaves)
}

/// A one-argument `.split(sep)` call: the shape the lowering answers
/// views for when the checker types both sides `string`. A limit
/// argument copies the prefix out as owned strings and releases the
/// block (`ssa_lower_str_str_split::copy_prefix`), so that product is
/// an `Arr<Str>` and can never share a view slot.
pub(super) fn is_split_call(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Call { callee, args }
        if matches!(ast.get_expr(*callee), Expr::Member { name, .. } if name == "split")
            && args.len() == 1)
}

/// May this declaration carry views: an owned-string array annotation
/// or none, and an init that is a split call, an empty literal, or a
/// bare alias of connectable names.
pub(super) fn is_candidate(
    ast: &Ast,
    names: &AliasNames,
    name: &str,
    type_ann: Option<&str>,
    init: ExprId,
) -> bool {
    if !matches!(type_ann, None | Some("string[]") | Some("Array<string>")) {
        return false;
    }
    // an unannotated `[]` is `Array<any>` to the checker, and an any
    // slot never carries the view layout; a split or an alias init
    // takes its type from the value
    is_split_call(ast, init)
        || (type_ann.is_some() && matches!(ast.get_expr(init), Expr::Array(els) if els.is_empty()))
        || alias_edge(ast, names, name, init).is_some()
}

/// One binding the window walk ruled on.
pub(super) struct Decl {
    pub name: String,
    pub init: ExprId,
    pub candidate: bool,
    /// Written into or escaping somewhere in its window, alias edges
    /// included — today's verdict.
    pub written: bool,
    /// The same, alias edges excluded.
    pub poisoned: bool,
}

/// Union-find over names.
struct Classes {
    parent: HashMap<String, String>,
}

impl Classes {
    fn find(&mut self, n: &str) -> String {
        let Some(p) = self.parent.get(n).cloned() else {
            return n.to_string();
        };
        let root = self.find(&p);
        if root != p {
            self.parent.insert(n.to_string(), root.clone());
        }
        root
    }

    fn union(&mut self, a: &str, b: &str) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }
}

/// Rule on every class: the inits that hold owned elements, and the
/// members of the classes that hold views (each init mapped to its
/// whole class, itself included).
pub(super) fn settle(
    ast: &Ast,
    names: &AliasNames,
    decls: &[Decl],
) -> (HashSet<ExprId>, HashMap<ExprId, Vec<ExprId>>) {
    let mut classes = Classes {
        parent: HashMap::new(),
    };
    for (name, init) in &names.decls {
        for leaf in alias_edge(ast, names, name, *init).unwrap_or_default() {
            classes.union(name, leaf);
        }
    }
    for e in &ast.exprs {
        if let Expr::Assign { target, value } = e
            && let Expr::Ident(t) = ast.get_expr(*target)
        {
            for leaf in alias_edge(ast, names, t, *value).unwrap_or_default() {
                classes.union(t, leaf);
            }
        }
    }
    // a name the window walk saw fewer declarations of than the
    // program holds has a binding nobody ruled on
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for d in decls {
        *seen.entry(d.name.as_str()).or_insert(0) += 1;
    }
    let mut poisoned: HashSet<String> = HashSet::new();
    let mut has_split: HashSet<String> = HashSet::new();
    for d in decls {
        let root = classes.find(&d.name);
        if d.poisoned
            || !d.candidate
            || !names.aliasable(&d.name)
            || names.promoted.contains(&d.name)
            || seen.get(d.name.as_str()) != names.let_counts.get(&d.name)
        {
            poisoned.insert(root.clone());
        }
        if is_split_call(ast, d.init) {
            has_split.insert(root);
        }
    }
    let mut members: HashMap<String, Vec<ExprId>> = HashMap::new();
    let mut owned = HashSet::new();
    for d in decls {
        let root = classes.find(&d.name);
        if !poisoned.contains(&root) && has_split.contains(&root) {
            members.entry(root).or_default().push(d.init);
        } else if d.written {
            owned.insert(d.init);
        }
    }
    let view_slots = members
        .values()
        .flat_map(|m| m.iter().map(move |i| (*i, m.clone())))
        .collect();
    (owned, view_slots)
}
