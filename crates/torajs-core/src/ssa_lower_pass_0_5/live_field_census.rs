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
//! parameter that every `new` and `super(...)` fills with one. Fields and functions are judged together to a greatest fixed
//! point: a cycle (a function returning its own call, a field copied
//! from itself) is optimistic, which is sound because a non-live value
//! can only enter through a leaf shape, and that leaf fails on its own.
//!
//! The census is syntactic and over-approximates every road, like
//! `arr_head_census`; the cost of a false negative is the guard the
//! program paid before.

mod judge;
mod walk;

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::fresh_this::fresh_this_writes;
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::Type;
use crate::num_width::WidthTable;

use super::arr_head_census::{ExprMask, unreachable_twin_exprs};
use walk::{
    ctor_class, ctor_fed_seed, derives_from, factory_class, key_is_numeric, nested_stmts,
    never_an_instance, user_params,
};

/// `(class, field)` pairs whose reads never see the undefined cell.
pub(crate) type LiveFields = HashSet<(String, String)>;

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

pub(crate) fn program_live_fields(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    widths: &WidthTable,
) -> LiveFields {
    let dead_twins = unreachable_twin_exprs(ast, expr_types);
    if !ast.cmany_twins.is_empty() && dead_twins.is_empty() {
        return LiveFields::new();
    }
    let census = Census::collect(ast, expr_types, widths, &dead_twins);
    if census.road {
        return LiveFields::new();
    }
    census.solve()
}

struct Ctor<'a> {
    class: String,
    params: &'a [Param],
    body: &'a [Stmt],
}

/// One prefix write `this.f = v` of a constructor, with the ctor
/// parameter it copies when `v` is a bare parameter.
struct PrefixWrite {
    class: String,
    field: String,
    value: ExprId,
    param: Option<usize>,
}

struct Census<'a> {
    ast: &'a Ast,
    expr_types: &'a HashMap<ExprId, Type>,
    widths: &'a WidthTable,
    /// Some road no typed census closes was found: nothing is live.
    road: bool,
    ctors: Vec<Ctor<'a>>,
    fns: HashMap<String, (&'a [Param], &'a [Stmt])>,
    /// Every `let` / `const` initialiser, by name.
    let_inits: HashMap<String, Vec<ExprId>>,
    /// Names bound other than by `let` / `const` or a constructor
    /// parameter (other params, loop variables, catch params): a read
    /// of one cannot be judged.
    bound: HashSet<String>,
    /// The constructor parameters a name is bound by, as `(class,
    /// user parameter index)`: a read of one is judged by the class's
    /// construction sites.
    ctor_params: HashMap<String, Vec<(String, usize)>>,
    /// Names assigned to anywhere, or declared more than once as a
    /// function.
    assigned: HashSet<String>,
    /// Every `x.f = v` outside the constructor prefixes, by field.
    member_writes: HashMap<String, Vec<ExprId>>,
    /// Field names some `delete x.f` names.
    deleted: HashSet<String>,
    /// Field names written through a receiver not typed as a class.
    foreign: HashSet<String>,
    /// Classes a value other than an instance may be typed as.
    foreign_classes: HashSet<String>,
    /// The expressions of the constructor bodies, where `this` is the
    /// instance under construction.
    ctor_exprs: HashSet<ExprId>,
    prefix: Vec<PrefixWrite>,
    /// `new C(..)` and `super(..)` argument lists, by class.
    construct_args: HashMap<String, Vec<Vec<ExprId>>>,
    /// Factory seed literals and the fields of each that the
    /// constructor prefix overwrites unread.
    unread_seeds: HashMap<ExprId, HashSet<String>>,
}

