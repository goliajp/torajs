//! Call-site rounds of [`crate::ast_closure_param_tag`] that know
//! where each call sits. A closure an object literal stores
//! (`{ k(cb) {...} }`, `{ k: t }`) is called as `o.k(...)`, which the
//! rounds keyed on a callee ident never see: its fn-typed params kept
//! the bare-pointer repr while an arrow reached them as a cell, and the
//! body called through the cell's header (SIGBUS). Such calls are
//! matched here by property name.
//!
//! For those calls and the ident-keyed ones alike, a closure-shaped
//! argument marks the param, and a marked param pulls the closure repr
//! back onto what reaches it, against the call direction until the
//! chain bottoms out at real values: a fn-typed param of the function
//! the call sits in (or of the one it captured the name from), or a
//! function whose returned value it is. Left as a bare pointer, that
//! value reached a slot the callee reads as a closure cell (probe
//! p631c, rc 138). The name is resolved where the call sits, so a
//! param elsewhere that only shares the name is left alone.
//!
//! A stored closure whose method value is read other than as a callee
//! (`const f = o.k`) is called where no round looks, so all its
//! fn-typed params take the closure repr. A property name shared by
//! unrelated receivers only costs the matched params their
//! direct-dispatch ABI.

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};
use crate::ast_closure_param_tag::push_child_stmts;
use std::collections::{HashMap, HashSet};

/// A callee's fn-typed params: (`fn_params` key, count of leading
/// synthetic params the call does not pass).
type Holder = (String, usize);

/// A call into known params: the callees, how many leading arguments
/// precede the user ones (the receiver of a class-method call the
/// class desugar made static), the arguments, and the function the
/// call sits in.
type CallSite = (Vec<Holder>, usize, Vec<ExprId>, Option<String>);

/// The program facts the rounds read, gathered once.
pub(crate) struct CallSites {
    /// property name → the stored closures under it
    holders: HashMap<String, Vec<Holder>>,
    calls: Vec<CallSite>,
    /// keys whose method value is read other than as a callee
    escaped: HashSet<String>,
    /// function → where its free names resolve
    parents: HashMap<String, Parent>,
    /// (function, local) → the name `let local = name` binds it to
    local_aliases: HashMap<(String, String), String>,
}

/// The function a lifted closure is created in, with the names it
/// captures; or the one a nested declaration sits in, whose names it
/// may read (`None`: not listed).
type Parent = (Option<String>, Option<Vec<String>>);

/// What one round adds: param marks and return-lane marks.
pub(crate) type RoundMarks = (Vec<(String, usize)>, Vec<String>);

/// Method name → the class methods `__cm_<C>__<k>` with fn-typed
/// params, for a name unrelated classes declare: only then does the
/// class desugar leave `x.k(...)` a member call, until the receiver's
/// type picks the class. A name one class declares becomes a direct
/// call (a member call left under it is a builtin's, `this.xs.map(..)`),
/// and an override chain calls its `__dispatch_` stub.
fn sibling_methods<'a>(
    ast: &'a Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
    shifts: &HashMap<&str, usize>,
) -> HashMap<&'a str, Vec<Holder>> {
    let mut out: HashMap<&str, Vec<Holder>> = HashMap::new();
    for (name, owners) in &ast.method_owners {
        let base = &owners[0];
        let chain = owners[1..]
            .iter()
            .all(|sub| crate::ast::method_owner_is_in_chain(&ast.class_parents, base, sub));
        if chain || ast.accessor_slots.contains(name) {
            continue;
        }
        for owner in owners {
            let callee = format!("__cm_{owner}__{name}");
            if fn_params.contains_key(&callee) {
                let shift = shifts.get(callee.as_str()).copied().unwrap_or(0);
                out.entry(name.as_str()).or_default().push((callee, shift));
            }
        }
    }
    out
}

/// Per function (by name), its `const` locals initialized from a bare
/// name: `const k = h` lets a relay through `k` reach `h`. A `let`
/// stays out, since a later write may bind a different function.
fn local_aliases(ast: &Ast, decls: &[(&str, &[Stmt], usize)]) -> HashMap<(String, String), String> {
    let mut out = HashMap::new();
    for (name, body, _) in decls {
        let mut stack: Vec<&Stmt> = body.iter().collect();
        while let Some(s) = stack.pop() {
            match s {
                Stmt::FnDecl { .. } => continue,
                Stmt::LetDecl {
                    name: local,
                    init,
                    mutable: false,
                    ..
                } => {
                    if let Expr::Ident(n) = ast.get_expr(*init) {
                        out.insert((name.to_string(), local.clone()), n.clone());
                    }
                }
                _ => {}
            }
            push_child_stmts(s, &mut stack);
        }
    }
    out
}

