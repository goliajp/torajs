//! The bindings that may store a value the checker types by a
//! declaration while the lowering holds it as `any`.
//!
//! An escaped typed array lowers as an `Arr<Any>` (`escape.rs`), yet
//! the checker still reads `d[i]` by the declared element type; the
//! element parameter of a callback over such an array, or over an
//! `any[]`, takes the elements through the callback's all-`any` twin;
//! and an `any[]` read answers whatever the array holds. A binding
//! typed `number` that stores such a value converted it on the way in,
//! while §13.15.2 stores it as is: `t = t + d[1]` holds "0s" when the
//! element is "s". The check driver (`check_entry`) retypes these
//! bindings `any`; every other binding keeps its type.
//!
//! The value flows through copies, `+` (it may concatenate), `&&`,
//! `||`, `??`, the conditional, the comma and assignment expressions.
//! Every other operator answers a number or a boolean whatever its
//! operands hold, so the flow stops there.
//!
//! Each declaration is its own node, so two same-named `let`s of one
//! function (sibling blocks, or an inner one shadowing an outer one)
//! are retyped apart; names resolve by block scope (`hidden_any_scope`).

use std::collections::{HashMap, HashSet, VecDeque};

use super::captures::bodies;
use super::container::canon_key_frozen;
use super::hidden_any_calls::answers_any;
use super::{Analysis, Scope, SlotKey};
use crate::ast::{BinOp, Expr, ExprId};
use crate::check::Type;
use crate::check_monomorph_any_widen::struct_escape::twin_original;

/// One point a value can sit in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Node {
    /// One declaration, by its index in `Graph::decls`.
    Decl(usize),
    /// A read through a slot key the scope walk could not pin to one
    /// declaration: a parameter, an element point, or every
    /// declaration of that key.
    Read(SlotKey),
    /// A write through such a key; it reaches every declaration of it.
    Write(SlotKey),
    /// An element read of an `any[]`, or a call answering through an
    /// any-widened clone typed `any` (`hidden_any_calls`).
    AnyVal,
    /// A direct call's argument.
    Arg(ExprId),
}

/// What a name resolves to: one node, or the slot keys of every
/// binding it may be.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Ref {
    At(Node),
    Keyed(Vec<SlotKey>),
}

impl Ref {
    pub(super) fn reads(&self) -> Vec<Node> {
        match self {
            Ref::At(n) => vec![n.clone()],
            Ref::Keyed(ks) => ks.iter().map(|k| Node::Read(k.clone())).collect(),
        }
    }

    pub(super) fn writes(&self) -> Vec<Node> {
        match self {
            Ref::At(n) => vec![n.clone()],
            Ref::Keyed(ks) => ks.iter().map(|k| Node::Write(k.clone())).collect(),
        }
    }
}

pub(super) struct Decl {
    pub(super) key: SlotKey,
    /// The initializer of a retype candidate ([`candidate`]).
    pub(super) init: Option<ExprId>,
}

/// Lifted fn and captured name → the binding its creator handed it.
/// `None` marks two creators that disagree.
pub(super) type CapMap = HashMap<(String, String), Option<Ref>>;

pub(super) struct Graph<'s> {
    pub(super) any_escaped: &'s HashSet<SlotKey>,
    pub(super) edges: HashMap<Node, Vec<Node>>,
    pub(super) seeds: Vec<Node>,
    pub(super) decls: Vec<Decl>,
    /// The captures resolved by the previous pass, and by this one.
    pub(super) caps_in: CapMap,
    pub(super) caps_out: CapMap,
    /// The block frames of the body being walked, outermost first:
    /// its parameters, then its own top-level bindings.
    pub(super) frames: Vec<HashMap<String, Node>>,
}

/// What the check driver changes for the next round: the initializers
/// of the declarations to retype `any`, and the call arguments that may
/// carry a value of another kind than the checker reads them as.
#[derive(Default)]
pub(crate) struct HiddenAny {
    pub(crate) inits: HashSet<ExprId>,
    pub(crate) args: HashSet<ExprId>,
}

/// The initializers of the declarations to retype `any`, and the call
/// arguments that may carry such a value. `elem_params`
/// are the parameters handed values of any kind: callback parameters
/// handed `any` elements (`hof_receivers::settle`) and the widened
/// parameters of the any-widened clones (`hidden_any_seeds`).
pub(super) fn collect(
    a: &Analysis<'_>,
    any_escaped: &HashSet<SlotKey>,
    elem_params: &HashSet<(String, String)>,
) -> HiddenAny {
    let bodies = bodies(a.ast);
    // one pass per closure nesting level settles the captures
    let mut caps = CapMap::new();
    let g = loop {
        let mut g = Graph {
            any_escaped,
            edges: HashMap::new(),
            seeds: (elem_params.iter())
                .map(|(f, p)| Node::Read(SlotKey::Param(f.clone(), p.clone())))
                .collect(),
            decls: Vec::new(),
            caps_in: caps,
            caps_out: CapMap::new(),
            frames: Vec::new(),
        };
        for b in &bodies {
            g.walk_body(a, b);
        }
        if g.caps_out == g.caps_in {
            break g;
        }
        caps = g.caps_out;
    };
    g.retyped()
}

/// A `let` / `const` declared `number`, `string`, `boolean` or with
/// no annotation, whose initializer the checker types as one of those
/// three, is a retype candidate.
pub(super) fn candidate(a: &Analysis<'_>, type_ann: Option<&str>, init: ExprId) -> bool {
    matches!(type_ann, None | Some("number" | "string" | "boolean"))
        && matches!(
            a.expr_types.get(&init),
            Some(Type::Number | Type::String | Type::Boolean)
        )
}

