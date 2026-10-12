//! Same-named function declarations of different kinds, renamed apart.
//!
//! Whether a declaration is `async` reaches the passes only through the
//! name-keyed sets `Ast::async_fns` / `Ast::async_generator_fns`, filled
//! while parsing. Two declarations sharing a name in different scopes
//! share one entry:
//!
//! ```text
//! function a1(k) { function inner() { return k } return inner }
//! function a2(k) { async function inner() { return k } return inner }
//! a1(1)()   // bun: 1, the shared entry made it a Promise
//! ```
//!
//! After the parse, every function-body declaration whose name is also
//! spelled by a declaration of another kind takes a fresh name, together
//! with the references that resolve to it, so each name in the sets
//! stands for one kind. The declaration that keeps the name is a
//! top-level one when there is one (a program-level binding is visible
//! to other modules and to `globalThis`), otherwise a synchronous one.
//! `.name` keeps the source spelling through `Ast::nested_fn_names` and
//! `Ast::renamed_binding_display`.
//!
//! The kind is read from the declaration's own source text: its span
//! starts at `async` exactly when it is an async declaration.

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_with::walk::{stmt_children, stmt_children_ref};
use crate::ast::{Ast, Expr, Param, StaticInit, Stmt};
use crate::lexer::{Span, Spanned, Token};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Plain,
    Async,
    AsyncGen,
}

/// Set membership from the parses that filled this arena before the
/// current one — their declarations are not walked again.
pub(super) struct PriorKinds {
    pub(super) async_fns: HashSet<String>,
    pub(super) async_gens: HashSet<String>,
}

/// Whether the declaration spanning `span` was written `async function`.
pub(super) fn decl_text_is_async(source: &str, span: Span) -> bool {
    source
        .get(span.start as usize..span.end as usize)
        .is_some_and(|t| t.starts_with("async"))
}

fn kind_of(source: &str, span: Span, is_generator: bool) -> Option<Kind> {
    let text = source.get(span.start as usize..span.end as usize)?;
    let is_async = text.starts_with("async");
    if !is_async && !text.starts_with("function") {
        return None;
    }
    Some(match (is_async, is_generator) {
        (false, _) => Kind::Plain,
        (true, false) => Kind::Async,
        (true, true) => Kind::AsyncGen,
    })
}

#[derive(Clone, Copy)]
enum ListAt<'p> {
    Top,
    FnBody(&'p [Param]),
    Inner,
}

