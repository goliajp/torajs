//! Member calls on a builtin namespace receiver (`Promise.*`,
//! `Array.from`, `Object.defineProperty`, `Object.assign`,
//! `Reflect.set` / `Reflect.deleteProperty`): the receiver is no
//! container, so each wires its own edges. Also the value properties
//! of `Math` and `Number`, which no container lattice holds. Split from
//! `container_methods.rs` (file-size limit).

use super::magnitude::Mag;
use super::{Analysis, Scope, W};
use crate::ast::{Expr, ExprId, PropKey};

impl<'a> Analysis<'a> {
    /// A call on a builtin namespace receiver (`Promise.*`,
    /// `Array.from` / `<TypedArray>.from`, `Object.defineProperty(ies)`)
    /// — the receiver is no container, so each wires its own edges.
    /// `true` when the call was one of them.
    pub(super) fn namespace_call_effects(
        &mut self,
        eid: ExprId,
        obj: ExprId,
        name: &str,
        args: &[ExprId],
        scope: &Scope,
    ) -> bool {
        // ②.6b — Promise statics have an untrackable receiver (the
        // global namespace ident); their value-point wiring must fire
        // from here, the every-call-site hook (the result-key path
        // only fires when the result is consumed via a tracked flow —
        // `console.log(await Promise.resolve(2.5))` never is).
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Promise"
            && self.resolve(ns, scope).is_none()
        {
            self.promise_static_wiring(eid, name, args, scope);
            return true;
        }
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && (ns == "Array"
                || crate::ast::desugar_classes_builtin_heritage::is_typedarray_ctor(ns))
            && name == "from"
            && self.resolve(ns, scope).is_none()
        {
            self.array_from_wiring(args, scope);
            return true;
        }
        // W-ESC (RFC 20260721-object-descriptor-cluster 刀 5) — the
        // Object.defineProperty / defineProperties receiver escapes to
        // the any world: the define family's exotic semantics
        // (attribute shadows, accessor indexes, cross-kind values)
        // exist only on the NaN-box lane, and a typed receiver's
        // raw-slot loads could not observe them. The runtime typed arm
        // keeps its loud reject as the fallback for receiver shapes
        // this analysis can't see.
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Object"
            && self.resolve(ns, scope).is_none()
            && matches!(name, "defineProperty" | "defineProperties")
        {
            if let Some(a0) = args.first()
                && let Some(rk) = self.container_key_of(*a0, scope)
            {
                self.any_seeds.push(rk);
            }
            return true;
        }
        // §28.1.12 Reflect.set is the target's [[Set]] and §28.1.4
        // Reflect.deleteProperty its [[Delete]]: on an array they write
        // an element and leave a hole, so they seed the target's
        // element class as `t[k] = v` and `delete t[k]` do. Unseeded,
        // an integer-lane target refused the fraction or the hole with
        // a kind TypeError.
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Reflect"
            && self.resolve(ns, scope).is_none()
            && matches!(name, "set" | "deleteProperty")
        {
            if let Some(a0) = args.first()
                && let Some(rk) = self.container_key_of(*a0, scope)
            {
                self.mark_containerish(rk);
                let ek = self.keys.elem(rk);
                if name == "deleteProperty" {
                    self.add_container_constraint(ek, super::magnitude::f64_flow());
                } else if let Some(v) = args.get(2).copied() {
                    let w = self.flow(v, scope);
                    self.add_container_constraint(ek, w);
                    if let Some(vk) = self.container_key_of(v, scope) {
                        self.nested_unions.push((ek, vk));
                    }
                }
            }
            return true;
        }
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Object"
            && self.resolve(ns, scope).is_none()
            && name == "assign"
        {
            self.assign_wiring(args, scope);
            return true;
        }
        false
    }

    /// §20.1.2.1 `Object.assign(target, ...sources)` reads each own
    /// property of a source and [[Set]]s it on the target: every source
    /// field writes the target field of its name, so the target's
    /// field takes the source field's width as `t.k = s.k` gives it. A
    /// getter half answers what its body returns, which no field holds:
    /// it writes an f64.
    fn assign_wiring(&mut self, args: &[ExprId], scope: &Scope) {
        let Some(rk) = args.first().and_then(|t| self.container_key_of(*t, scope)) else {
            return;
        };
        self.mark_containerish(rk);
        for s in args.iter().skip(1) {
            let Some(crate::check::Type::Struct(fs)) = self.expr_types.get(s) else {
                continue;
            };
            let sk = self.container_key_of(*s, scope);
            for (k, _) in fs.iter() {
                let (key, getter) = match k
                    .as_wtf8()
                    .as_str()
                    .and_then(|n| n.strip_prefix("__getter_"))
                {
                    Some(p) => (PropKey::from(p), true),
                    None => (k.clone(), false),
                };
                let flow = match &sk {
                    Some(sk) if !getter => {
                        let fk = self.keys.field(*sk, &key);
                        let name = key.lossy().into_owned();
                        let mag = Mag::Max(vec![Mag::Slot(fk), Mag::Prop(name)]);
                        (W::Num(vec![(fk, false)]), mag)
                    }
                    _ => super::magnitude::f64_flow(),
                };
                self.add_container_constraint(self.keys.field(rk, &key), flow);
            }
        }
    }

    /// `Math.PI` / `Number.EPSILON` and their siblings (§21.3.1,
    /// §21.1.2): doubles — fractions, NaN, ±∞ — so the slot they land
    /// in has to be F64. As a receiver-free read it had no container
    /// key and answered NotNum, and an I64 slot then held the f64's
    /// bit pattern as an integer.
    ///
    /// A RegExp's `lastIndex` (§22.2.6.13) is the same kind of read:
    /// the receiver is no container either, the lowering always loads
    /// it as a double (`ssa_lower_member_regexp_props`), and the local
    /// holding the regex only lent it a `Field` key nothing ever
    /// constrains — so `xs.push(re.lastIndex)` met an I64 `xs`.
    pub(super) fn namespace_number_read(&self, eid: ExprId) -> bool {
        let Expr::Member { obj, name } = self.ast.get_expr(eid) else {
            return false;
        };
        matches!(self.ast.get_expr(*obj), Expr::Ident(ns) if ns == "Math" || ns == "Number")
            || (name == "lastIndex"
                && matches!(self.expr_types.get(obj), Some(crate::check::Type::RegExp)))
    }
}
