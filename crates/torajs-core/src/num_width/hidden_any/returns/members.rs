//! What the members of a class instance the return walk reads hand
//! back: a method by its declaration, a field or getter by its
//! annotation.

use super::lookups;
use crate::check::Type;
use crate::num_width::Analysis;
use crate::num_width::hidden_any::{Graph, Node};

impl Graph<'_> {
    /// Whether a call of method `name` on a receiver of type `recv`
    /// answers a value the return converts: one of a kind left open, or
    /// of a scalar other than `declared`. A method each receiver class
    /// declares is read by its annotation, or through its returns when
    /// it wrote none or its class is generic; a method of anything else
    /// answers what its library declares, which the checker typed `any`.
    pub(super) fn method_spelled_any(
        &self,
        a: &Analysis<'_>,
        (recv, name): (Option<&Type>, &str),
        declared: Option<&Type>,
        from: &mut Vec<Node>,
    ) -> bool {
        let mut classes = Vec::new();
        if !recv.is_some_and(|t| receiver_classes(t, &mut classes)) {
            return true;
        }
        let l = self.lookups(a);
        let mut hit = false;
        let mut ran = false;
        for c in classes {
            // an arm with no such method cannot run the call (a union
            // narrowed by `in` or a tag); one that holds it in a field or
            // an accessor answers what that holds
            let Some(body) = method_body(a, c, name) else {
                if self.member_in_chain(a, (c, name)) {
                    return true;
                }
                continue;
            };
            ran = true;
            match l.written_ret(&body).filter(|_| !generic(a, c)) {
                Some(r) => hit |= lookups::open_ann(r) || lookups::scalar(r).as_ref() != declared,
                None => from.push(Node::RetAny(body)),
            }
        }
        // a method no arm declares is one the walk does not see
        hit || !ran
    }

    /// Whether member `f` of a value of type `t` is of a kind left open:
    /// a field or getter of a struct or class that spells `any`, on any
    /// arm of a union or behind `?.`. A member of a builtin value is
    /// typed by its library.
    pub(super) fn field_spelled_any(
        &self,
        a: &Analysis<'_>,
        (t, f): (&Type, &str),
        from: &mut Vec<Node>,
    ) -> bool {
        match t {
            Type::Any => true,
            Type::Struct(fs) => {
                let getter = format!("__getter_{f}");
                fs.iter().any(|(k, t)| match t {
                    Type::Function(_, r) if k == getter.as_str() => **r == Type::Any,
                    _ => k == f && *t == Type::Any,
                })
            }
            Type::ClassRef(c) => self.member_spelled_any(a, (c, f), from),
            Type::Nullable(t) => self.field_spelled_any(a, (t, f), from),
            Type::Union(ts) => {
                (ts.iter()).fold(false, |h, t| self.field_spelled_any(a, (t, f), from) | h)
            }
            _ => false,
        }
    }

    /// Does class `c`, or a class it extends, declare a field or an
    /// accessor `f`?
    fn member_in_chain(&self, a: &Analysis<'_>, (c, f): (&str, &str)) -> bool {
        let fields = &self.lookups(a).fields;
        let mut cur = Some(c.split('<').next().unwrap_or(c));
        while let Some(k) = cur {
            if fields.contains_key(&(k.to_string(), f.to_string())) {
                return true;
            }
            cur = a.ast.class_parents.get(k).and_then(|p| p.as_deref());
        }
        crate::ast::accessor_lookup::accessor_getter_in_chain(a.ast, c, f).is_some()
    }

    /// Does class `c`, or the class it extends, declare member `f` of a
    /// kind left open: a field with an annotation that spells `any`, or
    /// a getter declared to return `any`? A method is typed by its own
    /// declaration. A getter of a generic class goes to `from` instead,
    /// as one with no annotation does: its annotation may spell a type
    /// parameter the class desugar erased.
    pub(super) fn member_spelled_any(
        &self,
        a: &Analysis<'_>,
        (c, f): (&str, &str),
        from: &mut Vec<Node>,
    ) -> bool {
        let fields = &self.lookups(a).fields;
        let mut cur = Some(c.split('<').next().unwrap_or(c));
        while let Some(k) = cur {
            if let Some(ann) = fields.get(&(k.to_string(), f.to_string())) {
                return lookups::open_ann(ann);
            }
            cur = a.ast.class_parents.get(k).and_then(|p| p.as_deref());
        }
        let Some(g) = crate::ast::accessor_lookup::accessor_getter_in_chain(a.ast, c, f) else {
            return false;
        };
        if generic(a, c) {
            from.push(Node::RetAny(g.fn_name.clone()));
            return false;
        }
        self.lookups(a).returns_open(&g.fn_name)
    }
}

/// Push the classes a method call on a receiver of type `t` may run
/// the method of; false when an arm of `t` is no class instance. `?.`
/// skips a `null` or `undefined` receiver.
fn receiver_classes<'t>(t: &'t Type, out: &mut Vec<&'t str>) -> bool {
    match t {
        Type::ClassRef(c) => {
            out.push(c.as_str());
            true
        }
        Type::Nullable(t) => receiver_classes(t, out),
        Type::Union(ts) => (ts.iter())
            .all(|t| matches!(t, Type::Null | Type::Undefined) || receiver_classes(t, out)),
        _ => false,
    }
}

/// Is class `c` generic? Its members' annotations may spell a type
/// parameter, which the class desugar erases to `any`.
fn generic(a: &Analysis<'_>, c: &str) -> bool {
    let base = c.split('<').next().unwrap_or(c);
    a.ast
        .class_type_params
        .get(base)
        .is_some_and(|ps| !ps.is_empty())
}

/// The body of method `m` a receiver of class `c` runs: its own, or the
/// nearest ancestor's.
pub(super) fn method_body(a: &Analysis<'_>, c: &str, m: &str) -> Option<String> {
    let mut cur = Some(c.split('<').next().unwrap_or(c));
    while let Some(k) = cur {
        let body = format!("__cm_{k}__{m}");
        if a.fn_params.contains_key(&body) {
            return Some(body);
        }
        cur = a.ast.class_parents.get(k).and_then(|p| p.as_deref());
    }
    None
}
