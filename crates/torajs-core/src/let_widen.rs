//! Mutable-let cross-type reassignment widen — scope-correct
//! pre-pass (RFC 20260804-mutable-let-widen).
//!
//! test262 is JS: `let it = new C(); it = Iterator.from(it);` is
//! legal, but tr types an unannotated `let` from its init and the
//! checker rejects the cross-type reassign (and every later member
//! read resolves against the stale type). The lowering side already
//! handles a binding that is `any` FROM DECLARATION (probe wd1 —
//! the Any-let slot machinery), so the whole fix is typing such
//! bindings `any` up front.
//!
//! Same architecture as [`crate::dynobj_degrade`] (the third set of
//! its family, after `dynobj_degraded` and `any_promoted_inits`):
//! an AST-level walk keyed by the DECLARATION site (LetDecl init
//! ExprId), consumed by both `check_stmt_let_decl` and
//! `ssa_lower_stmt_let_decl` so the two homes cannot drift.
//!
//! Widening costs Any-lane precision, so the trigger is a
//! conservative SYNTACTIC family judgment: widen only when the init
//! and some assign rhs are both classifiable and their families
//! differ. Anything unclassifiable (a plain call, a BinOp, an
//! ident) never widens — `let i = 0; i = i + 1` stays on the typed
//! lane, and a shape the judgment misses keeps today's loud checker
//! reject (never silent wrong). Recorded boundary: an assign inside
//! a nested fn body resolves against that fn's own scope only
//! (closure-captured rebinds of an outer `let` keep the loud
//! reject; extend to a name-keyed fallback if a cluster surfaces).

mod walk;

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId};

/// Syntactic family of an init / assign-rhs expression. `None` =
/// unclassifiable (never participates in widening).
#[derive(PartialEq, Eq, Clone)]
enum Family {
    New(String),
    Num,
    Str,
    Bool,
    Null,
    Arr,
    Obj,
    /// `Ns.method(...)` where `Ns` is a known global namespace
    /// (`Iterator.from`, `Array.of`, `Object.create`, ...): the
    /// return type is namespace-determined and virtually never the
    /// init's class.
    NsCall(String, String),
}

