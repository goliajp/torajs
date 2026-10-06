//! Which property creation orders the cells of a layout can have
//! besides the layout's own.
//!
//! Only two things create a cell in an order other than its layout's:
//! a literal the checker put in a declared type's order (its recorded
//! type is in the storage order, its source spells the creation
//! order), and a spread whose source is such a cell. Every other
//! literal is stored in the order it creates its properties, so a
//! program without reordered literals records nothing here and every
//! layout keeps its compile-time enumeration.

use std::collections::HashMap;

use super::AnonStampPool;
use crate::ast::{Ast, Expr, ExprId, PropKey};
use crate::check::Type as CType;
use crate::ssa::Type;

/// The most source-order combinations a spread literal is expanded
/// into, and the most creation orders recorded per layout. A spread
/// literal past it records nothing and its lowering mints no order
/// tag, so the two sides always agree on which tags exist.
pub(crate) const MAX_COMBOS: usize = 64;

impl AnonStampPool {
    /// Record the foreign creation orders of every object literal the
    /// checker typed, keyed by the literal's storage order.
    pub fn note_objlit_orders(&mut self, ast: &Ast, expr_types: &HashMap<ExprId, CType>) {
        let mut spread_lits: Vec<(&[(PropKey, ExprId)], Vec<PropKey>)> = Vec::new();
        for (eid, ty) in expr_types {
            let CType::Struct(fields) = ty else {
                continue;
            };
            let Expr::ObjectLit { fields: entries } = ast.get_expr(*eid) else {
                continue;
            };
            let storage: Vec<PropKey> = fields.iter().map(|(n, _)| n.clone()).collect();
            if entries
                .iter()
                .any(|(n, _)| crate::check_type_of_object_lit::spread_omit_set(n).is_some())
            {
                spread_lits.push((entries, storage));
            } else if let Some(created) = plain_objlit_order(entries) {
                self.add_order(storage, created);
            }
        }
        // a spread of a spread result needs the inner literal's orders
        // first: repeat until nothing new is recorded (finite, since
        // each layout holds at most MAX_COMBOS orders)
        loop {
            let mut grew = false;
            for (entries, storage) in &spread_lits {
                for created in self.spread_literal_orders(expr_types, entries) {
                    grew |= self.add_order(storage.clone(), created);
                }
            }
            if !grew {
                break;
            }
        }
        for orders in self.objlit_orders.values_mut() {
            orders.sort();
        }
        self.orders_noted = true;
    }

    /// Record `created` as a creation order of the layout spelled
    /// `storage` when it differs from it (same names, other order).
    pub(super) fn add_order(&mut self, storage: Vec<PropKey>, created: Vec<PropKey>) -> bool {
        if created == storage
            || created.len() != storage.len()
            || !created.iter().all(|n| storage.contains(n))
        {
            return false;
        }
        let orders = self.objlit_orders.entry(storage).or_default();
        if orders.contains(&created) || orders.len() >= MAX_COMBOS {
            return false;
        }
        orders.push(created);
        true
    }

    /// Every creation order of a literal with spread members, one per
    /// combination of its sources' creation orders; none when no
    /// source has another order or the combinations pass MAX_COMBOS.
    fn spread_literal_orders(
        &self,
        expr_types: &HashMap<ExprId, CType>,
        entries: &[(PropKey, ExprId)],
    ) -> Vec<Vec<PropKey>> {
        // per entry: the name runs it can contribute
        let mut runs: Vec<Vec<Vec<PropKey>>> = Vec::new();
        for (n, eid) in entries {
            let Some(omit) = crate::check_type_of_object_lit::spread_omit_set(n) else {
                runs.push(vec![vec![n.clone()]]);
                continue;
            };
            let Some(CType::Struct(src)) = expr_types.get(eid) else {
                return Vec::new();
            };
            let names: Vec<PropKey> = src.iter().map(|(n, _)| n.clone()).collect();
            let mut orders = vec![names.clone()];
            if let Some(more) = self.objlit_orders.get(&names) {
                orders.extend(more.iter().cloned());
            }
            let omit: Vec<PropKey> = omit.iter().map(|s| PropKey::from(*s)).collect();
            runs.push(
                orders
                    .iter()
                    .map(|o| spread_contribution(o, &omit))
                    .collect(),
            );
        }
        let total = runs.iter().map(Vec::len).product::<usize>();
        if total == 1 || total > MAX_COMBOS {
            return Vec::new();
        }
        let mut combo = vec![0usize; runs.len()];
        let mut out = Vec::new();
        loop {
            let mut created: Vec<PropKey> = Vec::new();
            for (k, r) in runs.iter().enumerate() {
                for n in &r[combo[k]] {
                    if !created.contains(n) {
                        created.push(n.clone());
                    }
                }
            }
            out.push(created);
            let Some(k) = (0..combo.len()).find(|&k| combo[k] + 1 < runs[k].len()) else {
                break;
            };
            combo[k] += 1;
            for c in combo.iter_mut().take(k) {
                *c = 0;
            }
        }
        out
    }

    /// The creation orders other than its own a cell of `layout` can
    /// have, as layout indices (empty for every layout no reordered
    /// literal is stored in).
    pub fn foreign_orders(&self, layout: &[(PropKey, Type)]) -> Vec<Vec<u32>> {
        let names: Vec<PropKey> = layout.iter().map(|(n, _)| n.clone()).collect();
        let Some(orders) = self.objlit_orders.get(&names) else {
            return Vec::new();
        };
        orders
            .iter()
            .filter_map(|o| {
                o.iter()
                    .map(|n| names.iter().position(|m| m == n).map(|i| i as u32))
                    .collect()
            })
            .collect()
    }
}

/// The property names a spread of a cell created in `order`
/// contributes (§7.3.25: private elements and lone setters contribute
/// nothing, a getter its property name).
pub(crate) fn spread_contribution(order: &[PropKey], omit: &[PropKey]) -> Vec<PropKey> {
    let mut out = Vec::new();
    for sn in order {
        if crate::check_type_of_object_lit::private_slot(sn) {
            continue;
        }
        let name = match crate::check_type_of_object_lit::accessor_slot(sn) {
            Some(("__setter_", _)) => continue,
            Some((_, prop)) => PropKey::from(prop),
            None => sn.clone(),
        };
        if !omit.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The order a literal of only plain `key: value` members creates its
/// properties in (a repeated key keeps its first position, §10.1.9.2).
fn plain_objlit_order(entries: &[(PropKey, ExprId)]) -> Option<Vec<PropKey>> {
    let mut out: Vec<PropKey> = Vec::new();
    for (n, _) in entries {
        if crate::check_type_of_object_lit::spread_omit_set(n).is_some()
            || crate::check_type_of_object_lit::accessor_slot(n).is_some()
            || crate::check_type_of_object_lit::private_slot(n)
        {
            return None;
        }
        if !out.contains(n) {
            out.push(n.clone());
        }
    }
    Some(out)
}
