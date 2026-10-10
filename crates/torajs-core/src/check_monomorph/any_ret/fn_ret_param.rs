//! A function of the lane handed as a value to a fn-typed parameter
//! whose own return is fn-typed (`apply(pick, 1)` against `apply(f: (k:
//! number) => (n: number) => number, k)`).
//!
//! The parameter's signature says a call through it returns a bare
//! function, while `pick` now hands back a fn object: `f(k)(5)` jumped
//! into the object's header (EXIT 138). The return of such a parameter
//! takes the closure repr too, and so does the same parameter of the
//! declaration's forwarder shells. Every other top-level function
//! handed to that parameter joins the lane, so the parameter sees one
//! repr whichever function it holds; the caller runs the lane's
//! fixpoint again over them. A marked parameter relayed by name into
//! another such parameter marks that one as well.

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Every declaration by name: its parameter names and annotations.
/// Nested declarations are lifted to the top under unique names before
/// the check, so a name is one declaration.
type Params<'a> = HashMap<&'a str, Vec<(&'a str, &'a Option<String>)>>;

fn all_params(ast: &Ast) -> Params<'_> {
    let mut out = HashMap::new();
    for s in crate::ast::toplevel_stmts_flat(ast) {
        if let Stmt::FnDecl { name, params, .. } = s {
            out.insert(
                name.as_str(),
                params
                    .iter()
                    .map(|p| (p.name.as_str(), &p.type_ann))
                    .collect(),
            );
        }
    }
    out
}

/// The bindings a body declares, not counting those of a function
/// nested in it: the initializer of an immutable one, `None` for one
/// whose value the syntax does not give.
type Locals<'a> = HashMap<&'a str, Option<ExprId>>;

fn locals(body: &[Stmt]) -> Locals<'_> {
    let mut out = HashMap::new();
    let mut stack: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::FnDecl { .. } => continue,
            Stmt::LetDecl {
                mutable,
                name,
                init,
                ..
            } => {
                out.insert(name.as_str(), (!mutable).then_some(*init));
            }
            Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                out.insert(var_name.as_str(), None);
            }
            Stmt::Try {
                catch_param: Some(p),
                ..
            } => {
                out.insert(p.as_str(), None);
            }
            _ => {}
        }
        crate::ast_closure_param_tag::push_child_stmts(s, &mut stack);
    }
    out
}

/// A value handed to a parameter, read in the scope it is written in.
enum Val<'a> {
    /// A parameter of a declaration, by index.
    Slot(&'a str, usize),
    /// A module-scope name: a declaration, or a closure's lifted name.
    Global(&'a str),
}

type Call<'a> = (Option<&'a str>, ExprId, &'a [ExprId]);
/// Each lifted closure: the declaration it is created in and the names
/// it captures.
type Parents<'a> = HashMap<&'a str, (Option<&'a str>, &'a [String])>;

/// Names by scope: each declaration's parameters and locals, the
/// module's own bindings, and where each closure reads its captures.
struct Scopes<'a> {
    ast: &'a Ast,
    params: Params<'a>,
    locals: HashMap<&'a str, Locals<'a>>,
    module: Locals<'a>,
    parents: Parents<'a>,
}

impl<'a> Scopes<'a> {
    fn new(ast: &'a Ast, parents: Parents<'a>) -> Self {
        let mut decl_locals = HashMap::new();
        for s in crate::ast::toplevel_stmts_flat(ast) {
            if let Stmt::FnDecl { name, body, .. } = s {
                decl_locals.insert(name.as_str(), locals(body));
            }
        }
        Scopes {
            ast,
            params: all_params(ast),
            locals: decl_locals,
            module: locals(&ast.stmts),
            parents,
        }
    }

    /// The values `e`, written in `encl` (`None` at the module scope),
    /// may answer. A binding is followed through its immutable
    /// initializer; one whose value the syntax does not give answers
    /// nothing, as does any other expression.
    fn values(&self, encl: Option<&'a str>, e: ExprId, depth: usize, out: &mut Vec<Val<'a>>) {
        if depth > 8 {
            return;
        }
        for leaf in crate::ast_closure_param_tag_member::passthrough_leaves(self.ast, e) {
            match self.ast.get_expr(leaf) {
                Expr::Ident(n) => self.name_values(encl, n, depth, out),
                Expr::Closure { fn_name, .. } => out.push(Val::Global(fn_name)),
                _ => {}
            }
        }
    }