fn classify(ast: &Ast, e: ExprId) -> Option<Family> {
    match ast.get_expr(strip_as(ast, e)) {
        Expr::New { class_name, .. } => Some(Family::New(class_name.clone())),
        Expr::Number(_) => Some(Family::Num),
        Expr::String(_) => Some(Family::Str),
        Expr::Bool(_) => Some(Family::Bool),
        Expr::Null => Some(Family::Null),
        Expr::Array(_) => Some(Family::Arr),
        Expr::ObjectLit { .. } => Some(Family::Obj),
        Expr::Call { callee, .. } => match ast.get_expr(*callee) {
            Expr::Member { obj, name } => match ast.get_expr(*obj) {
                Expr::Ident(ns) if crate::ast::free_vars_is_global_name(ns) => {
                    Some(Family::NsCall(ns.clone(), name.clone()))
                }
                _ => None,
            },
            // The pre-pass runs on the post-desugar AST, where
            // desugar_classes has rewritten `new C(...)` into a
            // `__new_C(...)` factory call — the dominant shape.
            Expr::Ident(n) if n.starts_with("__new_") => Some(Family::New(n.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// TS casts are static assertions — classify the value underneath.
fn strip_as(ast: &Ast, mut e: ExprId) -> ExprId {
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    e
}

enum Binding {
    /// A widen candidate: unannotated mutable `let` with a
    /// classifiable init.
    Tracked { init: ExprId, family: Family },
    /// Shadows without tracking (const, annotated, unclassifiable
    /// init, params, catch params, imports).
    Opaque,
}

/// Collect the init ExprIds of every unannotated mutable `let`
/// whose binding is later reassigned a value of a DIFFERENT
/// syntactic family — such bindings type (and lower) as `any`.
pub(crate) fn collect_cross_type_widen_inits(ast: &Ast) -> HashSet<ExprId> {
    let mut w = Walker {
        ast,
        gen_factories: collect_gen_factories(ast),
        scopes: vec![HashMap::new()],
        out: HashSet::new(),
    };
    for s in &ast.stmts {
        w.walk_stmt(s);
    }
    w.out
}

struct Walker<'a> {
    ast: &'a Ast,
    /// FnDecl names whose declared return type is a synthesized
    /// `__Gen_*` class — the post-desugar spelling of `function*`.
    /// `let it = g()` initializes a generator OBJECT, and the
    /// iterator-helper self-transform (`it = it.filter(cb)`) then
    /// assigns a helper cell into the same slot; without the widen
    /// the checker rejects the reassign (Obj slot, Any value).
    gen_factories: HashSet<String>,
    scopes: Vec<HashMap<String, Binding>>,
    out: HashSet<ExprId>,
}

/// `filter` / `map` / `flatMap` / `take` / `drop` — the §27.1.4
/// transforms that answer a NEW iterator-helper cell (the eager
/// consumers answer scalars, which classify on their own).
fn is_iter_transform(name: &str) -> bool {
    matches!(name, "filter" | "map" | "flatMap" | "take" | "drop")
}

fn collect_gen_factories(ast: &Ast) -> HashSet<String> {
    fn walk(stmts: &[crate::ast::Stmt], out: &mut HashSet<String>) {
        for s in stmts {
            match s {
                crate::ast::Stmt::FnDecl {
                    name,
                    return_type: Some(rt),
                    body,
                    ..
                } => {
                    if rt.starts_with("__Gen_") {
                        out.insert(name.clone());
                    }
                    walk(body, out);
                }
                crate::ast::Stmt::FnDecl { body, .. } => walk(body, out),
                crate::ast::Stmt::Block(inner) | crate::ast::Stmt::Multi(inner) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = HashSet::new();
    walk(&ast.stmts, &mut out);
    out
}

impl Walker<'_> {
    fn register_let(&mut self, name: &str, mutable: bool, unannotated: bool, init: ExprId) {
        let binding = if mutable
            && unannotated
            && let Some(family) = classify(self.ast, init).or_else(|| {
                // A generator-factory call initializes a `__Gen_*`
                // object — classifiable through the factory table.
                match self.ast.get_expr(strip_as(self.ast, init)) {
                    Expr::Call { callee, .. } => match self.ast.get_expr(*callee) {
                        Expr::Ident(n) if self.gen_factories.contains(n) => {
                            Some(Family::New(format!("__Gen_{n}")))
                        }
                        _ => None,
                    },
                    _ => None,
                }
            }) {
            Binding::Tracked { init, family }
        } else {
            Binding::Opaque
        };
        self.scopes
            .last_mut()
            .expect("scope stack never empty")
            .insert(name.to_string(), binding);
    }

    fn shadow(&mut self, name: &str) {
        self.scopes
            .last_mut()
            .expect("scope stack never empty")
            .insert(name.to_string(), Binding::Opaque);
    }

    /// `name = value` — widen the tracked declaration when the rhs
    /// classifies into a different family.
    fn scan_assign(&mut self, target: ExprId, value: ExprId) {
        let Expr::Ident(name) = self.ast.get_expr(target) else {
            return;
        };
        let Some(rhs) = classify(self.ast, value).or_else(|| {
            // The iterator-helper self-transform: `it = it.filter(cb)`
            // (receiver IS the assigned binding) answers a helper
            // cell, never the receiver's own class — classifiable by
            // shape alone, and kept narrow to the self-receiver so a
            // plain method call on some other object stays out.
            match self.ast.get_expr(strip_as(self.ast, value)) {
                Expr::Call { callee, .. } => match self.ast.get_expr(*callee) {
                    Expr::Member { obj, name: m }
                        if is_iter_transform(m)
                            && matches!(self.ast.get_expr(*obj), Expr::Ident(r) if r == name) =>
                    {
                        Some(Family::NsCall("__iter_helper".into(), m.clone()))
                    }
                    _ => None,
                },
                _ => None,
            }
        }) else {
            return;
        };
        for scope in self.scopes.iter().rev() {
            match scope.get(name) {
                Some(Binding::Tracked { init, family }) => {
                    if *family != rhs {
                        self.out.insert(*init);
                    }
                    return;
                }
                Some(Binding::Opaque) => return,
                None => {}
            }
        }
    }
}
