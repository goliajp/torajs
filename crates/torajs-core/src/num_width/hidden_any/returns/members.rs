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
        let classes: Option<Vec<&str>> = match recv {
            Some(Type::ClassRef(c)) => Some(vec![c.as_str()]),
            Some(Type::Union(ts)) => (ts.iter())
                .map(|t| match t {
                    Type::ClassRef(c) => Some(c.as_str()),
                    _ => None,
                })
                .collect(),
            _ => None,
        };
        let Some(classes) = classes else {
            return true;
        };
        let l = self.lookups(a);
        let mut hit = false;
        for c in classes {
            let Some(body) = method_body(a, c, name) else {
                return true;
            };
            match l.written_ret(&body).filter(|_| !generic(a, c)) {
                Some(r) => hit |= lookups::open_ann(r) || lookups::scalar(r).as_ref() != declared,
                None => from.push(Node::RetAny(body)),
            }
        }
        hit
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
                return *ann;
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
fn method_body(a: &Analysis<'_>, c: &str, m: &str) -> Option<String> {
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
