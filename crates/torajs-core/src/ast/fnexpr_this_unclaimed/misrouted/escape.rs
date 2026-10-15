//! An unclaimed function expression whose value leaves the sites
//! [`super::misroutes`] judges.
//!
//! The parent scan reads the three invocation shapes it can see —
//! `new f()`, `f()`, `f.call(t)` / `f.apply(t, …)` — and leaves every
//! other use alone. But the value of such an expression carries the
//! ENCLOSING `__this` wherever it goes, and every other use hands it
//! to code whose receiver tr does not see:
//!
//! * a callee spelled through another expression — `f?.()`,
//!   `(0, f)()`, `(c ? f : g)()`, `(f || g)()`, `(g = f)()` — runs it
//!   with `undefined` (§13.3.6.2: the callee is no Reference);
//! * `f.bind(x)` — §20.2.3.2 fixes the receiver to `x` (the enclosing
//!   `this` is the one the body reads, so `f.bind(this)` stays);
//! * an argument, a parameter default, a return value, an array or
//!   object-literal element, a store into a property or into a name
//!   the parent does not track, a thrown or yielded value — whatever
//!   later invokes it supplies a receiver of its own.
//!
//! In each, the body reads the receiver of the function that minted
//! it, which matches the receiver it is given only by accident. So
//! any such use refuses. Uses that cannot invoke the value stay
//! allowed: a property read other than `bind` / `call` / `apply`,
//! `typeof`, `!`, a comparison or arithmetic operand, an `instanceof`
//! operand, a condition, an expression statement, an argument to a
//! top-level function whose body never names the parameter receiving
//! it (nor `arguments`), and a store into a property of the enclosing
//! `this` itself — read back through that object, the receiver is the
//! one the body reads. A write into a binding the parent tracks
//! (initializer or assignment) is judged at that binding's uses; a
//! binding it does not track (written elsewhere, holding other values
//! too, a `var` declared twice) is not followed, so a write into one
//! refuses. The exception is a name declared by `let` / `const` in
//! sibling blocks and never written: each such binding holds its
//! initializer alone, so the initializer stands here, and the binding
//! that holds the function expression is judged within its own block
//! (`super::scoped`).

use std::collections::{HashMap, HashSet};

use super::{Fns, Held, Outer, Skip};
use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::fnexpr_this_default::unclaimed_fnexpr;
use crate::ast::fnexpr_this_names::peel_as;
use crate::ast::{Ast, BinOp, Expr, ExprId, Param, Stmt};

/// `true` = in `body`, a tracked binding or an inline unclaimed
/// function expression stands in a position that hands its value on.
pub(super) fn escapes(
    ast: &Ast,
    fns: &Fns<'_>,
    (params, body): (&[Param], &[Stmt]),
    (held, scoped): (&HashMap<&str, Held>, &HashSet<&str>),
    outer: Option<Outer>,
    skip: &Skip,
) -> bool {
    let scan = Scan {
        ast,
        fns,
        held,
        scoped,
        outer,
        skip,
    };
    scan.defaults(params) || scan.stmts(body)
}

struct Scan<'a> {
    ast: &'a Ast,
    fns: &'a Fns<'a>,
    held: &'a HashMap<&'a str, Held>,
    scoped: &'a HashSet<&'a str>,
    outer: Option<Outer>,
    skip: &'a Skip,
}