trait ListVisitor {
    fn list(&mut self, ast: &mut Ast, list: &mut [Stmt], at: ListAt<'_>);
}

pub(super) fn split_fn_kind_names(
    ast: &mut Ast,
    source: &str,
    tokens: &[Spanned],
    stmt_offset: usize,
    id_offset: usize,
    prior: &PriorKinds,
) {
    let mut census = Census {
        source,
        decls: Vec::new(),
    };
    walk_parse(ast, stmt_offset, id_offset, &mut census);
    let plan = plan_renames(&census.decls, prior);
    if plan.is_empty() {
        return;
    }
    let mut taken: HashSet<String> = tokens
        .iter()
        .filter_map(|t| match &t.token {
            Token::Ident(n) => Some(n.clone()),
            _ => None,
        })
        .collect();
    taken.extend(ast.nested_fn_names.keys().cloned());
    let mut rename = Rename {
        source,
        plan: &plan,
        taken,
        seq: 0,
        renamed: Vec::new(),
    };
    walk_parse(ast, stmt_offset, id_offset, &mut rename);
    let renamed = rename.renamed;
    let moved: HashSet<(String, u32, u32)> = renamed
        .iter()
        .map(|r| (r.old.clone(), r.span.start, r.span.end))
        .collect();
    for r in &renamed {
        match r.kind {
            Kind::Async => {
                ast.async_fns.insert(r.new.clone());
            }
            Kind::AsyncGen => {
                ast.async_generator_fns.insert(r.new.clone());
            }
            Kind::Plain => {}
        }
        if r.is_generator
            && let Some(&n) = ast.gen_param_destr_prefix.get(&r.old)
        {
            ast.gen_param_destr_prefix.insert(r.new.clone(), n);
        }
        if let Some(&n) =
            ast.gen_param_destr_prefix_at
                .get(&(r.old.clone(), r.span.start, r.span.end))
        {
            ast.gen_param_destr_prefix_at
                .insert((r.new.clone(), r.span.start, r.span.end), n);
        }
        ast.nested_fn_names.insert(r.new.clone(), r.old.clone());
        ast.renamed_binding_display
            .insert(r.new.clone(), r.old.clone());
    }
    // A name keeps its set entry only while a declaration of that kind
    // still answers to it.
    for name in plan.keys() {
        let left = |k: Kind| {
            census.decls.iter().any(|d| {
                d.name == *name
                    && d.kind == Some(k)
                    && !moved.contains(&(d.name.clone(), d.span.start, d.span.end))
            })
        };
        if !left(Kind::Async) && !prior.async_fns.contains(name) {
            ast.async_fns.remove(name);
        }
        if !left(Kind::AsyncGen) && !prior.async_gens.contains(name) {
            ast.async_generator_fns.remove(name);
        }
    }
}

/// Every statement list of this parse: the program's own, then each
/// function expression or arrow body in the arena slice it minted (a
/// body hanging off an expression is not reachable as a statement).
fn walk_parse(ast: &mut Ast, stmt_offset: usize, id_offset: usize, v: &mut dyn ListVisitor) {
    let mut stmts = std::mem::take(&mut ast.stmts);
    walk_list(ast, &mut stmts[stmt_offset..], ListAt::Top, v);
    ast.stmts = stmts;
    for i in id_offset..ast.exprs.len() {
        let (params, mut body) = match &mut ast.exprs[i] {
            Expr::ArrowFn { params, body, .. } => (params.clone(), std::mem::take(body)),
            _ => continue,
        };
        walk_list(ast, &mut body, ListAt::FnBody(&params), v);
        if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[i] {
            *slot = body;
        }
    }
}

fn walk_list(ast: &mut Ast, list: &mut [Stmt], at: ListAt<'_>, v: &mut dyn ListVisitor) {
    v.list(ast, list, at);
    for s in list.iter_mut() {
        walk_stmt(ast, s, at, v);
    }
}

fn walk_stmt(ast: &mut Ast, s: &mut Stmt, at: ListAt<'_>, v: &mut dyn ListVisitor) {
    match s {
        Stmt::FnDecl { params, body, .. } => walk_list(ast, body, ListAt::FnBody(params), v),
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                walk_list(ast, &mut c.body, ListAt::FnBody(&c.params), v);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                walk_list(ast, &mut m.body, ListAt::FnBody(&m.params), v);
            }
            for i in static_init.iter_mut() {
                if let StaticInit::Block(b) = i {
                    walk_list(ast, b, ListAt::FnBody(&[]), v);
                }
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => walk_list(ast, std::slice::from_mut(inner.as_mut()), at, v),
        Stmt::Multi(items) => {
            for s2 in items.iter_mut() {
                walk_stmt(ast, s2, at, v);
            }
        }
        other => {
            for c in stmt_children(other) {
                walk_list(ast, std::slice::from_mut(c), ListAt::Inner, v);
            }
        }
    }
}

struct Decl {
    name: String,
    span: Span,
    kind: Option<Kind>,
    top: bool,
}

struct Census<'s> {
    source: &'s str,
    decls: Vec<Decl>,
}

impl ListVisitor for Census<'_> {
    fn list(&mut self, _ast: &mut Ast, list: &mut [Stmt], at: ListAt<'_>) {
        for s in list.iter() {
            let items: &[Stmt] = match s {
                Stmt::Multi(items) => items,
                one => std::slice::from_ref(one),
            };
            for s2 in items {
                if let Stmt::FnDecl {
                    name,
                    is_generator,
                    span,
                    ..
                } = s2
                {
                    self.decls.push(Decl {
                        name: name.clone(),
                        span: *span,
                        kind: kind_of(self.source, *span, *is_generator),
                        top: matches!(at, ListAt::Top),
                    });
                }
            }
        }
    }
}