impl<'a> Census<'a> {
    fn collect(
        ast: &'a Ast,
        expr_types: &'a HashMap<ExprId, Type>,
        widths: &'a WidthTable,
        dead: &ExprMask,
    ) -> Self {
        let mut c = Census {
            ast,
            expr_types,
            widths,
            road: false,
            ctors: Vec::new(),
            fns: HashMap::new(),
            let_inits: HashMap::new(),
            bound: HashSet::new(),
            ctor_params: HashMap::new(),
            assigned: HashSet::new(),
            member_writes: HashMap::new(),
            deleted: HashSet::new(),
            foreign: HashSet::new(),
            foreign_classes: ast.ctor_return_override.clone(),
            ctor_exprs: HashSet::new(),
            prefix: Vec::new(),
            construct_args: HashMap::new(),
            unread_seeds: HashMap::new(),
        };
        for s in &ast.stmts {
            c.collect_stmt(s);
        }
        for ctor in &c.ctors {
            if c.foreign_classes.contains(&ctor.class) {
                continue;
            }
            for s in ctor.body {
                visit_stmt_exprs(s, &mut |e| add_tree(ast, e, &mut c.ctor_exprs));
            }
        }
        let prefix_writes: HashSet<ExprId> = c.prefix_assigns();
        c.unread_seeds = c.unread_seeds();
        for (i, e) in ast.exprs.iter().enumerate() {
            let id = ExprId(i as u32);
            if dead.contains(id) || prefix_writes.contains(&id) {
                continue;
            }
            c.collect_expr(id, e);
        }
        c
    }

    /// A name some binding other than a function declaration may
    /// stand for.
    fn shadowed(&self, name: &str) -> bool {
        self.bound.contains(name)
            || self.let_inits.contains_key(name)
            || self.ctor_params.contains_key(name)
    }

