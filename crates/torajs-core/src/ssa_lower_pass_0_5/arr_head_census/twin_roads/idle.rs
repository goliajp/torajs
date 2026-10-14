//! Which twin bodies never run at all.
//!
//! [`super::unreachable_twin_exprs`] answers for the reads a twin makes:
//! entered with its own class's receiver, a twin reads what the mono
//! reads. Its writes are another matter. The boxed adapter also hands
//! a twin a call whose arguments its typed face cannot take
//! (`ssa_lower_boxed_entry::build_reroute`), and then the twin stores
//! values the mono's types rule out, onto a receiver of the class
//! itself. So a twin's writes stay out of a census only when no road
//! hands it a foreign receiver and no dynamic call hands it such
//! arguments. A typed call binds the mono statically; a dynamic one is
//! a call through a receiver no class type names, which may pass
//! anything, or one of the iterator-protocol helpers' `next()` /
//! `return()`, which pass nothing and call a method only on the
//! iterator they read it from (so they are no road). A mono whose
//! parameters take every argument, or every missing one, sends no such
//! call to its twin, unless it answers for one argument count alone.

use std::collections::{HashMap, HashSet};

use super::ExprMask;
use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::Type;

/// What the verdict reads off the program alone, gathered once per AST
/// and asked again under each run's types ([`TwinBodies::idle`]).
pub(crate) struct TwinBodies<'a> {
    ast: &'a Ast,
    /// The body each expression sits in: [`NO_BODY`], [`PROTOCOL`] (an
    /// iterator-protocol helper's), or `TWIN + k` for the `k`-th twin
    /// of `twins`.
    owner: Vec<u32>,
    twins: Vec<&'a str>,
    /// `None` when no twin is idle whatever the types: there is none,
    /// one cannot be read back off its name, or a road no type rules
    /// out leads to them.
    roads: Option<Roads<'a>>,
}

const NO_BODY: u32 = 0;
const PROTOCOL: u32 = 1;
const TWIN: u32 = 2;

/// The part of the verdict the types decide.
struct Roads<'a> {
    twinned: Vec<(&'a str, String)>,
    /// The expressions outside every twin and helper whose part the
    /// types decide: keyed reads, and calls.
    typed: Vec<ExprId>,
    monos: HashMap<&'a str, &'a [Param]>,
    /// Some helper has a body: its `next()` / `return()` calls pass no
    /// argument.
    protocol: bool,
}

impl<'a> TwinBodies<'a> {
    pub(crate) fn of(ast: &'a Ast) -> Self {
        let mut out = TwinBodies {
            ast,
            owner: Vec::new(),
            twins: Vec::new(),
            roads: None,
        };
        let Some(twinned) = super::twinned(ast).filter(|t| !t.is_empty()) else {
            return out;
        };
        out.twins = ast.cmany_twins.values().map(String::as_str).collect();
        let index: HashMap<&str, u32> = (out.twins.iter().enumerate())
            .map(|(k, t)| (*t, TWIN + k as u32))
            .collect();
        let helpers = crate::ast::desugar_generators_forof_helpers::HELPERS;
        let label = |n: &str| match index.get(n) {
            Some(k) => *k,
            None if helpers.contains(&n) => PROTOCOL,
            None => NO_BODY,
        };
        out.owner = vec![NO_BODY; ast.exprs.len()];
        for s in &ast.stmts {
            mark_bodies(ast, s, &label, &mut out.owner);
        }
        let methods: HashSet<String> = twinned.iter().map(|(_, m)| m.clone()).collect();
        let callees = super::callees(ast);
        let mut typed = Vec::new();
        for (i, e) in ast.exprs.iter().enumerate() {
            let id = ExprId(i as u32);
            if out.owner[i] != NO_BODY {
                continue;
            }
            if super::untyped_road(e, id, (&methods, &callees)) {
                return out;
            }
            if matches!(
                e,
                Expr::Index { .. }
                    | Expr::OptIndex { .. }
                    | Expr::Call { .. }
                    | Expr::OptCall { .. }
            ) {
                typed.push(id);
            }
        }
        out.roads = Some(Roads {
            twinned,
            typed,
            monos: mono_params(ast),
            protocol: out.owner.contains(&PROTOCOL),
        });
        out
    }

    /// Whether `e` sits in some twin's body.
    pub(crate) fn in_twin(&self, e: ExprId) -> bool {
        self.owner.get(e.0 as usize).is_some_and(|o| *o >= TWIN)
    }