    /// What `n` names in `encl`: its parameter, its local, or, in a
    /// lifted closure capturing `n`, what `n` names where the closure
    /// is created; the module's binding otherwise.
    fn name_values(
        &self,
        mut encl: Option<&'a str>,
        n: &'a str,
        depth: usize,
        out: &mut Vec<Val<'a>>,
    ) {
        while let Some(d) = encl {
            let own = self.params.get(d).into_iter().flatten();
            if let Some(i) = own.into_iter().position(|(p, _)| *p == n) {
                out.push(Val::Slot(d, i));
                return;
            }
            if let Some(bound) = self.locals.get(d).and_then(|l| l.get(n)) {
                if let Some(init) = bound {
                    self.values(encl, *init, depth + 1, out);
                }
                return;
            }
            match self.parents.get(d) {
                Some(&(parent, caps)) if caps.iter().any(|c| c == n) => encl = parent,
                _ => break,
            }
        }
        match self.module.get(n) {
            Some(Some(init)) => self.values(None, *init, depth + 1, out),
            Some(None) => {}
            None => out.push(Val::Global(n)),
        }
    }

    /// The declaration a call's callee names in `encl`, when it names
    /// exactly one.
    fn callee(&self, encl: Option<&'a str>, callee: ExprId) -> Option<&'a str> {
        let mut vals = Vec::new();
        self.values(encl, callee, 0, &mut vals);
        match vals.as_slice() {
            [Val::Global(f)] => Some(f),
            _ => None,
        }
    }
}

/// Every call in the program, with the declaration whose own body it
/// sits in (`None` at the top level): callee, arguments. And every
/// closure created, by lifted name.
fn calls_by_decl(ast: &Ast) -> (Vec<Call<'_>>, Parents<'_>) {
    fn walk<'a>(
        ast: &'a Ast,
        e: ExprId,
        encl: Option<&'a str>,
        out: &mut (Vec<Call<'a>>, Parents<'a>),
    ) {
        match ast.get_expr(e) {
            Expr::Call { callee, args } => out.0.push((encl, *callee, args.as_slice())),
            Expr::Closure { fn_name, captures } => {
                out.1.insert(fn_name.as_str(), (encl, captures.as_slice()));
            }
            _ => {}
        }
        visit_expr_children(ast, e, &mut |c| walk(ast, c, encl, out));
    }
    let mut out = (Vec::new(), HashMap::new());
    let mut stack: Vec<(&Stmt, Option<&str>)> = ast.stmts.iter().map(|s| (s, None)).collect();
    while let Some((s, encl)) = stack.pop() {
        if let Stmt::FnDecl { name, body, .. } = s {
            stack.extend(body.iter().map(|b| (b, Some(name.as_str()))));
            continue;
        }
        // `visit_stmt_exprs` reaches the nested statements but skips
        // the declarations among them, which open their own scope
        visit_stmt_exprs(s, &mut |e| walk(ast, e, encl, &mut out));
        let mut kids = Vec::new();
        crate::ast_closure_param_tag::push_child_stmts(s, &mut kids);
        while let Some(k) = kids.pop() {
            if matches!(k, Stmt::FnDecl { .. }) {
                stack.push((k, encl));
            } else {
                crate::ast_closure_param_tag::push_child_stmts(k, &mut kids);
            }
        }
    }
    out
}

