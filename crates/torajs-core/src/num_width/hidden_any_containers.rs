//! The containers of `hidden_any`.
//!
//! An array a value of another kind is stored into holds it as is
//! (§10.4.2.4 for an element store, §23.1.3.23 for `push`): `[d[1],
//! 5]`, `acc.push(v)` and `ys[0] = d[1]` with `d[1]` an element of an
//! escaped typed array put "s" into a `number[]`, which lowers with raw
//! number slots. Such an array escapes into the `any` world itself
//! (`hof_receivers::settle` adds it to the escape seeds), so it lowers
//! as an `Arr<Any>` and its reads become sources in turn. A `Map`
//! stored one through `set` (§24.1.3.9) escapes the same way: it leaves
//! the typed number lane and `get` answers what was stored.
//!
//! A field of an object literal set from such a value, or stored such a
//! value later (`obj.k = v`), holds it the same way (§10.1.9): the
//! check driver initializes the literal's field through `as any`, and a
//! read of the field carries the value on. The element reads a method answers
//! (`d.at(1)`, `d.pop()`) are sources as the index reads are.

use super::container::canon_key_frozen;
use super::hidden_any::{Graph, Node};
use super::{Analysis, Scope, SlotKeyData};
use crate::ast::{Expr, ExprId};
use crate::check::Type;

impl Graph<'_> {
    /// The container stores under expression `e`.
    pub(super) fn container_writes(&mut self, a: &Analysis<'_>, sc: &Scope, e: ExprId) {
        match a.ast.get_expr(e) {
            Expr::Assign { target, value } => match a.ast.get_expr(*target) {
                Expr::Index { obj, .. } => self.store_into(a, sc, *obj, *value),
                Expr::Member { .. } => {
                    if let Some(f) = a.container_key_lookup(*target, sc) {
                        let f = canon_key_frozen(&a.uf, &a.keys, f);
                        self.flow(a, &[Node::Read(f)], *value, sc);
                    }
                }
                _ => {}
            },
            Expr::Call { callee, args } => {
                let Expr::Member { obj, name } = a.ast.get_expr(*callee) else {
                    return;
                };
                let stored = match name.as_str() {
                    "push" | "unshift" => &args[..],
                    "splice" => args.get(2..).unwrap_or_default(),
                    "fill" => args.get(..1).unwrap_or_default(),
                    "set" if matches!(a.expr_types.get(obj), Some(Type::Map(..))) => {
                        args.get(1..2).unwrap_or_default()
                    }
                    _ => &[],
                };
                for &v in stored {
                    self.store_into(a, sc, *obj, v);
                }
            }
            Expr::Array(els) => {
                for &v in els {
                    self.store_into(a, sc, e, v);
                }
            }
            Expr::ObjectLit { fields } => {
                let Some(k) = a.container_key_lookup(e, sc) else {
                    return;
                };
                for (name, v) in fields {
                    let f = a.keys.field(k, name);
                    let n = Node::Read(canon_key_frozen(&a.uf, &a.keys, f));
                    self.flow(a, std::slice::from_ref(&n), *v, sc);
                    self.lit_fields.push((n, *v));
                }
            }
            _ => {}
        }
    }

    /// An edge from every source `value` carries into the container
    /// `obj` evaluates to, unless that container already escaped.
    fn store_into(&mut self, a: &Analysis<'_>, sc: &Scope, obj: ExprId, value: ExprId) {
        let Some(k) = a.container_key_lookup(obj, sc) else {
            return;
        };
        let k = canon_key_frozen(&a.uf, &a.keys, k);
        if !self.any_escaped.contains(&k) {
            self.flow(a, &[Node::Into(k)], value, sc);
        }
    }

    /// What a read through a container carries: a field of an object
    /// literal, or an element a method of an escaped array answers.
    pub(super) fn container_read(
        &mut self,
        a: &Analysis<'_>,
        eid: ExprId,
        sc: &Scope,
    ) -> Option<Node> {
        let k = a.container_key_lookup(eid, sc)?;
        match a.keys.data(k) {
            SlotKeyData::Field(..) if matches!(a.ast.get_expr(eid), Expr::Member { .. }) => {
                Some(Node::Read(canon_key_frozen(&a.uf, &a.keys, k)))
            }
            SlotKeyData::Elem(recv) if matches!(a.ast.get_expr(eid), Expr::Call { .. }) => {
                let recv = canon_key_frozen(&a.uf, &a.keys, recv);
                if !self.any_escaped.contains(&recv) {
                    return None;
                }
                let n = Node::Read(a.keys.elem(recv));
                self.seeds.push(n.clone());
                Some(n)
            }
            _ => None,
        }
    }
}
