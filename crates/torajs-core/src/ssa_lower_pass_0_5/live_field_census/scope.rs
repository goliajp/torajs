//! Which function an expression sits in, and which binders a name read
//! there can reach.
//!
//! A parameter or a local is visible only in its own function, in the
//! functions declared inside it, and in the closures it is captured
//! into, besides the program's top level; a read elsewhere is of some
//! other binder of the name. Within one function every binder of the
//! name is taken, which over-approximates block scoping.

use std::cell::RefCell;

use super::names::{HashMap, HashSet};

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};

use super::bindings::judged_fn;
use super::walk::{Few, ctor_class, nested_stmts, user_params};

/// The function an expression is in: the program's own body, a
/// declared function by index, or none the walk reached (an arrow
/// body that was not lifted), whose reads are never judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Owner {
    Unknown,
    Global,
    Fn(u32),
}

/// What a parameter binder is judged by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Binder<'a> {
    /// User parameter `i` of a constructor of the class.
    Ctor(&'a str, usize),
    /// Parameter `i` of a declared function judged at its call sites.
    Param(&'a str, usize),
    /// Nothing: a hidden, receiver, environment or factory parameter,
    /// or one of a function whose calls are not all visible.
    Open,
}

#[derive(Default)]
pub(super) struct Scopes<'a> {
    owners: Vec<Owner>,
    ids: HashMap<&'a str, u32>,
    /// The function each declared function is nested in, by id.
    parent: Vec<Owner>,
    /// Where each lifted closure is built, with the names it captures.
    creators: HashMap<u32, Vec<(Owner, &'a [String])>>,
    /// Each function's parameters, by name.
    params: HashMap<(&'a str, u32), Binder<'a>>,
    /// The other binders each function (or the top level) declares.
    decls: HashMap<(&'a str, Owner), Decl>,
    /// Every parameter name of every function.
    pub(super) param_names: HashSet<&'a str>,
    /// The binders each read reaches, as answered so far.
    memo: RefCell<ReachMemo<'a>>,
}

/// A read's reach by the function it is in and the name it reads.
type ReachMemo<'a> = HashMap<(Owner, &'a str), Option<Reach<'a>>>;

/// The binders other than parameters one function declares for a name.
#[derive(Default)]
struct Decl {
    /// `let` / `const` initialisers.
    lets: Few<ExprId>,
    /// A loop variable, a catch parameter, an arrow's parameter or any
    /// other binder no census judges.
    open: bool,
    /// A split loop's piece, which is a string.
    split: bool,
}

/// The binders a read can reach.
#[derive(Default, Clone)]
pub(super) struct Reach<'a> {
    pub(super) params: Few<Binder<'a>>,
    pub(super) lets: Few<ExprId>,
    pub(super) open: bool,
    pub(super) split: bool,
}

impl<'a> Scopes<'a> {
    /// `(functions, parameters, other binders)` size the tables.
    pub(super) fn collect(ast: &'a Ast, (fns, params, binders): (usize, usize, usize)) -> Self {
        let mut s = Scopes {
            owners: vec![Owner::Unknown; ast.exprs.len()],
            ids: HashMap::with_capacity_and_hasher(fns, Default::default()),
            parent: Vec::with_capacity(fns),
            creators: HashMap::default(),
            params: HashMap::with_capacity_and_hasher(params, Default::default()),
            decls: HashMap::with_capacity_and_hasher(binders, Default::default()),
            param_names: HashSet::with_capacity_and_hasher(params, Default::default()),
            memo: RefCell::default(),
        };
        for st in &ast.stmts {
            s.walk(ast, st, Owner::Global);
        }
        s
    }

    /// A closure of the lifted function `fn_name` built at `at`,
    /// capturing `captures`; the census's pass over the expressions
    /// reports every one before the first [`Scopes::reach`].
    pub(super) fn closure(&mut self, at: ExprId, fn_name: &str, captures: &'a [String]) {
        if let Some(&id) = self.ids.get(fn_name) {
            let owner = self.owner(at);
            self.creators.entry(id).or_default().push((owner, captures));
        }
    }

    fn id(&mut self, name: &'a str) -> u32 {
        let next = self.ids.len() as u32;
        *self.ids.entry(name).or_insert(next)
    }

    fn walk(&mut self, ast: &'a Ast, st: &'a Stmt, owner: Owner) {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = st
        {
            let id = self.id(name);
            let at = id as usize;
            if at == self.parent.len() {
                self.parent.push(owner);
            }
            self.parent[at] = owner;
            let ctor = ctor_class(name);
            let user = ctor.map(|_| user_params(params)).unwrap_or_default();
            let judged = judged_fn(name, params);
            for (i, p) in params.iter().enumerate() {
                self.param_names.insert(&p.name);
                let binder = match ctor {
                    Some(class) => match user.iter().position(|u| u.name == p.name) {
                        Some(k) => Binder::Ctor(class, k),
                        None => Binder::Open,
                    },
                    None if judged => Binder::Param(name, i),
                    None => Binder::Open,
                };
                // a function declared twice keeps no judgement
                let at = self.params.entry((&p.name, id)).or_insert(binder);
                if *at != binder {
                    *at = Binder::Open;
                }
            }
            for b in body {
                self.walk(ast, b, Owner::Fn(id));
            }
            return;
        }
        visit_stmt_exprs(st, &mut |e| self.mark(ast, e, owner));
        self.declare(ast, st, owner);
    }

    /// The binders `st` and the statements nested in it declare, and
    /// the functions declared among them, whose expressions
    /// `visit_stmt_exprs` already marked.
    fn declare(&mut self, ast: &'a Ast, st: &'a Stmt, owner: Owner) {
        match st {
            Stmt::FnDecl { .. } => return self.walk(ast, st, owner),
            Stmt::LetDecl { name, init, .. } => self.decl(owner, name).lets.push(*init),
            Stmt::ForOfSplitIter { var_name, .. } => self.decl(owner, var_name).split = true,
            Stmt::ForOf { var_name: n, .. }
            | Stmt::UsingDecl { name: n, .. }
            | Stmt::YieldInto { var: n, .. }
            | Stmt::Try {
                catch_param: Some(n),
                ..
            } => self.decl(owner, n).open = true,
            _ => {}
        }
        nested_stmts(st, &mut |inner| self.declare(ast, inner, owner));
    }

    fn decl(&mut self, owner: Owner, n: &'a str) -> &mut Decl {
        self.decls.entry((n, owner)).or_default()
    }

    fn mark(&mut self, ast: &'a Ast, e: ExprId, owner: Owner) {
        let Some(slot) = self.owners.get_mut(e.0 as usize) else {
            return;
        };
        *slot = owner;
        if let Expr::ArrowFn { params, .. } = ast.get_expr(e) {
            for p in params {
                self.decl(owner, &p.name).open = true;
            }
        }
        // a leaf has nothing under it to visit
        visit_expr_children(ast, e, &mut |c| match ast.get_expr(c) {
            Expr::Ident(_) | Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null => {
                if let Some(slot) = self.owners.get_mut(c.0 as usize) {
                    *slot = owner;
                }
            }
            _ => self.mark(ast, c, owner),
        });
    }

    pub(super) fn owner(&self, e: ExprId) -> Owner {
        self.owners
            .get(e.0 as usize)
            .copied()
            .unwrap_or(Owner::Unknown)
    }

    /// The binders of `n` a read in `owner` can reach, the top level's
    /// among them, or `None` when the read is in no function the walk
    /// reached.
    pub(super) fn reach(&self, owner: Owner, n: &'a str) -> Option<Reach<'a>> {
        if let Some(r) = self.memo.borrow().get(&(owner, n)) {
            return r.clone();
        }
        let r = self.walk_reach(owner, n);
        self.memo.borrow_mut().insert((owner, n), r.clone());
        r
    }

    fn walk_reach(&self, owner: Owner, n: &'a str) -> Option<Reach<'a>> {
        let mut seen = Few::default();
        let mut out = Reach::default();
        if !self.chain(owner, n, &mut seen, &mut out) {
            return None;
        }
        self.take_decl(Owner::Global, n, &mut out);
        Some(out)
    }

    fn take_decl(&self, at: Owner, n: &'a str, out: &mut Reach<'a>) {
        if let Some(d) = self.decls.get(&(n, at)) {
            d.lets.as_slice().iter().for_each(|e| out.lets.push(*e));
            out.open |= d.open;
            out.split |= d.split;
        }
    }

    fn chain(&self, owner: Owner, n: &'a str, seen: &mut Few<u32>, out: &mut Reach<'a>) -> bool {
        let id = match owner {
            Owner::Unknown => return false,
            Owner::Global => return true,
            Owner::Fn(id) => id,
        };
        if seen.as_slice().contains(&id) {
            return true;
        }
        seen.push(id);
        if let Some(b) = self.params.get(&(n, id)) {
            out.params.push(*b);
        }
        self.take_decl(owner, n, out);
        for (at, caps) in self.creators.get(&id).into_iter().flatten() {
            if caps.iter().any(|c| c == n) && !self.chain(*at, n, seen, out) {
                return false;
            }
        }
        match self.parent.get(id as usize) {
            Some(p) => self.chain(*p, n, seen, out),
            None => true,
        }
    }
}