    /// The expressions of every twin body that never runs, under the
    /// run's `expr_types`.
    pub(crate) fn idle(&self, expr_types: &HashMap<ExprId, Type>) -> ExprMask {
        let (ast, Some(roads)) = (self.ast, &self.roads) else {
            return ExprMask::default();
        };
        // the names a dynamic call may invoke with arguments, and with none
        let mut any_args: HashSet<&str> = HashSet::new();
        let mut no_args: HashSet<&str> = HashSet::new();
        if roads.protocol {
            no_args.extend(["next", "return"]);
        }
        for &id in &roads.typed {
            let e = ast.get_expr(id);
            if super::typed_road(e, id, expr_types, true) {
                return ExprMask::default();
            }
            let (Expr::Call { callee, args } | Expr::OptCall { callee, args }) = e else {
                continue;
            };
            if let Expr::Member { obj, name } | Expr::OptChain { obj, name } = ast.get_expr(*callee)
                && !class_receiver(expr_types.get(obj))
            {
                match args.is_empty() {
                    true => no_args.insert(name.as_str()),
                    false => any_args.insert(name.as_str()),
                };
            }
        }
        // by the twin's index in `twins`
        let mut idle = vec![false; self.twins.len()];
        for (mono, method) in &roads.twinned {
            let params = roads.monos.get(mono).copied();
            let fixed = ast.cm_fixed_argc.contains_key(*mono);
            let refuses =
                |fits: fn(&Param) -> bool| fixed || params.is_none_or(|ps| !ps.iter().all(fits));
            let runs = (any_args.contains(method.as_str()) && refuses(takes_any))
                || (no_args.contains(method.as_str()) && refuses(takes_none));
            if runs {
                continue;
            }
            if let Some(twin) = ast.cmany_twins.get(*mono) {
                for (k, t) in self.twins.iter().enumerate() {
                    idle[k] |= t == twin;
                }
            }
        }
        let mut out = ExprMask::new(ast);
        for (i, o) in self.owner.iter().enumerate() {
            if *o >= TWIN && idle[(*o - TWIN) as usize] {
                out.insert(ExprId(i as u32));
            }
        }
        out
    }
}

/// Label every expression of the bodies `label` names (a twin's or a
/// helper's), as `super::collect_twin_bodies` gathers them.
fn mark_bodies(ast: &Ast, s: &Stmt, label: &dyn Fn(&str) -> u32, owner: &mut [u32]) {
    match s {
        Stmt::FnDecl {
            name, params, body, ..
        } => {
            let k = label(name);
            if k == NO_BODY {
                return;
            }
            let mut mark = |e: ExprId| mark_tree(ast, e, k, owner);
            for p in params {
                if let Some(d) = p.default {
                    mark(d);
                }
            }
            for st in body {
                visit_stmt_exprs(st, &mut mark);
            }
        }
        Stmt::Multi(v) | Stmt::Block(v) => {
            for st in v {
                mark_bodies(ast, st, label, owner);
            }
        }
        _ => {}
    }
}

fn mark_tree(ast: &Ast, e: ExprId, k: u32, owner: &mut [u32]) {
    if owner[e.0 as usize] == NO_BODY {
        owner[e.0 as usize] = k;
        visit_expr_children(ast, e, &mut |c| mark_tree(ast, c, k, owner));
    }
}

/// A receiver whose method calls the lowering binds statically.
fn class_receiver(t: Option<&Type>) -> bool {
    match t {
        Some(Type::ClassRef(_) | Type::Struct(_)) => true,
        Some(Type::Nullable(t)) => class_receiver(Some(t)),
        _ => false,
    }
}

/// A head parameter, or one whose slot binds every argument as it is.
fn takes_any(p: &Param) -> bool {
    matches!(p.name.as_str(), "__this" | "__new_target")
        || matches!(p.type_ann.as_deref(), Some("any" | "unknown" | "any[]"))
}

/// A parameter a missing argument leaves holding a value of its type:
/// a default fills it, or the type admits `undefined`.
fn takes_none(p: &Param) -> bool {
    takes_any(p)
        || p.default.is_some()
        || p.is_rest
        || (p.type_ann.as_deref()).is_some_and(|a| a.starts_with("__nullable(") || a == "undefined")
}

/// The parameters of every mono method with a twin, by name.
fn mono_params(ast: &Ast) -> HashMap<&str, &[Param]> {
    let mut out = HashMap::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::FnDecl { name, params, .. } if ast.cmany_twins.contains_key(name) => {
                out.insert(name.as_str(), params.as_slice());
            }
            Stmt::Multi(v) | Stmt::Block(v) => stack.extend(v),
            _ => {}
        }
    }
    out
}