/// Name → the kind that keeps it, for every name declared with more
/// than one kind. A name with a declaration whose kind cannot be read
/// back (a synthesized one) is left alone.
fn plan_renames(decls: &[Decl], prior: &PriorKinds) -> HashMap<String, Kind> {
    let mut by_name: HashMap<&str, Vec<&Decl>> = HashMap::new();
    for d in decls {
        by_name.entry(d.name.as_str()).or_default().push(d);
    }
    let mut plan = HashMap::new();
    for (name, ds) in by_name {
        let Some(kinds) = ds.iter().map(|d| d.kind).collect::<Option<Vec<Kind>>>() else {
            continue;
        };
        let prior_kind = if prior.async_fns.contains(name) {
            Some(Kind::Async)
        } else if prior.async_gens.contains(name) {
            Some(Kind::AsyncGen)
        } else {
            None
        };
        let mut all: Vec<Kind> = kinds.clone();
        all.extend(prior_kind);
        if all.iter().all(|k| *k == all[0]) {
            continue;
        }
        let top = ds.iter().rev().find(|d| d.top).and_then(|d| d.kind);
        let kept = top
            .or(prior_kind)
            .or_else(|| kinds.contains(&Kind::Plain).then_some(Kind::Plain))
            .unwrap_or(kinds[0]);
        plan.insert(name.to_string(), kept);
    }
    plan
}

struct Renamed {
    old: String,
    new: String,
    span: Span,
    kind: Kind,
    is_generator: bool,
}

struct Rename<'a> {
    source: &'a str,
    plan: &'a HashMap<String, Kind>,
    taken: HashSet<String>,
    seq: u32,
    renamed: Vec<Renamed>,
}

impl Rename<'_> {
    fn fresh(&mut self, name: &str) -> String {
        loop {
            self.seq += 1;
            let n = format!("{name}${}", self.seq);
            if self.taken.insert(n.clone()) {
                return n;
            }
        }
    }
}

impl ListVisitor for Rename<'_> {
    fn list(&mut self, ast: &mut Ast, list: &mut [Stmt], at: ListAt<'_>) {
        let ListAt::FnBody(params) = at else {
            return;
        };
        for i in 0..list.len() {
            let Stmt::FnDecl {
                name,
                is_generator,
                span,
                ..
            } = &list[i]
            else {
                continue;
            };
            let (old, is_generator, span) = (name.clone(), *is_generator, *span);
            let Some(&kept) = self.plan.get(&old) else {
                continue;
            };
            let Some(kind) = kind_of(self.source, span, is_generator) else {
                continue;
            };
            if kind == kept || !sole_binding(list, i, &old, params) {
                continue;
            }
            let new = self.fresh(&old);
            if let Stmt::FnDecl { name, .. } = &mut list[i] {
                *name = new.clone();
            }
            let sites: Vec<_> = list
                .iter()
                .flat_map(|s| crate::ast::top_ref_sites(ast, s, &old, &[]))
                .collect();
            for eid in sites {
                match &mut ast.exprs[eid.0 as usize] {
                    Expr::Ident(n) => *n = new.clone(),
                    Expr::New { class_name, .. } => *class_name = new.clone(),
                    _ => {}
                }
            }
            self.renamed.push(Renamed {
                old,
                new,
                span,
                kind,
                is_generator,
            });
        }
    }
}

/// The declaration at `list[idx]` is the only binding of `name` in the
/// function scope `list` opens: no parameter, no other declaration in
/// the list or in its blocks (which a `var` or an Annex B function
/// would hoist out of), no catch parameter or loop binding.
fn sole_binding(list: &[Stmt], idx: usize, name: &str, params: &[Param]) -> bool {
    if name == "arguments" || params.iter().any(|p| p.name == name) {
        return false;
    }
    list.iter()
        .enumerate()
        .all(|(i, s)| i == idx || !binds_within(s, name))
}

fn binds_within(s: &Stmt, name: &str) -> bool {
    let own = match s {
        Stmt::FnDecl { name: n, .. }
        | Stmt::LetDecl { name: n, .. }
        | Stmt::UsingDecl { name: n, .. }
        | Stmt::ClassDecl { name: n, .. }
        | Stmt::ForOf { var_name: n, .. } => n == name,
        Stmt::Try {
            catch_param: Some(p),
            ..
        } => p == name,
        _ => false,
    };
    own || stmt_children_ref(s)
        .into_iter()
        .any(|c| binds_within(c, name))
}