impl Graph<'_> {
    /// Edges from every source `value` carries into each of `dsts`.
    pub(super) fn flow(&mut self, a: &Analysis<'_>, dsts: &[Node], value: ExprId, scope: &Scope) {
        if dsts.is_empty() {
            return;
        }
        let mut srcs = Vec::new();
        self.sources(a, value, scope, &mut srcs);
        for s in srcs {
            self.edges
                .entry(s)
                .or_default()
                .extend(dsts.iter().cloned());
        }
    }

    /// The tainted candidates' initializers and arguments.
    fn retyped(mut self) -> HiddenAny {
        for (i, d) in self.decls.iter().enumerate() {
            let (r, w) = (Node::Read(d.key.clone()), Node::Write(d.key.clone()));
            self.edges.entry(Node::Decl(i)).or_default().push(r);
            self.edges.entry(w).or_default().push(Node::Decl(i));
        }
        let keyed: Vec<SlotKey> = (self.edges.values().flatten())
            .filter_map(|n| match n {
                Node::Write(k) => Some(k.clone()),
                _ => None,
            })
            .collect();
        for k in keyed {
            let r = Node::Read(k.clone());
            self.edges.entry(Node::Write(k)).or_default().push(r);
        }
        let mut tainted: HashSet<Node> = HashSet::new();
        let mut work: VecDeque<Node> = self.seeds.into_iter().collect();
        while let Some(n) = work.pop_front() {
            if tainted.insert(n.clone()) {
                work.extend(self.edges.get(&n).into_iter().flatten().cloned());
            }
        }
        let stats = std::env::var_os("TORAJS_NUM_WIDTH_STATS").is_some();
        let mut out = HiddenAny::default();
        for n in &tainted {
            if let Node::Arg(e) = n {
                out.args.insert(*e);
            }
        }
        for (i, d) in self.decls.iter().enumerate() {
            if let Some(init) = d.init
                && tainted.contains(&Node::Decl(i))
            {
                // TORAJS_NUM_WIDTH_STATS=1 — one line per retyped declaration
                if stats {
                    eprintln!("[num_width] hidden-any decl: {:?} init {}", d.key, init.0);
                }
                out.inits.insert(init);
            }
        }
        out
    }

    /// What `n` names at the current point of the walk.
    pub(super) fn resolve(&self, a: &Analysis<'_>, n: &str, sc: &Scope) -> Ref {
        if let Some(node) = self.frames.iter().rev().find_map(|f| f.get(n)) {
            return Ref::At(node.clone());
        }
        // a closure's all-`any` twin has no construction site of its
        // own; it reads the bindings the original's site names
        let built = twin_original(sc.fn_name).unwrap_or(sc.fn_name);
        if let Some(Some(r)) = self.caps_in.get(&(built.to_string(), n.to_string())) {
            return r.clone();
        }
        if let Some(k) = a.resolve(n, sc) {
            return Ref::Keyed(vec![k]);
        }
        // a lifted body may still write an outer binding of that
        // spelling (`walk::assign_to_name`)
        if sc.fn_name.starts_with("__") {
            return Ref::Keyed(a.by_name.get(n).cloned().unwrap_or_default());
        }
        Ref::Keyed(Vec::new())
    }

    fn sources(&mut self, a: &Analysis<'_>, eid: ExprId, scope: &Scope, out: &mut Vec<Node>) {
        match a.ast.get_expr(eid) {
            Expr::Ident(n) => out.extend(self.resolve(a, n, scope).reads()),
            Expr::Call { .. } if answers_any(a, eid) => {
                self.seeds.push(Node::AnyVal);
                out.push(Node::AnyVal);
            }
            Expr::Index { obj, .. } | Expr::OptIndex { obj, .. } => {
                if let Some(n) = self.elem_source(a, *obj, scope) {
                    self.seeds.push(n.clone());
                    out.push(n);
                }
            }
            // a `+` the checker types `string` has a string operand and
            // answers a string whatever the other one holds
            Expr::BinOp { op: BinOp::Add, .. } if a.expr_types.get(&eid) == Some(&Type::String) => {
            }
            Expr::BinOp {
                op: BinOp::Add | BinOp::LAnd | BinOp::LOr,
                left,
                right,
            }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.sources(a, *left, scope, out);
                self.sources(a, *right, scope, out);
            }
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                self.sources(a, *then_branch, scope, out);
                self.sources(a, *else_branch, scope, out);
            }
            Expr::Sequence { right: e, .. }
            | Expr::As { expr: e, .. }
            | Expr::Assign { value: e, .. } => self.sources(a, *e, scope, out),
            _ => {}
        }
    }

    /// The source a read of an element of `obj` is: any element of an
    /// `any[]`, or the element point of a typed array whose alias class
    /// escaped, which answers whatever the `any` side stored.
    fn elem_source(&self, a: &Analysis<'_>, obj: ExprId, scope: &Scope) -> Option<Node> {
        let Some(Type::Array(el)) = a.expr_types.get(&obj) else {
            return None;
        };
        if **el == Type::Any {
            return Some(Node::AnyVal);
        }
        let k = canon_key_frozen(&a.uf, &a.container_key_lookup(obj, scope)?);
        self.any_escaped
            .contains(&k)
            .then(|| Node::Read(SlotKey::Elem(Box::new(k))))
    }
}

#[cfg(test)]
#[path = "hidden_any_tests.rs"]
mod tests;
