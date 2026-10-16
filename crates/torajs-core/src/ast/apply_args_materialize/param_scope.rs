//! The parameter environment of a function with parameter expressions.
//!
//! §10.2.11 FunctionDeclarationInstantiation gives such a function two
//! environments: the parameters and their initializers in one (steps
//! 19-26), the body's `var`s (step 28) and lexical declarations (steps
//! 30-34) in another created after the defaults have run. So in
//! `function f(y = K) { let K = 2 }` the default reads the `K` outside
//! `f`, and in `function f(a, g = () => a) { var a = 2 }` the closure
//! keeps the parameter while the body's `a` starts as a copy of it.
//!
//! tr runs a default either at the call site or as a guard at the head
//! of the body, and both see one scope. This pass gives each body
//! declaration a default could otherwise reach its own name, together
//! with every reference that resolves to it (`K` becomes `K__ps0`),
//! and seeds a renamed `var` that shadows a parameter from that
//! parameter. Run once, before any pass moves a default, so every later
//! lane sees distinct names.
//!
//! Parameter-side code is the defaults, the statements a destructuring
//! parameter unpacks into at the head of the body, and a constructor's
//! field initializers (those read the class's surroundings). A name
//! declared by a body `class`, or held in a name-keyed side table, is
//! left alone. A constructor binding the field initializers would
//! reach moves first, on its own (`field_scope`).

mod field_scope;
mod refs;
mod walk;

use std::collections::{HashMap, HashSet};

use super::super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::super::{Ast, Expr, ExprId, Param, Stmt};
use refs::Refs;
use walk::{all_decl_names, decls, for_each_fn, for_each_fn_mut, rename_decl};

const DESTR_PARAM_PREFIX: &str = "__param_destr_";

/// One body binding moved out of the defaults' way.
struct Split {
    name: String,
    fresh: String,
    /// A `var` shadowing a parameter starts as the parameter's value.
    copy: bool,
}

struct Plan {
    side: Vec<bool>,
    splits: Vec<Split>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Var,
    Lex,
    Fn,
    Class,
}

/// See module doc.
pub fn split_param_scope(ast: &mut Ast) {
    let mut taken: HashSet<String> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Ident(n) => Some(n.clone()),
            _ => None,
        })
        .collect();
    collect_taken(ast, &mut taken);
    field_scope::split_field_scope(ast, &mut taken);
    let mut planner = Planner {
        ast: &*ast,
        roots: ast.class_field_init_roots.iter().copied().collect(),
        taken: &mut taken,
        sites: Vec::new(),
        by_ord: HashMap::new(),
        by_arrow: HashMap::new(),
    };
    let mut ord = 0usize;
    for_each_fn(&ast.stmts, &mut |p, b, ctor| {
        planner.plan_ord(&mut ord, p, b, ctor)
    });
    for (i, e) in ast.exprs.iter().enumerate() {
        if let Expr::ArrowFn { params, body, .. } = e {
            if let Some(plan) = planner.plan_fn(params, body, false) {
                planner.by_arrow.insert(i, plan);
            }
            for_each_fn(body, &mut |p, b, ctor| {
                planner.plan_ord(&mut ord, p, b, ctor)
            });
        }
    }
    let Planner {
        sites,
        mut by_ord,
        mut by_arrow,
        ..
    } = planner;
    if by_ord.is_empty() && by_arrow.is_empty() {
        return;
    }
    for (eid, fresh) in sites {
        match &mut ast.exprs[eid.0 as usize] {
            Expr::Ident(n) | Expr::New { class_name: n, .. } => *n = fresh,
            _ => {}
        }
    }
    let mut ord = 0usize;
    let mut stmts = std::mem::take(&mut ast.stmts);
    for_each_fn_mut(&mut stmts, &mut |_, b| {
        apply(ast, by_ord.remove(&ord_next(&mut ord)), b)
    });
    ast.stmts = stmts;
    let arrows = ast.exprs.len();
    for i in 0..arrows {
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[i] else {
            continue;
        };
        let mut b = std::mem::take(body);
        apply(ast, by_arrow.remove(&i), &mut b);
        for_each_fn_mut(&mut b, &mut |_, fb| {
            apply(ast, by_ord.remove(&ord_next(&mut ord)), fb)
        });
        if let Expr::ArrowFn { body, .. } = &mut ast.exprs[i] {
            *body = b;
        }
    }
}

fn ord_next(ord: &mut usize) -> usize {
    *ord += 1;
    *ord - 1
}

struct Planner<'a> {
    ast: &'a Ast,
    roots: HashSet<ExprId>,
    taken: &'a mut HashSet<String>,
    sites: Vec<(ExprId, String)>,
    by_ord: HashMap<usize, Plan>,
    by_arrow: HashMap<usize, Plan>,
}

