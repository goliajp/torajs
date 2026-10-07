//! The arrays handed to a builtin array iteration together with a
//! callback, and which of them escape into the `any` world.
//!
//! A typed array whose alias class escapes lowers as an `Arr<Any>`
//! (`ssa_lower_container_width::widen_arr_elem`), so an alias written
//! through `any` can leave any value in it while the checker still
//! reads the array by its declared element type. The callback of
//! `d.map(cb)` then receives values its parameter type does not
//! describe, and the check pipeline mints it an all-`any` twin
//! (`check_monomorph_any_widen::struct_escape::escaped_cb`). The keys
//! are recorded during the walk, where the scope that resolves them is
//! known, and read once the union-find is frozen.

use std::collections::HashSet;

use super::container::canon_key_frozen;
use super::sentinel_hof::{callback_fn_name, taint_param};
use super::{Analysis, Scope, SlotKey};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;

impl Analysis<'_> {
    /// The Call arm of `walk_expr`: record the array a callback-taking
    /// iteration walks, then walk the call.
    pub(super) fn walk_call_site(
        &mut self,
        eid: ExprId,
        callee: ExprId,
        args: &[ExprId],
        scope: &Scope,
    ) {
        if let Some(recv) = iterated_array(self.ast, callee, args)
            && let Some(k) = self.container_key_lookup(recv, scope)
        {
            self.hof_receivers.push((eid, recv, k));
        }
        self.walk_call(eid, callee, args, scope);
    }
}

/// The array a call hands its callback the elements of: the receiver
/// of an array iteration method, or the source of `Array.from(src, cb)`.
fn iterated_array(ast: &Ast, callee: ExprId, args: &[ExprId]) -> Option<ExprId> {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return None;
    };
    match name.as_str() {
        "map" | "filter" | "reduce" | "reduceRight" | "forEach" | "find" | "findIndex"
        | "findLast" | "findLastIndex" | "some" | "every" | "flatMap" | "sort" => Some(*obj),
        "from"
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Array") && args.len() >= 2 =>
        {
            Some(args[0])
        }
        _ => None,
    }
}

/// Close the escape faces over the iterations: propagate the `any`
/// and escape-only seeds (`escape::propagate`), and when an iteration
/// that fills a product walks `any` elements, the product joins them.
/// The product holds what the callback answers, and a callback fed
/// `any` elements through a typed first parameter answers through its
/// all-`any` twin in any kind (`d.map(v => v + 1)` on a "5" element is
/// "51"), so the product lowers as an `Arr<Any>` too, with every alias
/// of it. Returns the escaped classes and the recorded arrays among
/// them.
///
/// A callback over such an array, or over an `any[]`, can be handed
/// `undefined` as its element, which a `number` slot holds as the
/// undefined sentinel (`ssa_lower_boxed_entry::unbox`); its element
/// parameter joins the sentinel-capable ones, so a read that asks
/// (`typeof v`, printing `v`) answers `undefined`, not NaN.
pub(super) fn settle(
    a: &Analysis<'_>,
    undef_params: &mut HashSet<(String, String)>,
) -> (HashSet<SlotKey>, HashSet<ExprId>) {
    let mut seeds = [&a.any_seeds[..], &a.escape_only_seeds[..]].concat();
    let (any_escaped, escaped) = loop {
        let any_escaped = super::escape::propagate(&seeds, &a.uf, &a.containerish);
        let escaped: HashSet<ExprId> = a
            .hof_receivers
            .iter()
            .filter(|(_, _, k)| any_escaped.contains(&canon_key_frozen(&a.uf, k)))
            .map(|(_, recv, _)| *recv)
            .collect();
        let products: Vec<SlotKey> = a
            .hof_receivers
            .iter()
            .filter(|(call, recv, _)| any_elems(a, &escaped, *recv) && fills_any(a, *call))
            .map(|(call, _, _)| SlotKey::Anon(call.0))
            .filter(|k| !any_escaped.contains(&canon_key_frozen(&a.uf, k)))
            .collect();
        if products.is_empty() {
            break (any_escaped, escaped);
        }
        seeds.extend(products);
    };
    for e in &a.ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Some(recv) = iterated_array(a.ast, *callee, args) else {
            continue;
        };
        let cb = if args.first() == Some(&recv) {
            args.get(1)
        } else {
            args.first()
        };
        if any_elems(a, &escaped, recv)
            && let (Some(idx), Some(&cb)) = (elem_param(a.ast, *callee), cb)
        {
            taint_param(a, &callback_fn_name(a, cb), idx, undef_params);
        }
    }
    (any_escaped, escaped)
}

/// Does `recv` hand its callbacks `any` elements: an escaped typed
/// array, or an `any[]`?
fn any_elems(a: &Analysis<'_>, escaped: &HashSet<ExprId>, recv: ExprId) -> bool {
    escaped.contains(&recv)
        || matches!(a.expr_types.get(&recv), Some(Type::Array(el)) if **el == Type::Any)
}

/// Is `call` a `map` / `flatMap` / `Array.from` whose product a lifted
/// closure with a typed first parameter fills? Only such a callback
/// answers through a twin, in a kind its declared return does not
/// spell. A function declaration passed by name has no twin, and a
/// callback taking `any` already answers in its declared kind for
/// every element; either leaves the product as typed as before.
fn fills_any(a: &Analysis<'_>, call: ExprId) -> bool {
    let Expr::Call { callee, args } = a.ast.get_expr(call) else {
        return false;
    };
    let cb = match a.ast.get_expr(*callee) {
        Expr::Member { name, .. } if name == "map" || name == "flatMap" => args.first(),
        Expr::Member { name, .. } if name == "from" => args.get(1),
        _ => None,
    };
    let Some(&cb) = cb else {
        return false;
    };
    let fns = callback_fn_name(a, cb);
    !fns.is_empty()
        && fns.iter().all(|f| f.starts_with("__closure_"))
        && matches!(a.expr_types.get(&cb), Some(Type::Function(ps, _))
            if ps.first().is_some_and(|p| *p != Type::Any))
}

/// The callback parameter an iteration hands the element to. `sort`
/// takes none: §23.1.3.30 sorts `undefined` to the end without calling
/// the comparator.
fn elem_param(ast: &Ast, callee: ExprId) -> Option<usize> {
    let Expr::Member { name, .. } = ast.get_expr(callee) else {
        return None;
    };
    match name.as_str() {
        "reduce" | "reduceRight" => Some(1),
        "sort" => None,
        _ => Some(0),
    }
}
