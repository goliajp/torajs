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
//! What an expression carries is in `hidden_any_sources`.
//!
//! Each declaration is its own node, so two same-named `let`s of one
//! function (sibling blocks, or an inner one shadowing an outer one)
//! are retyped apart; names resolve by block scope (`hidden_any_scope`).

use std::collections::{HashMap, HashSet, VecDeque};

use super::captures::bodies;
use super::{Analysis, Scope, SlotKey};
use crate::ast::ExprId;
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
    /// What a function returns.
    Ret(String),
    /// A function that may return a value the program spells `any`
    /// (`returns`); it reaches only other returns.
    RetAny(String),
    /// What a container stores (`hidden_any_containers`).
    Into(SlotKey),
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
    /// Where the values it holds come from (`returns`).
    pub(super) origin: Origin,
}

/// What a declaration says about the values it holds.
#[derive(Clone, Copy, Debug)]
pub(super) enum Origin {
    /// A `const` with no annotation: its initializer is all it holds.
    Fixed(ExprId),
    /// A `let` / `var`, a binding with an annotation (which a desugar
    /// pass may have written) or a loop binding: its initializer, or the
    /// element it reads, and every value stored to its name.
    Open(ExprId),
    /// A parameter, and whether its annotation leaves its kind open
    /// (absent, or `any`).
    Param(bool),
    /// A binding the walk does not trace (`using`, a catch binding, a
    /// loop binding with no element read).
    Unknown,
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
    /// [`Decl::origin`] by declaration as the previous pass found them:
    /// a capture names a declaration of a body walked later.
    pub(super) origins_in: Vec<Origin>,
    /// What the return walk looks up, built when a return first asks
    /// and shared by every pass.
    pub(super) lookups: &'s std::cell::OnceCell<returns::Lookups>,
    /// The kinds the returns of each function with no scalar return
    /// annotation hide behind `any` (`returns::kinds`).
    pub(super) ret_kinds: std::cell::RefCell<returns::RetKinds>,
    /// The block frames of the body being walked, outermost first:
    /// its parameters, then its own top-level bindings.
    pub(super) frames: Vec<HashMap<String, Node>>,
    /// Each object literal field value, with the field point it sets.
    pub(super) lit_fields: Vec<(Node, ExprId)>,
}

/// What the check driver changes for the next round: the initializers
/// of the declarations to retype `any`, the call arguments that may
/// carry a value of another kind than the checker reads them as, the
/// object literal field values whose field may be stored one, and the
/// functions that may return one. `filled` are the containers such a
/// value is stored into, which the width analysis itself turns into
/// escapes.
#[derive(Default)]
pub(crate) struct HiddenAny {
    pub(crate) inits: HashSet<ExprId>,
    pub(crate) args: HashSet<ExprId>,
    pub(crate) fields: HashSet<ExprId>,
    pub(crate) rets: HashSet<String>,
    pub(super) filled: Vec<SlotKey>,
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
    let mut origins = Vec::new();
    let lookups = std::cell::OnceCell::new();
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
            origins_in: origins,
            lookups: &lookups,
            frames: Vec::new(),
            lit_fields: Vec::new(),
            ret_kinds: Default::default(),
        };
        for b in &bodies {
            g.walk_body(a, b);
        }
        if g.caps_out == g.caps_in {
            break g;
        }
        caps = g.caps_out;
        origins = g.decls.iter().map(|d| d.origin).collect();
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
        let kind_seeds = self.kind_seeds();
        self.seeds.extend(kind_seeds);
        let mut tainted: HashSet<Node> = HashSet::new();
        let mut work: VecDeque<Node> = self.seeds.into_iter().collect();
        while let Some(n) = work.pop_front() {
            if tainted.insert(n.clone()) {
                work.extend(self.edges.get(&n).into_iter().flatten().cloned());
            }
        }
        let stats = std::env::var_os("TORAJS_NUM_WIDTH_STATS").is_some();
        let mut out = HiddenAny::default();
        for (n, v) in &self.lit_fields {
            if tainted.contains(n) {
                out.fields.insert(*v);
            }
        }
        for n in &tainted {
            match n {
                Node::Arg(e) => drop(out.args.insert(*e)),
                Node::Ret(f) | Node::RetAny(f) => drop(out.rets.insert(f.clone())),
                Node::Into(k) => out.filled.push(k.clone()),
                _ => {}
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
}

mod returns;

#[cfg(test)]
#[path = "hidden_any_tests.rs"]
mod tests;
