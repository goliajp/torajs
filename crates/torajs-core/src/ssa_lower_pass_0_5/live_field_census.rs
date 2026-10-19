//! Whole-program census: which class fields hold a live cell at every
//! read.
//!
//! The factory seeds a pointer-shaped field with the immortal
//! `undefined` cell and the constructor overwrites it, so a read of
//! such a field tests for the cell: in general the write may not have
//! run yet (a read inside the constructor ahead of it), or a later
//! write may have put the cell back (a value typed `any`, an element
//! read past the end, a function that ran off its end). A field is
//! live when every one of those roads is closed:
//!
//! - its declared type is non-nullable and pointer-shaped;
//! - the constructor writes it in the leading run of statements the
//!   instance has not escaped from (`ast::fresh_this`), from a value
//!   that is live by construction;
//! - every other write to a member of that name, anywhere in the
//!   program, stores a value live by construction, and no computed key
//!   store, `delete`, dynamic `new` or reflection surface can reach a
//!   field; a property of an object literal counts as such a write,
//!   since the checker lets a literal of the class's shape into a slot
//!   typed as the class. The one literal left out is the class's own
//!   factory seed for a field the constructor prefix writes, when the
//!   factory passes the seed straight into the constructor: the prefix
//!   overwrites that value before anything can read it;
//! - nothing but an instance of the class reaches a slot typed as it:
//!   the class (or an ancestor) has no constructor answering another
//!   object, and no `as` cast names it over a value typed as something
//!   other than the class, a subclass or `any` (a value typed `any`
//!   enters a class slot through a checked cast that refuses a foreign
//!   object);
//! - no foreign receiver can enter a method twin.
//!
//! Live by construction: `new`, an array or object literal, `this`
//! inside a constructor body (a method's receiver comes from its
//! caller), a closure, a `const` every declaration of which is
//! initialised with one, a call to a declared function all of whose
//! returns are one, which cannot run off its end and whose name no
//! binding shadows, a read of another live field, or a constructor
//! parameter that every `new` and `super(...)` fills with one. Fields
//! and functions are judged together to a greatest fixed point: a
//! cycle (a function returning its own call, a field copied from
//! itself) is optimistic, which is sound because a non-live value can
//! only enter through a leaf shape, and that leaf fails on its own.
//!
//! The same fixed point answers reads of bindings and call results
//! (`bindings`, `scope`), which the lowering guards unless proven, and
//! the parameters a caller tests before the call (`entry`). A builtin
//! answering a fresh object, and a string a literal, a concatenation or
//! a string builtin answers, are live by construction too (`fresh`).
//!
//! The census is syntactic and over-approximates every road, like
//! `arr_head_census`; the cost of a false negative is the guard the
//! program paid before.

mod bindings;
mod collect;
mod disc;
mod entry;
mod fresh;
mod judge;
mod scope;
pub(super) mod walk;

use std::collections::HashMap as StdMap;

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, ExprId, Param, Stmt};
use crate::check::Type;
use crate::num_width::WidthTable;
use crate::ssa::id_hash::{HashMap, HashSet};

use super::arr_head_census::ExprMask;
use walk::{ctor_class, ctor_fed_seed, ctor_prefix, nested_stmts, user_params};

/// `(class, field)` pairs whose reads never see the undefined cell.
pub(crate) type LiveFields = HashSet<(String, String)>;

/// What the census proved live.
#[derive(Default)]
pub(crate) struct Live {
    pub(crate) fields: LiveFields,
    /// Reads of a binding and calls whose value the census did not
    /// prove live (`bindings`).
    unproven: ExprMask,
    /// Member reads a discriminant test proved live (`disc`).
    pub(crate) members: ExprMask,
    /// The arguments each direct call tests before the call (`entry`).
    entry: Entry,
}

/// Argument indices, by direct call.
type Entry = HashMap<ExprId, Vec<usize>>;

const THIS: &str = "__this";
/// Member / identifier / string names that reach a field or an
/// instance by other means than a typed read or write.
const ROADS: &[&str] = &[
    "Reflect",
    "eval",
    "Function",
    "Proxy",
    "assign",
    "defineProperty",
    "defineProperties",
    "create",
    "setPrototypeOf",
    "__proto__",
];

/// `retargets` names the specialization a generic call enters, which
/// is the function whose parameters its arguments fill.
pub(crate) fn program_live(
    ast: &Ast,
    expr_types: &StdMap<ExprId, Type>,
    (widths, retargets): (&WidthTable, &StdMap<ExprId, String>),
    dead_twins: &ExprMask,
) -> Live {
    let twins_open = !ast.cmany_twins.is_empty() && dead_twins.is_empty();
    let census = Census::collect(
        ast,
        expr_types,
        (widths, retargets),
        (dead_twins, twins_open),
    );
    census.solve(!twins_open && !census.road)
}

