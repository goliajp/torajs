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
    scopes: Vec<HashMap<String, Binding>>,
    out: HashSet<ExprId>,
}

impl Walker<'_> {
    fn register_let(&mut self, name: &str, mutable: bool, unannotated: bool, init: ExprId) {
        let binding = if mutable
            && unannotated
            && let Some(family) = classify(self.ast, init)
        {
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
        let Some(rhs) = classify(self.ast, value) else {
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
