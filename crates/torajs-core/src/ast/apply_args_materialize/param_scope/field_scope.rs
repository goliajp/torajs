//! The instance field initializers the parser spliced into a
//! constructor body, kept out of the constructor's scope.
//!
//! §15.7.10 ClassFieldDefinitionEvaluation makes each initializer a
//! method whose environment is the class scope: it sees what surrounds
//! the class, never the constructor's parameters or locals. Spliced
//! into the body, `v = k` beside `constructor(k)` read the argument,
//! and an arrow field writing an outer `lk` wrote a `const lk` the
//! constructor declared instead.
//!
//! So a constructor binding an initializer would otherwise reach takes
//! a name of its own (`k` becomes `k__fi0`): the parameter or
//! declaration, and every reference in the constructor that resolves
//! to it. The initializers keep their spelling. A parameter and a
//! `var` of one name are one binding, so both move together; a default
//! or a destructuring parameter's unpacking reads a parameter, never a
//! body declaration, so its references move only with a parameter.
//! A name declared by a body `class`, held in a name-keyed side table,
//! or read by a direct `eval` in the constructor is left alone.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::super::super::desugar_with::walk::{stmt_children, stmt_children_ref};
use super::super::super::hoist_gen_fn_exprs_self_write::ctor_field_inits;
use super::super::super::{Ast, ClassCtor, Expr, ExprId, StaticInit, Stmt};
use super::refs::Refs;
use super::walk::{decls, rename_decl};
use super::{DESTR_PARAM_PREFIX, Kind, pinned, reads_holder};

struct Plan {
    fields: Range<usize>,
    /// (name, fresh)
    renames: Vec<(String, String)>,
}

/// See module doc. `taken` holds every spelling in use.
pub(super) fn split_field_scope(ast: &mut Ast, taken: &mut HashSet<String>) {
    let mut sites: Vec<(ExprId, String)> = Vec::new();
    let mut plans: HashMap<usize, Plan> = HashMap::new();
    let mut ord = 0usize;
    {
        let ast_ref = &*ast;
        let mut visit = |c: &ClassCtor| {
            if let Some(p) = plan_ctor(ast_ref, c, taken, &mut sites) {
                plans.insert(ord, p);
            }
            ord += 1;
        };
        each_ctor(&ast_ref.stmts, &mut visit);
        for e in &ast_ref.exprs {
            if let Expr::ArrowFn { body, .. } = e {
                each_ctor(body, &mut visit);
            }
        }
    }
    if plans.is_empty() {
        return;
    }
    for p in plans.values() {
        for (name, fresh) in &p.renames {
            ast.renamed_binding_display
                .insert(fresh.clone(), name.clone());
        }
    }
    for (eid, fresh) in sites {
        match &mut ast.exprs[eid.0 as usize] {
            Expr::Ident(n) | Expr::New { class_name: n, .. } => *n = fresh,
            _ => {}
        }
    }
    let mut ord = 0usize;
    let mut apply = |c: &mut ClassCtor| {
        if let Some(p) = plans.remove(&ord) {
            apply_plan(p, c);
        }
        ord += 1;
    };
    let mut stmts = std::mem::take(&mut ast.stmts);
    each_ctor_mut(&mut stmts, &mut apply);
    for i in 0..ast.exprs.len() {
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[i] else {
            continue;
        };
        let mut b = std::mem::take(body);
        each_ctor_mut(&mut b, &mut apply);
        if let Expr::ArrowFn { body, .. } = &mut ast.exprs[i] {
            *body = b;
        }
    }
    ast.stmts = stmts;
}