struct Ctor<'a> {
    /// The constructor function's own name.
    name: &'a str,
    class: &'a str,
    params: &'a [Param],
    /// The parameters a `new` fills (`walk::user_params`).
    user: Vec<&'a Param>,
    body: &'a [Stmt],
}

/// One prefix write `this.f = v` of a constructor, with the ctor
/// parameter it copies when `v` is a bare parameter.
struct PrefixWrite<'a> {
    class: &'a str,
    field: &'a str,
    value: ExprId,
    param: Option<usize>,
}

/// Each expression's checked type, by arena slot.
fn type_table<'a>(ast: &Ast, expr_types: &'a StdMap<ExprId, Type>) -> Vec<Option<&'a Type>> {
    let mut types = vec![None; ast.exprs.len()];
    for (e, t) in expr_types {
        if let Some(slot) = types.get_mut(e.0 as usize) {
            *slot = Some(t);
        }
    }
    types
}

struct Census<'a> {
    ast: &'a Ast,
    /// Each expression's checked type, by arena slot.
    types: Vec<Option<&'a Type>>,
    widths: &'a WidthTable,
    /// The specialization each retargeted generic call enters.
    retargets: &'a StdMap<ExprId, String>,
    /// Some road no typed census closes was found: nothing is live.
    road: bool,
    ctors: Vec<Ctor<'a>>,
    fns: HashMap<&'a str, (&'a [Param], &'a [Stmt])>,
    /// Every `let` / `const` initialiser, by name.
    let_inits: HashMap<&'a str, Vec<ExprId>>,
    /// Names bound other than by `let` / `const` or a judged parameter
    /// (other params, loop variables, catch params): a read of one
    /// cannot be judged.
    bound: HashSet<&'a str>,
    /// The constructor parameters a name is bound by, as `(class,
    /// user parameter index)`: a read of one is judged by the class's
    /// construction sites.
    ctor_params: HashMap<&'a str, Vec<(&'a str, usize)>>,
    /// The same for parameters of declared functions judged by their
    /// call sites (`bindings::judged_fn`), as `(function, index)`.
    fn_params: HashMap<&'a str, Vec<(&'a str, usize)>>,
    calls: bindings::Calls<'a>,
    scopes: scope::Scopes<'a>,
    /// Names assigned to anywhere, or declared more than once as a
    /// function.
    assigned: HashSet<&'a str>,
    /// The values `n = v` stores, by name.
    assign_values: HashMap<&'a str, Vec<ExprId>>,
    /// Every `x.f = v` outside the constructor prefixes, by field.
    member_writes: HashMap<&'a str, Vec<ExprId>>,
    /// Field names some `delete x.f` names.
    deleted: HashSet<&'a str>,
    /// Field names written through a receiver not typed as a class.
    foreign: HashSet<&'a str>,
    /// Classes a value other than an instance may be typed as.
    foreign_classes: HashSet<&'a str>,
    /// The expressions of the constructor bodies, where `this` is the
    /// instance under construction.
    ctor_exprs: ExprMask,
    prefix: Vec<PrefixWrite<'a>>,
    /// `new C(..)` and `super(..)` argument lists, by class.
    construct_args: HashMap<&'a str, Vec<&'a [ExprId]>>,
    /// Factory seed literals and the fields of each that the
    /// constructor prefix overwrites unread.
    unread_seeds: HashMap<ExprId, HashSet<&'a str>>,
}

impl<'a> Census<'a> {
    fn collect(
        ast: &'a Ast,
        expr_types: &'a StdMap<ExprId, Type>,
        (widths, retargets): (&'a WidthTable, &'a StdMap<ExprId, String>),
        (dead, twins_open): (&ExprMask, bool),
    ) -> Self {
        let mut c = Census {
            ast,
            types: type_table(ast, expr_types),
            widths,
            retargets,
            road: false,
            ctors: Vec::new(),
            fns: HashMap::default(),
            let_inits: HashMap::default(),
            bound: HashSet::default(),
            ctor_params: HashMap::default(),
            fn_params: HashMap::default(),
            calls: bindings::Calls::default(),
            scopes: scope::Scopes::collect(ast),
            assigned: HashSet::default(),
            assign_values: HashMap::default(),
            member_writes: HashMap::default(),
            deleted: HashSet::default(),
            foreign: HashSet::default(),
            foreign_classes: ast
                .ctor_return_override
                .iter()
                .map(String::as_str)
                .collect(),
            ctor_exprs: ExprMask::new(ast),
            prefix: Vec::new(),
            construct_args: HashMap::default(),
            unread_seeds: HashMap::default(),
        };
        for s in &ast.stmts {
            c.collect_stmt(s);
        }
        c.calls = bindings::Calls::collect(ast, dead, (retargets, &c.fns));
        c.mark_ctor_exprs();
        c.collect_exprs(dead, twins_open);
        c
    }