impl Scan<'_> {
    /// A parameter default hands its value to a binding no one tracks.
    fn defaults(&self, params: &[Param]) -> bool {
        params
            .iter()
            .filter_map(|p| p.default)
            .any(|d| self.expr(d, false))
    }

    fn stmts(&self, stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| self.stmt(s))
    }

    fn stmt(&self, s: &Stmt) -> bool {
        if matches!(s, Stmt::FnDecl { .. }) || self.skip.contains(&(s as *const Stmt)) {
            return false;
        }
        let safe_root = match s {
            Stmt::Expr(_)
            | Stmt::If { .. }
            | Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::Switch { .. }
            | Stmt::For { .. } => true,
            Stmt::LetDecl { name, .. } => {
                self.held.contains_key(name.as_str()) || self.scoped.contains(name.as_str())
            }
            _ => false,
        };
        if stmt_exprs(s).into_iter().any(|e| self.expr(e, safe_root)) {
            return true;
        }
        stmt_children_ref(s).into_iter().any(|c| self.stmt(c))
    }

    fn target(&self, e: ExprId) -> bool {
        match self.ast.get_expr(e) {
            Expr::Ident(n) => self.held.contains_key(n.as_str()),
            _ => self.outer.is_some() && unclaimed_fnexpr(self.ast, e).is_some(),
        }
    }

    /// `safe` = the parent position cannot hand the value on.
    fn expr(&self, e: ExprId, safe: bool) -> bool {
        if self.target(e) {
            return !safe;
        }
        let ast = self.ast;
        let kids: Vec<(ExprId, bool)> = match ast.get_expr(e) {
            Expr::As { expr, .. } => vec![(*expr, safe)],
            // the invocation itself is `misroutes`' to judge
            Expr::Call { callee, args } | Expr::NewDynamic { callee, args } => {
                let c = peel_as(&ast.exprs, *callee);
                // any other callee expression hands its value to the call
                let mut v = match ast.get_expr(c) {
                    Expr::Member { obj, name } if name == "call" || name == "apply" => {
                        vec![(*obj, self.target(peel_as(&ast.exprs, *obj)))]
                    }
                    Expr::Member { obj, name } if name == "bind" => {
                        let own = args.first().is_some_and(|&a| self.is_this(a));
                        vec![(*obj, own && self.reads_this(*obj))]
                    }
                    _ => vec![(c, self.target(c))],
                };
                let inert = match ast.get_expr(c) {
                    Expr::Ident(g) if matches!(ast.get_expr(e), Expr::Call { .. }) => {
                        self.inert_params(g, args.len())
                    }
                    _ => Vec::new(),
                };
                v.extend(
                    args.iter()
                        .enumerate()
                        .map(|(i, &a)| (a, inert.get(i).copied().unwrap_or(false))),
                );
                v
            }
            Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
                vec![(*obj, !matches!(name.as_str(), "bind" | "call" | "apply"))]
            }
            Expr::TypeOf { expr } | Expr::Unary { expr, .. } | Expr::Delete { expr } => {
                vec![(*expr, true)]
            }
            Expr::InstanceOf { expr, rhs } => vec![(*expr, true), (*rhs, true)],
            Expr::BinOp { op, left, right } => {
                let flows = matches!(op, BinOp::LAnd | BinOp::LOr);
                vec![(*left, !flows), (*right, !flows)]
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => vec![(*cond, true), (*then_branch, false), (*else_branch, false)],
            Expr::Sequence { left, right } => vec![(*left, true), (*right, false)],
            Expr::Assign { target, value } => {
                // the assignment's own value flows on unless discarded
                vec![
                    (*target, true),
                    (*value, safe && self.kept_store(*target, *value)),
                ]
            }
            // an arrow shares the body's bindings and receiver
            Expr::ArrowFn { params, body, .. } => {
                return self.defaults(params) || self.stmts(body);
            }
            _ => expr_children(ast, e)
                .into_iter()
                .map(|c| (c, false))
                .collect(),
        };
        kids.into_iter().any(|(c, s)| self.expr(c, s))
    }
}

impl Scan<'_> {
    /// `target = value` keeps the value where its receiver stays the
    /// one its body reads: a binding the parent tracks, or a property
    /// of the enclosing `this` when the value reads that same `this`.
    fn kept_store(&self, target: ExprId, value: ExprId) -> bool {
        let ast = self.ast;
        match ast.get_expr(peel_as(&ast.exprs, target)) {
            Expr::Ident(n) => self.held.contains_key(n.as_str()),
            Expr::Member { obj, .. } | Expr::Index { obj, .. } => {
                self.is_this(*obj) && self.reads_this(value)
            }
            _ => false,
        }
    }

    fn is_this(&self, e: ExprId) -> bool {
        matches!(self.ast.get_expr(peel_as(&self.ast.exprs, e)), Expr::Ident(n) if n == "__this")
    }

    /// The value's body reads the `this` this body has: an inline
    /// expression always, a binding when no function with its own
    /// receiver sits between its mint and here.
    fn reads_this(&self, value: ExprId) -> bool {
        match self.ast.get_expr(peel_as(&self.ast.exprs, value)) {
            Expr::Ident(n) => self.held.get(n.as_str()).is_some_and(|h| h.1),
            _ => true,
        }
    }

    /// Per argument slot of a direct call to top-level function `g`:
    /// `true` when its body cannot reach the value — the parameter is
    /// never named there, nor captured by a closure it mints, and the
    /// body does not read `arguments`. Empty when `g` is not such a
    /// function or the arity does not line up.
    fn inert_params(&self, g: &str, argc: usize) -> Vec<bool> {
        let Some(&(params, body)) = self.fns.get(g) else {
            return Vec::new();
        };
        if params.len() != argc || params.iter().any(|p| p.name.starts_with("__")) {
            return Vec::new();
        }
        let mut named: HashSet<&str> = HashSet::new();
        let mut roots: Vec<ExprId> = params.iter().filter_map(|p| p.default).collect();
        let mut stmts: Vec<&Stmt> = body.iter().collect();
        while let Some(s) = stmts.pop() {
            if let Stmt::FnDecl { .. } = s {
                return Vec::new();
            }
            roots.extend(stmt_exprs(s));
            stmts.extend(stmt_children_ref(s));
        }
        while let Some(e) = roots.pop() {
            match self.ast.get_expr(e) {
                Expr::Ident(n) => {
                    named.insert(n);
                }
                Expr::Closure { captures, .. } => named.extend(captures.iter().map(String::as_str)),
                Expr::ArrowFn { .. } => return Vec::new(),
                _ => {}
            }
            roots.extend(expr_children(self.ast, e));
        }
        if named
            .iter()
            .any(|n| *n == "arguments" || n.starts_with("__arg"))
        {
            return Vec::new();
        }
        params
            .iter()
            .map(|p| !p.is_rest && !named.contains(p.name.as_str()))
            .collect()
    }
}