impl CallSites {
    pub(crate) fn collect(
        ast: &Ast,
        fn_params: &HashMap<String, Vec<(usize, String)>>,
        aliases: &crate::ast_closure_param_tag_collect::ClosureAliases,
    ) -> Self {
        let mut decls: Vec<(&str, &[Stmt], usize)> = Vec::new();
        let mut parents: HashMap<String, Parent> = HashMap::new();
        let mut stack: Vec<(&Stmt, Option<&str>)> = ast.stmts.iter().map(|s| (s, None)).collect();
        while let Some((s, mut encl)) = stack.pop() {
            if let Stmt::FnDecl {
                name, params, body, ..
            } = s
            {
                let shift = params
                    .iter()
                    .take_while(|p| p.name == "__env" || p.name == "__this")
                    .count();
                decls.push((name, body, shift));
                if let Some(p) = encl {
                    parents.insert(name.clone(), (Some(p.to_string()), None));
                }
                encl = Some(name);
            }
            let mut kids = Vec::new();
            push_child_stmts(s, &mut kids);
            stack.extend(kids.into_iter().map(|k| (k, encl)));
        }
        let shifts: HashMap<&str, usize> = decls.iter().map(|(n, _, s)| (*n, *s)).collect();
        let mut holders: HashMap<String, Vec<Holder>> = HashMap::new();
        for e in &ast.exprs {
            let Expr::ObjectLit { fields } = e else {
                continue;
            };
            for (key, v) in fields {
                let held = match ast.get_expr(*v) {
                    Expr::Closure { fn_name, .. } if fn_params.contains_key(fn_name) => {
                        let shift = shifts.get(fn_name.as_str()).copied().unwrap_or(0);
                        Some((fn_name.clone(), shift))
                    }
                    Expr::Ident(n) if aliases.contains_key(n) && fn_params.contains_key(n) => {
                        Some((n.clone(), 0))
                    }
                    _ => None,
                };
                if let Some(h) = held {
                    holders.entry(key.lossy().into_owned()).or_default().push(h);
                }
            }
        }
        let methods = sibling_methods(ast, fn_params, &shifts);
        let mut walk = Walk {
            ast,
            fn_params,
            holders: &holders,
            methods: &methods,
            calls: Vec::new(),
            escaped: HashSet::new(),
            parents,
        };
        for s in &ast.stmts {
            visit_stmt_exprs(s, &mut |e| walk.expr(e, None));
        }
        for (name, body, _) in &decls {
            for s in body.iter() {
                visit_stmt_exprs(s, &mut |e| walk.expr(e, Some(*name)));
            }
        }
        let Walk {
            calls,
            escaped,
            parents,
            ..
        } = walk;
        CallSites {
            holders,
            calls,
            escaped,
            parents,
            local_aliases: local_aliases(ast, &decls),
        }
    }