fn plan_ctor(
    ast: &Ast,
    c: &ClassCtor,
    taken: &mut HashSet<String>,
    sites: &mut Vec<(ExprId, String)>,
) -> Option<Plan> {
    let fields = ctor_field_inits(ast, &c.body);
    if fields.is_empty() {
        return None;
    }
    let field_stmts = &c.body[fields.clone()];
    let destr = c
        .params
        .iter()
        .any(|p| p.name.starts_with(DESTR_PARAM_PREFIX));
    let mut temps: HashSet<&str> = HashSet::new();
    let mut unpack: Vec<&Stmt> = Vec::new();
    let mut rest: Vec<&Stmt> = Vec::new();
    for (i, s) in c.body.iter().enumerate() {
        if fields.contains(&i) {
            continue;
        }
        if destr && reads_holder(ast, s, &temps) {
            if let Stmt::LetDecl { name, .. } = s
                && name.starts_with("__")
            {
                temps.insert(name.as_str());
            }
            unpack.push(s);
        } else {
            rest.push(s);
        }
    }
    let mut param_side: Vec<(&str, Kind)> = c
        .params
        .iter()
        .map(|p| (p.name.as_str(), Kind::Var))
        .collect();
    for s in &unpack {
        decls(s, true, &mut param_side);
    }
    let mut body_side: Vec<(&str, Kind)> = Vec::new();
    for s in &rest {
        decls(s, true, &mut body_side);
    }
    let mut names: Vec<&str> = param_side
        .iter()
        .chain(body_side.iter())
        .map(|(n, _)| *n)
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut renames = Vec::new();
    for name in names {
        if name == "arguments"
            || name.starts_with("__")
            || body_side
                .iter()
                .any(|(n, k)| *n == name && *k == Kind::Class)
            || pinned(ast, name)
        {
            continue;
        }
        let mut field = Refs::new(ast, name);
        field.scope_stmts(field_stmts);
        if field.sites.is_empty() {
            continue;
        }
        let mut own = Refs::new(ast, name);
        if param_side.iter().any(|(n, _)| *n == name) {
            for d in c.params.iter().filter_map(|p| p.default) {
                own.expr(d);
            }
            own.scope_stmts(unpack.iter().copied());
        }
        own.scope_stmts(rest.iter().copied());
        if !own.ok {
            continue;
        }
        let fresh = (0usize..)
            .map(|k| format!("{name}__fi{k}"))
            .find(|f| !taken.contains(f))
            .expect("an unused spelling exists");
        taken.insert(fresh.clone());
        sites.extend(own.sites.into_iter().map(|e| (e, fresh.clone())));
        renames.push((name.to_string(), fresh));
    }
    (!renames.is_empty()).then_some(Plan { fields, renames })
}

fn apply_plan(plan: Plan, c: &mut ClassCtor) {
    for (name, fresh) in &plan.renames {
        for p in c.params.iter_mut().filter(|p| p.name == *name) {
            p.name = fresh.clone();
        }
        for (i, s) in c.body.iter_mut().enumerate() {
            if !plan.fields.contains(&i) {
                rename_decl(s, name, fresh, true);
            }
        }
    }
}

/// Every constructor in a statement tree, outside expressions, in the
/// order [`each_ctor_mut`] repeats.
fn each_ctor(stmts: &[Stmt], f: &mut dyn FnMut(&ClassCtor)) {
    for s in stmts {
        match s {
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                if let Some(c) = ctor {
                    f(c);
                    each_ctor(&c.body, f);
                }
                for m in methods.iter().chain(static_methods.iter()) {
                    each_ctor(&m.body, f);
                }
                for si in static_init {
                    if let StaticInit::Block(v) = si {
                        each_ctor(v, f);
                    }
                }
            }
            Stmt::FnDecl { body, .. } => each_ctor(body, f),
            Stmt::ExportDecl { inner: Some(i), .. } => each_ctor(std::slice::from_ref(i), f),
            _ => {
                for c in stmt_children_ref(s) {
                    each_ctor(std::slice::from_ref(c), f);
                }
            }
        }
    }
}

fn each_ctor_mut(stmts: &mut [Stmt], f: &mut dyn FnMut(&mut ClassCtor)) {
    for s in stmts {
        match s {
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                if let Some(c) = ctor {
                    f(c);
                    each_ctor_mut(&mut c.body, f);
                }
                for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                    each_ctor_mut(&mut m.body, f);
                }
                for si in static_init {
                    if let StaticInit::Block(v) = si {
                        each_ctor_mut(v, f);
                    }
                }
            }
            _ => {
                for c in stmt_children(s) {
                    each_ctor_mut(std::slice::from_mut(c), f);
                }
            }
        }
    }
}