    /// Every live expression but the constructor prefix writes, which
    /// are recorded apart (only where no twin is open, since a field is
    /// live only there).
    fn collect_exprs(&mut self, dead: &ExprMask, twins_open: bool) {
        let mut prefix_writes = HashSet::default();
        if !twins_open {
            prefix_writes = self.prefix_assigns();
            self.unread_seeds = self.unread_seeds();
        }
        for (i, e) in self.ast.exprs.iter().enumerate() {
            let id = ExprId(i as u32);
            if dead.contains(id) || prefix_writes.contains(&id) {
                continue;
            }
            self.collect_expr(id, e);
        }
    }

    /// Marks the expressions of the constructor bodies of classes
    /// only instances reach.
    fn mark_ctor_exprs(&mut self) {
        for ctor in &self.ctors {
            if self.foreign_classes.contains(ctor.class) {
                continue;
            }
            for s in ctor.body {
                visit_stmt_exprs(s, &mut |e| add_tree(self.ast, e, &mut self.ctor_exprs));
            }
        }
    }

    fn ty(&self, e: ExprId) -> Option<&'a Type> {
        self.types.get(e.0 as usize).copied().flatten()
    }

    /// A name some binding other than a function declaration may
    /// stand for.
    fn shadowed(&self, name: &str) -> bool {
        self.bound.contains(name)
            || self.let_inits.contains_key(name)
            || self.ctor_params.contains_key(name)
            || self.scopes.param_names.contains(name)
    }

    /// Declarations: functions, constructors, bindings.
    fn collect_stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                if let Some(class) = ctor_class(name) {
                    let user = user_params(params);
                    for (i, p) in user.iter().enumerate() {
                        let at = self.ctor_params.entry(&p.name).or_default();
                        at.push((class, i));
                    }
                    self.ctors.push(Ctor {
                        name,
                        class,
                        params,
                        user,
                        body,
                    });
                } else {
                    // the other parameters are scoped to their function
                    // (`scope::Scopes`)
                    if bindings::judged_fn(name, params) {
                        for (i, p) in params.iter().enumerate() {
                            let at = self.fn_params.entry(&p.name).or_default();
                            at.push((name, i));
                        }
                    }
                    if self.fns.insert(name, (params, body)).is_some() {
                        self.assigned.insert(name);
                    }
                }
                for st in body {
                    self.collect_stmt(st);
                }
            }
            Stmt::LetDecl { name, init, .. } => {
                self.let_inits.entry(name).or_default().push(*init);
            }
            Stmt::UsingDecl { name: n, .. }
            | Stmt::YieldInto { var: n, .. }
            | Stmt::ForOf { var_name: n, .. }
            | Stmt::ForOfSplitIter { var_name: n, .. }
            | Stmt::Try {
                catch_param: Some(n),
                ..
            } => {
                self.bound.insert(n);
            }
            _ => {}
        }
        nested_stmts(s, &mut |st| self.collect_stmt(st));
    }

    /// The constructor prefix writes, recorded and excluded from the
    /// general write census.
    fn prefix_assigns(&mut self) -> HashSet<ExprId> {
        let mut out = HashSet::default();
        for ctor in &self.ctors {
            if self.foreign_classes.contains(ctor.class) {
                continue;
            }
            for (e, w) in ctor_prefix(self.ast, (ctor.name, ctor.class), ctor.params, ctor.body) {
                out.insert(e);
                self.prefix.push(w);
            }
        }
        out
    }

    /// The factory seeds of the prefix-written fields, by literal.
    fn unread_seeds(&self) -> HashMap<ExprId, HashSet<&'a str>> {
        let mut out: HashMap<ExprId, HashSet<&'a str>> = HashMap::default();
        for s in &self.ast.stmts {
            let Stmt::FnDecl { name, body, .. } = s else {
                continue;
            };
            let Some(class) = name.strip_prefix("__new_") else {
                continue;
            };
            if self.foreign_classes.contains(class) {
                continue;
            }
            let Some(seed) = ctor_fed_seed(self.ast, class, body) else {
                continue;
            };
            let fields = (self.prefix.iter())
                .filter(|w| w.class == class)
                .map(|w| w.field);
            out.entry(seed).or_default().extend(fields);
        }
        out
    }
}

fn add_tree(ast: &Ast, e: ExprId, out: &mut ExprMask) {
    if out.insert(e) {
        visit_expr_children(ast, e, &mut |c| add_tree(ast, c, out));
    }
}
