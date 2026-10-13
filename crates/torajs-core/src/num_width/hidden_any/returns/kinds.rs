//! The kinds of the values a function with no scalar return annotation
//! hands back behind `any`.
//!
//! `function g() { return "s" as any }` converts nothing: it declares no
//! return to convert to. A function declared `number` that returns
//! `g()` converts the string, so whether its return seeds depends on
//! what `g` hides, not on `g` alone. The walk notes the kinds each such
//! function's returns hide; they flow on through the functions with no
//! scalar annotation that hand a call's answer on, and seed each
//! function declared a scalar that returns a call answering a kind
//! other than the one it declares.

use std::collections::{HashMap, HashSet};

use super::Read;
use crate::check::Type;
use crate::num_width::hidden_any::{Graph, Node};

/// The kind of a value, as a return converts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::num_width) enum Kind {
    Number,
    String,
    Boolean,
    /// Anything else: an object, `undefined`, `null`, a bigint.
    Other,
}

impl Kind {
    pub(super) fn of(t: &Type) -> Kind {
        match t {
            Type::Number => Kind::Number,
            Type::String => Kind::String,
            Type::Boolean => Kind::Boolean,
            _ => Kind::Other,
        }
    }
}

/// What the walk notes for [`Graph::kind_seeds`].
#[derive(Default)]
pub(in crate::num_width) struct RetKinds {
    /// Function → the kinds its returns hide behind `any`.
    kinds: HashMap<String, HashSet<Kind>>,
    /// (callee, caller): a return of the caller hands on what a call of
    /// the callee answers, not a member of it.
    hands: HashSet<(String, String)>,
    /// (callee, caller): a return of the caller reads a member of what a
    /// call of the callee answers, or takes it as an operand.
    members: HashSet<(String, String)>,
}

impl RetKinds {
    pub(super) fn note(&mut self, f: &str, k: Kind) {
        self.kinds.entry(f.to_string()).or_default().insert(k);
    }
}

impl Graph<'_> {
    /// Whether a value of kind `k` read by a return of `f` (`read`)
    /// differs from the scalar it `declared`, which the return would
    /// convert it to. A function with no scalar annotation converts
    /// nothing; the kind is noted for the functions that hand its answer
    /// on.
    pub(super) fn differs(
        &self,
        (f, read): (&str, Read),
        declared: Option<&Type>,
        k: Kind,
    ) -> bool {
        if read == Read::Value && declared.is_none() {
            self.ret_kinds.borrow_mut().note(f, k);
        }
        declared.is_some_and(|d| Kind::of(d) != k)
    }

    /// Note that a return of `f` hands on what the functions whose
    /// returns `nodes` names answer (`read` is `Read::Value`), or reads
    /// a member of it or takes it as an operand.
    pub(super) fn hands_on(&self, (f, read): (&str, Read), nodes: &[Node]) {
        let mut rk = self.ret_kinds.borrow_mut();
        let to = match read {
            Read::Value => &mut rk.hands,
            Read::Members => &mut rk.members,
        };
        for n in nodes {
            if let Node::RetAny(g) = n {
                to.insert((g.clone(), f.to_string()));
            }
        }
    }

    /// The returns of the functions declared a scalar that hand on the
    /// answer of a function hiding a value of another kind, and of the
    /// functions that read a member of such an answer or take it as an
    /// operand: what the member or the operation answers then depends on
    /// a kind the checker did not see (`g() + 1` is `"s1"` when `g`
    /// hides `"s"`).
    pub(in crate::num_width) fn kind_seeds(&self) -> Vec<Node> {
        let (Some(l), rk) = (self.lookups.get(), self.ret_kinds.borrow()) else {
            return Vec::new();
        };
        let mut kinds = rk.kinds.clone();
        loop {
            let mut grew = false;
            for (g, f) in &rk.hands {
                let Some(ks) = kinds.get(g).cloned() else {
                    continue;
                };
                if l.declared(f).is_none() {
                    let to = kinds.entry(f.clone()).or_default();
                    let before = to.len();
                    to.extend(ks);
                    grew |= to.len() > before;
                }
            }
            if !grew {
                break;
            }
        }
        let mut out = Vec::new();
        for (g, f) in &rk.hands {
            if let (Some(ks), Some(t)) = (kinds.get(g), l.declared(f))
                && ks.iter().any(|k| *k != Kind::of(&t))
            {
                out.push(Node::RetAny(f.clone()));
            }
        }
        for (g, f) in &rk.members {
            if kinds.get(g).is_some_and(|ks| !ks.is_empty()) {
                out.push(Node::RetAny(f.clone()));
            }
        }
        out
    }
}