    /// The fn-typed params of every stored closure whose method value
    /// escapes a callee position.
    pub(crate) fn escaped_marks(
        &self,
        fn_params: &HashMap<String, Vec<(usize, String)>>,
    ) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        for key in &self.escaped {
            for (h, _) in self.holders.get(key).into_iter().flatten() {
                let fps = fn_params.get(h).into_iter().flatten();
                out.extend(fps.map(|(i, _)| (h.clone(), *i)));
            }
        }
        out
    }

    /// One round over the calls (see the module doc); the caller's
    /// fixpoint folds the marks in and reruns it until nothing changes.
    pub(crate) fn round(
        &self,
        ast: &Ast,
        fn_params: &HashMap<String, Vec<(usize, String)>>,
        fn_returns: &HashMap<String, Vec<ExprId>>,
        marked: &HashSet<(String, usize)>,
        shaped: &dyn Fn(ExprId) -> bool,
    ) -> RoundMarks {
        let (mut marks, mut rets) = (Vec::new(), Vec::new());
        for (callees, lead, args, encl) in &self.calls {
            for (h, shift) in callees {
                for (pidx, _) in fn_params.get(h).into_iter().flatten() {
                    let at = pidx.checked_sub(*shift).map(|i| i + lead);
                    let Some(&arg) = at.and_then(|i| args.get(i)) else {
                        continue;
                    };
                    if !marked.contains(&(h.clone(), *pidx)) {
                        if shaped(arg) {
                            marks.push((h.clone(), *pidx));
                        }
                        continue;
                    }
                    for leaf in passthrough_leaves(ast, arg) {
                        match ast.get_expr(leaf) {
                            Expr::Ident(n) => {
                                let n = self.unalias(n, encl.as_deref());
                                marks.extend(self.param_owner(n, encl.as_deref(), fn_params));
                            }
                            Expr::Call { callee, .. } => {
                                if let Expr::Ident(g) = ast.get_expr(*callee)
                                    && fn_returns.contains_key(g)
                                {
                                    rets.push(g.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        (marks, rets)
    }

    /// What a local that only renames another binding stands for
    /// inside `f` (`const k = h; hof(k)`), followed to the end of the
    /// chain; `n` itself otherwise.
    fn unalias<'a>(&'a self, mut n: &'a str, f: Option<&str>) -> &'a str {
        let Some(f) = f else {
            return n;
        };
        let mut seen = HashSet::new();
        while seen.insert(n)
            && let Some(m) = self.local_aliases.get(&(f.to_string(), n.to_string()))
        {
            n = m;
        }
        n
    }

    /// The fn-typed param `n` names inside `f`: `f`'s own, or, when `f`
    /// is a lifted closure capturing `n`, the one it names where the
    /// closure is created.
    fn param_owner<'a>(
        &'a self,
        n: &str,
        mut f: Option<&'a str>,
        fn_params: &HashMap<String, Vec<(usize, String)>>,
    ) -> Option<(String, usize)> {
        while let Some(name) = f {
            let own = fn_params.get(name).into_iter().flatten();
            if let Some((i, _)) = own.into_iter().find(|(_, p)| p == n) {
                return Some((name.to_string(), *i));
            }
            let (parent, caps) = self.parents.get(name)?;
            if caps
                .as_ref()
                .is_some_and(|caps| !caps.iter().any(|c| c == n))
            {
                return None;
            }
            f = parent.as_deref();
        }
        None
    }
}

/// The operands `e` may answer as its own value, through the
/// operators that hand one straight through.
pub(crate) fn passthrough_leaves(ast: &Ast, e: ExprId) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut stack = vec![e];
    while let Some(e) = stack.pop() {
        match ast.get_expr(e) {
            Expr::Ternary {
                then_branch: a,
                else_branch: b,
                ..
            }
            | Expr::Nullish { lhs: a, rhs: b }
            | Expr::BinOp {
                op: BinOp::LAnd | BinOp::LOr,
                left: a,
                right: b,
            } => stack.extend([*a, *b]),
            Expr::Sequence { right: v, .. } | Expr::As { expr: v, .. } => stack.push(*v),
            _ => out.push(e),
        }
    }
    out
}

struct Walk<'a> {
    ast: &'a Ast,
    fn_params: &'a HashMap<String, Vec<(usize, String)>>,
    holders: &'a HashMap<String, Vec<Holder>>,
    /// [`sibling_methods`]
    methods: &'a HashMap<&'a str, Vec<Holder>>,
    calls: Vec<CallSite>,
    escaped: HashSet<String>,
    parents: HashMap<String, Parent>,
}

impl Walk<'_> {
    fn expr(&mut self, e: ExprId, encl: Option<&str>) {
        let ast = self.ast;
        let encl_owned = || encl.map(str::to_string);
        match ast.get_expr(e) {
            Expr::Call { callee, args } => match ast.get_expr(*callee) {
                Expr::Member { obj, name } => {
                    if let Some(hs) = self.holders.get(name) {
                        self.calls.push((hs.clone(), 0, args.clone(), encl_owned()));
                    }
                    // a method name two unrelated classes declare keeps
                    // the member call until the receiver's type picks
                    // the class, so every such method is a callee here
                    if let Some(hs) = self.methods.get(name.as_str()) {
                        self.calls.push((hs.clone(), 0, args.clone(), encl_owned()));
                    }
                    self.expr(*obj, encl);
                    for a in args {
                        self.expr(*a, encl);
                    }
                    return;
                }
                Expr::Ident(f) => {
                    if self.fn_params.contains_key(f) {
                        let site = (vec![(f.clone(), 0)], 0, args.clone(), encl_owned());
                        self.calls.push(site);
                    }
                    // the class desugar turns `x.k(...)` for a method one
                    // class declares into `__cm_<C>__k(x, ...)` whatever
                    // `x` is, so a stored closure under `k` is reached too
                    let static_method = |k: &str| {
                        f.strip_prefix("__cm_")
                            .and_then(|r| r.strip_suffix(k))
                            .is_some_and(|r| r.ends_with("__"))
                            || f.strip_prefix("__dispatch_") == Some(k)
                    };
                    for (key, hs) in self.holders {
                        if static_method(key) {
                            self.calls.push((hs.clone(), 1, args.clone(), encl_owned()));
                        }
                    }
                }
                _ => {}
            },
            Expr::Member { name, .. } if self.holders.contains_key(name) => {
                self.escaped.insert(name.clone());
            }
            Expr::Closure { fn_name, captures } => {
                let entry = (encl_owned(), Some(captures.clone()));
                self.parents.insert(fn_name.clone(), entry);
            }
            _ => {}
        }
        visit_expr_children(ast, e, &mut |c| self.expr(c, encl));
    }
}
