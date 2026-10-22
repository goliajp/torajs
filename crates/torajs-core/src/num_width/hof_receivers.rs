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

use super::bound_closures::callback_fn_name;
use super::container::canon_key_frozen;
use super::sentinel_hof::taint_param;
use super::{Analysis, KeySet, Scope, SlotKeyId};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;
use crate::check_monomorph_any_widen::struct_escape::twin_name;

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
pub(crate) fn iterated_array(ast: &Ast, callee: ExprId, args: &[ExprId]) -> Option<ExprId> {
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
/// and escape-only seeds (`escape_propagate::propagate`), and when an
/// iteration that fills a product walks `any` elements, the product
/// joins them.
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
/// (`typeof v`, printing `v`) answers `undefined`, not NaN. The last
/// part answers what such values reach (`hidden_any`), from the element
/// parameters and the `any` parameters of the widened clones; a
/// container one is stored into escapes too, and the faces close again.
pub(super) fn settle(
    a: &Analysis<'_>,
    undef_params: &mut HashSet<(String, String)>,
) -> (KeySet, HashSet<ExprId>, super::HiddenAny) {
    let mut seeds = [&a.any_seeds[..], &a.escape_only_seeds[..]].concat();
    loop {
        let (any_escaped, escaped) = close_products(a, &mut seeds);
        let elem_params = elem_params(a, &escaped);
        let mut sources = elem_params.clone();
        sources.extend(super::hidden_any_seeds::widened_params(a.ast));
        let hidden = super::hidden_any::collect(a, &any_escaped, &sources);
        if hidden.filled.is_empty() {
            undef_params.extend(elem_params);
            return (any_escaped, escaped, hidden);
        }
        seeds.extend(hidden.filled);
    }
}

/// Propagate `seeds` until no iteration product joins them; answers the
/// escaped classes and the recorded arrays among them.
fn close_products(a: &Analysis<'_>, seeds: &mut Vec<SlotKeyId>) -> (KeySet, HashSet<ExprId>) {
    loop {
        let any_escaped =
            super::escape_propagate::propagate(seeds, &a.uf, &a.keys, &a.containerish);
        let escaped: HashSet<ExprId> = a
            .hof_receivers
            .iter()
            .filter(|(_, _, k)| any_escaped.contains(&canon_key_frozen(&a.uf, &a.keys, *k)))
            .map(|(_, recv, _)| *recv)
            .collect();
        let products: Vec<SlotKeyId> = a
            .hof_receivers
            .iter()
            .filter(|(call, recv, _)| any_elems(a, &escaped, *recv) && fills_any(a, *call))
            .map(|(call, _, _)| a.keys.anon(call.0))
            .filter(|k| !any_escaped.contains(&canon_key_frozen(&a.uf, &a.keys, *k)))
            .collect();
        if products.is_empty() {
            return (any_escaped, escaped);
        }
        seeds.extend(products);
    }
}

/// The callback parameters an iteration over `escaped` or an `any[]`
/// hands its elements to.
fn elem_params(a: &Analysis<'_>, escaped: &HashSet<ExprId>) -> HashSet<(String, String)> {
    let mut elem_params = HashSet::new();
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
        if any_elems(a, escaped, recv)
            && let (Some(idx), Some(&cb)) = (elem_param(a.ast, *callee), cb)
        {
            taint_param(a, &callback_fn_name(a, cb), idx, &mut elem_params);
        }
    }
    elem_params
}

/// Does `recv` hand its callbacks `any` elements: an escaped typed
/// array, or an `any[]`?
fn any_elems(a: &Analysis<'_>, escaped: &HashSet<ExprId>, recv: ExprId) -> bool {
    escaped.contains(&recv)
        || matches!(a.expr_types.get(&recv), Some(Type::Array(el)) if **el == Type::Any)
}

/// Is `call` a `map` / `flatMap` / `Array.from` whose product a lifted
/// closure or a relay shell with a typed first parameter fills? Only
/// such a callback answers through a twin, in a kind its declared
/// return does not spell (a function declaration passed by name gets
/// its shell first, `struct_escape::named_cb`; a shell whose twin
/// did not check answers through its typed face). A callback taking
/// `any` already answers in its declared kind for every element, and
/// leaves the product as typed as before.
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
        && fns.iter().all(|f| {
            f.starts_with("__closure_")
                || (crate::ast::is_shell(f) && a.fn_params.contains_key(&twin_name(f)))
        })
        && matches!(a.expr_types.get(&cb), Some(Type::Function(ps, _))
            if ps.first().is_some_and(boxed_slot))
}

/// A first parameter the lowering sends through the boxed entry when an
/// any element meets it (`ssa_lower_call_arr_ho::cb_face`): a scalar or
/// a struct slot. Any other kind keeps the direct call and a typed
/// product, so the product must not be marked as an any one either.
fn boxed_slot(p: &Type) -> bool {
    matches!(
        p,
        Type::Number | Type::String | Type::Boolean | Type::Struct(_) | Type::Object(_)
    )
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