impl Planner<'_> {
    fn plan_ord(&mut self, ord: &mut usize, params: &[Param], body: &[Stmt], ctor: bool) {
        let k = ord_next(ord);
        if let Some(plan) = self.plan_fn(params, body, ctor) {
            self.by_ord.insert(k, plan);
        }
    }

    fn plan_fn(&mut self, params: &[Param], body: &[Stmt], ctor: bool) -> Option<Plan> {
        let ast = self.ast;
        let destr = params
            .iter()
            .any(|p| p.name.starts_with(DESTR_PARAM_PREFIX));
        let has_exprs = destr
            || params
                .iter()
                .any(|p| p.default.is_some_and(|d| !super::is_absent_default(ast, d)));
        if !has_exprs {
            return None;
        }
        let side = self.param_side(body, ctor, destr);
        let mut bound: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
        for (s, _) in body.iter().zip(&side).filter(|(_, s)| **s) {
            let mut out = Vec::new();
            decls(s, true, &mut out);
            bound.extend(out.into_iter().map(|(n, _)| n));
        }
        let mut kinds: Vec<(&str, Kind)> = Vec::new();
        for (s, _) in body.iter().zip(&side).filter(|(_, s)| !**s) {
            decls(s, true, &mut kinds);
        }
        let mut names: Vec<&str> = kinds.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        let mut splits = Vec::new();
        for name in names {
            let has = |k: Kind| kinds.iter().any(|(n, kk)| *n == name && *kk == k);
            if name == "arguments"
                || name.starts_with("__")
                || has(Kind::Class)
                || pinned(ast, name)
            {
                continue;
            }
            let mut outer = Refs::new(ast, name);
            for p in params {
                if let Some(d) = p.default {
                    outer.expr(d);
                }
            }
            outer.scope_stmts(body.iter().zip(&side).filter(|(_, s)| **s).map(|(s, _)| s));
            let is_param = bound.contains(name);
            let needed = !outer.sites.is_empty()
                && (!is_param || has(Kind::Fn) || (outer.in_closure && has(Kind::Var)));
            if !needed || !outer.ok {
                continue;
            }
            let mut inner = Refs::new(ast, name);
            inner.scope_stmts(body.iter().zip(&side).filter(|(_, s)| !**s).map(|(s, _)| s));
            if !inner.ok {
                continue;
            }
            let fresh = (0usize..)
                .map(|k| format!("{name}__ps{k}"))
                .find(|c| !self.taken.contains(c))
                .expect("an unused spelling exists");
            self.taken.insert(fresh.clone());
            self.sites
                .extend(inner.sites.into_iter().map(|e| (e, fresh.clone())));
            splits.push(Split {
                name: name.to_string(),
                fresh,
                copy: is_param && !has(Kind::Fn),
            });
        }
        (!splits.is_empty()).then_some(Plan { side, splits })
    }

    /// Which top-level body statements are parameter-side code: a
    /// constructor's field initializers, and the unpacking of a
    /// destructuring parameter (the statements that read its holder or
    /// a `__` temp an earlier unpacking statement declared).
    fn param_side(&self, body: &[Stmt], ctor: bool, destr: bool) -> Vec<bool> {
        let ast = self.ast;
        let mut temps: HashSet<&str> = HashSet::new();
        let mut side = Vec::with_capacity(body.len());
        for s in body {
            let field = ctor
                && matches!(s, Stmt::Expr(e)
                    if matches!(ast.get_expr(*e), Expr::Assign { value, .. } if self.roots.contains(value)));
            let unpack = destr && reads_holder(ast, s, &temps);
            if unpack
                && let Stmt::LetDecl { name, .. } = s
                && name.starts_with("__")
            {
                temps.insert(name.as_str());
            }
            side.push(field || unpack);
        }
        side
    }
}

/// Every spelling a declaration already uses, so a minted name
/// collides with none.
fn collect_taken(ast: &Ast, taken: &mut HashSet<String>) {
    let mut out = Vec::new();
    for s in &ast.stmts {
        all_decl_names(s, &mut out);
    }
    for e in &ast.exprs {
        if let Expr::ArrowFn { params, body, .. } = e {
            out.extend(params.iter().map(|p| p.name.as_str()));
            for s in body {
                all_decl_names(s, &mut out);
            }
        }
    }
    taken.extend(out.into_iter().map(str::to_string));
}

/// A name some side table recorded by spelling before this pass.
fn pinned(ast: &Ast, name: &str) -> bool {
    ast.obj_rest_names.contains(name)
        || ast.strict_eval_assign_names.contains(name)
        || ast.sloppy_eval_fn_names.contains(name)
}

fn reads_holder(ast: &Ast, s: &Stmt, temps: &HashSet<&str>) -> bool {
    fn expr_reads(ast: &Ast, e: ExprId, temps: &HashSet<&str>) -> bool {
        match ast.get_expr(e) {
            Expr::Ident(n) => n.starts_with(DESTR_PARAM_PREFIX) || temps.contains(n.as_str()),
            Expr::ArrowFn { .. } => false,
            _ => expr_children(ast, e)
                .into_iter()
                .any(|c| expr_reads(ast, c, temps)),
        }
    }
    stmt_exprs(s).into_iter().any(|e| expr_reads(ast, e, temps))
        || stmt_children_ref(s)
            .into_iter()
            .any(|c| reads_holder(ast, c, temps))
}

/// Rename the planned declarations in the body proper, then seed each
/// renamed `var` that shadows a parameter right after the
/// parameter-side statements.
fn apply(ast: &mut Ast, plan: Option<Plan>, body: &mut Vec<Stmt>) {
    let Some(plan) = plan else {
        return;
    };
    let at = plan.side.iter().rposition(|s| *s).map_or(0, |i| i + 1);
    for sp in &plan.splits {
        for (s, _) in body.iter_mut().zip(&plan.side).filter(|(_, s)| !**s) {
            rename_decl(s, &sp.name, &sp.fresh, true);
        }
        ast.renamed_binding_display
            .insert(sp.fresh.clone(), sp.name.clone());
    }
    let seeds: Vec<Stmt> = plan
        .splits
        .iter()
        .filter(|sp| sp.copy)
        .map(|sp| Stmt::LetDecl {
            mutable: true,
            name: sp.fresh.clone(),
            type_ann: None,
            init: ast.add_expr(Expr::Ident(sp.name.clone())),
            is_var: true,
        })
        .collect();
    body.splice(at..at, seeds);
}

#[cfg(test)]
mod tests;