    /// Declarations: functions, constructors, bindings.
    fn collect_stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                if let Some(class) = ctor_class(name) {
                    for (i, p) in user_params(params).iter().enumerate() {
                        self.ctor_params
                            .entry(p.name.clone())
                            .or_default()
                            .push((class.to_string(), i));
                    }
                    self.ctors.push(Ctor {
                        class: class.to_string(),
                        params,
                        body,
                    });
                } else {
                    // a factory's parameters are the constructor's,
                    // forwarded; they are judged at the construction sites
                    if factory_class(name).is_none() {
                        for p in params {
                            self.bound.insert(p.name.clone());
                        }
                    }
                    if self.fns.insert(name.clone(), (params, body)).is_some() {
                        self.assigned.insert(name.clone());
                    }
                }
                for st in body {
                    self.collect_stmt(st);
                }
            }
            Stmt::LetDecl { name, init, .. } => {
                self.let_inits.entry(name.clone()).or_default().push(*init);
            }
            Stmt::UsingDecl { name, .. } | Stmt::YieldInto { var: name, .. } => {
                self.bound.insert(name.clone());
            }
            Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                self.bound.insert(var_name.clone());
            }
            Stmt::Try {
                catch_param: Some(p),
                ..
            } => {
                self.bound.insert(p.clone());
            }
            _ => {}
        }
        nested_stmts(s, &mut |st| self.collect_stmt(st));
    }

    /// The constructor prefix writes, recorded and excluded from the
    /// general write census.
    fn prefix_assigns(&mut self) -> HashSet<ExprId> {
        let mut out = HashSet::new();
        for ctor in &self.ctors {
            if self.foreign_classes.contains(&ctor.class) {
                continue;
            }
            let name = format!("__cm_{}__ctor", ctor.class);
            let fresh = fresh_this_writes(self.ast, &name, ctor.params, ctor.body);
            let user = user_params(ctor.params);
            for s in ctor.body {
                let Stmt::Expr(e) = s else { continue };
                let Expr::Assign { target, value } = self.ast.get_expr(*e) else {
                    continue;
                };
                let Expr::Member { obj, name } = self.ast.get_expr(*target) else {
                    continue;
                };
                if !fresh.recv.contains(obj) {
                    continue;
                }
                let param = match self.ast.get_expr(*value) {
                    Expr::Ident(n) if user.iter().any(|p| p.name == *n) => {
                        user.iter().position(|p| p.name == *n)
                    }
                    _ => None,
                };
                out.insert(*e);
                self.prefix.push(PrefixWrite {
                    class: ctor.class.clone(),
                    field: name.clone(),
                    value: *value,
                    param,
                });
            }
        }
        out
    }

    /// The factory seeds of the prefix-written fields, by literal.
    fn unread_seeds(&self) -> HashMap<ExprId, HashSet<String>> {
        let mut out: HashMap<ExprId, HashSet<String>> = HashMap::new();
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
                .map(|w| w.field.clone());
            out.entry(seed).or_default().extend(fields);
        }
        out
    }

    fn collect_expr(&mut self, id: ExprId, e: &'a Expr) {
        match e {
            Expr::Ident(n) => {
                if ROADS.contains(&n.as_str()) {
                    self.road = true;
                }
            }
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
                if ROADS.contains(&name.as_str()) {
                    self.road = true;
                }
            }
            Expr::String(s) => {
                if ROADS.iter().any(|n| s == *n) {
                    self.road = true;
                }
            }
            Expr::Assign { target, value } => match self.ast.get_expr(*target) {
                // a write through a receiver that is not a class
                // instance by type (`any`, a struct) may store anything
                Expr::Member { obj, name } => {
                    if matches!(self.expr_types.get(obj), Some(Type::ClassRef(_))) {
                        self.member_writes
                            .entry(name.clone())
                            .or_default()
                            .push(*value);
                    } else {
                        self.foreign.insert(name.clone());
                    }
                }
                Expr::Ident(n) => {
                    self.assigned.insert(n.clone());
                }
                Expr::Index { obj, index } => {
                    if !key_is_numeric(self.expr_types.get(index))
                        && !never_an_instance(self.expr_types.get(obj))
                    {
                        self.road = true;
                    }
                }
                _ => self.road = true,
            },
            Expr::Delete { expr } => match self.ast.get_expr(*expr) {
                Expr::Member { name, .. } => {
                    self.deleted.insert(name.clone());
                }
                _ => self.road = true,
            },
            Expr::New {
                class_name, args, ..
            } => {
                let class = class_name.split('<').next().unwrap_or(class_name);
                self.construct_args
                    .entry(class.to_string())
                    .or_default()
                    .push(args.clone());
            }
            Expr::NewDynamic { .. } => self.road = true,
            Expr::ObjectLit { fields } => {
                let unread = self.unread_seeds.get(&id).cloned();
                for (k, v) in fields {
                    if unread
                        .as_ref()
                        .is_some_and(|u| u.contains(k.lossy().as_ref()))
                    {
                        continue;
                    }
                    match self.ast.get_expr(*v) {
                        Expr::Spread { .. } => self.road = true,
                        // a scalar never fills a pointer-shaped field
                        _ if matches!(
                            self.expr_types.get(v),
                            Some(Type::Number | Type::Boolean | Type::String)
                        ) => {}
                        _ => self
                            .member_writes
                            .entry(k.lossy().into_owned())
                            .or_default()
                            .push(*v),
                    }
                }
            }
            Expr::As { expr, .. } => {
                if let Some(Type::ClassRef(c)) = self.expr_types.get(&id) {
                    let c = c.split('<').next().unwrap_or(c);
                    let instance = match self.expr_types.get(expr) {
                        Some(Type::Any) => true,
                        Some(Type::ClassRef(d)) => {
                            let d = d.split('<').next().unwrap_or(d);
                            d == c || derives_from(self.ast, d, c)
                        }
                        _ => false,
                    };
                    if !instance {
                        self.foreign_classes.insert(c.to_string());
                    }
                }
            }
            // `new C(..)` of a user class is a call of its factory
            Expr::Call { callee, args } => {
                if let Expr::Ident(n) = self.ast.get_expr(*callee)
                    && let Some(class) = factory_class(n)
                {
                    self.construct_args
                        .entry(class.to_string())
                        .or_default()
                        .push(args.clone());
                }
            }
            Expr::ArrowFn { params, body, .. } => {
                for p in params {
                    self.bound.insert(p.name.clone());
                }
                for st in body {
                    self.collect_stmt(st);
                }
            }
            _ => {}
        }
    }
}

fn add_tree(ast: &Ast, e: ExprId, out: &mut HashSet<ExprId>) {
    if out.insert(e) {
        visit_expr_children(ast, e, &mut |c| add_tree(ast, c, out));
    }
}