/// The fn-typed parameters (declaration, index) that receive a function
/// of `lane`, directly or relayed through another such parameter, and
/// the functions handed to them that are not in `lane` yet. Callees and
/// arguments are read in the scope of the call, so a parameter or local
/// binding that shadows a declaration stands for what it holds.
pub(super) fn marked_params(
    ast: &Ast,
    lane: &HashSet<String>,
    fnsig_fns: &HashSet<String>,
) -> (HashSet<(String, usize)>, HashSet<String>) {
    let (calls, parents) = calls_by_decl(ast);
    let scopes = Scopes::new(ast, parents);
    let calls: Vec<(&str, Vec<Vec<Val>>)> = calls
        .into_iter()
        .filter_map(|(encl, callee, args)| {
            let f = scopes.callee(encl, callee)?;
            let vals = args
                .iter()
                .map(|&a| {
                    let mut vals = Vec::new();
                    scopes.values(encl, a, 0, &mut vals);
                    vals
                })
                .collect();
            Some((f, vals))
        })
        .collect();
    let mut marked: HashSet<(String, usize)> = HashSet::new();
    // a function handed to a closure-repr parameter is wrapped in its
    // forwarder cell, a shell of the lane when the function is
    let in_lane = |v: &Val, marked: &HashSet<(String, usize)>| match v {
        Val::Slot(d, i) => marked.contains(&(d.to_string(), *i)),
        Val::Global(n) => lane.contains(*n),
    };
    loop {
        let mut grew = false;
        for (callee, args) in &calls {
            let Some(ps) = scopes.params.get(callee) else {
                continue;
            };
            for (i, vals) in args.iter().enumerate() {
                let Some((_, ann)) = ps.get(i) else {
                    continue;
                };
                if crate::ast_collect_fn_closure_target::raw_ret::fnsig_ret(ann).is_some()
                    && !marked.contains(&(callee.to_string(), i))
                    && vals.iter().any(|v| in_lane(v, &marked))
                {
                    marked.insert((callee.to_string(), i));
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    let mut joins = HashSet::new();
    for (callee, args) in &calls {
        for (i, vals) in args.iter().enumerate() {
            if !marked.contains(&(callee.to_string(), i)) {
                continue;
            }
            for v in vals {
                let Val::Global(n) = v else {
                    continue;
                };
                let n = n.strip_prefix("__forward_").unwrap_or(n);
                if fnsig_fns.contains(n) && !lane.contains(n) {
                    joins.insert(n.to_string());
                }
            }
        }
    }
    (marked, joins)
}

/// The marked parameters by (declaration, parameter name).
pub(super) fn param_names(
    ast: &Ast,
    marked: &HashSet<(String, usize)>,
) -> HashSet<(String, String)> {
    let params = all_params(ast);
    marked
        .iter()
        .filter_map(|(d, i)| {
            let p = params.get(d.as_str())?.get(*i)?.0.to_string();
            Some((d.clone(), p))
        })
        .collect()
}

/// Retag the return of each marked parameter, and of the parameter of
/// the same name on the declaration's forwarder shells.
pub(super) fn retag_params(ast: &mut Ast, marked: &HashSet<(String, usize)>) {
    if marked.is_empty() {
        return;
    }
    let by_name: HashSet<(String, String)> = {
        param_names(ast, marked)
            .into_iter()
            .flat_map(|(d, p)| {
                let shells = crate::ast::shell_names(&d);
                std::iter::once((d, p.clone()))
                    .chain(shells.into_iter().map(move |s| (s, p.clone())))
            })
            .collect()
    };
    let mut stack: Vec<&mut Stmt> = ast.stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl { name, params, .. } = s {
            for p in params.iter_mut() {
                if by_name.contains(&(name.clone(), p.name.clone()))
                    && let Some(ann) = &mut p.type_ann
                {
                    retag_ret(ann);
                }
            }
        }
        crate::ast_closure_param_tag_retag::push_child_stmts_mut(s, &mut stack);
    }
}

/// `__fn(P)->(__fn(Q)->(R))` → `__fn(P)->(__cls(Q)->(R))`. A
/// `__nullable(` wrapper on the parameter is stripped here and put
/// back; one on the return is kept by `retag_to_cls`.
fn retag_ret(ann: &mut String) {
    let Some(ret) = crate::ast_collect_fn_closure_target::raw_ret::fnsig_ret(&Some(ann.clone()))
    else {
        return;
    };
    let tail = format!("->({ret})");
    let Some(head) = ann
        .strip_suffix(')')
        .filter(|a| a.starts_with("__nullable("))
        .unwrap_or(ann)
        .strip_suffix(tail.as_str())
        .map(str::to_string)
    else {
        return;
    };
    let mut new_ret = ret;
    crate::ast_closure_param_tag_retag::retag_to_cls(&mut new_ret);
    let close = if ann.starts_with("__nullable(") {
        ")"
    } else {
        ""
    };
    *ann = format!("{head}->({new_ret}){close}");
}
