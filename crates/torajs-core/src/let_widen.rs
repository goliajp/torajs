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
//! a nested fn body resolves against that fn's own scope and the
//! module's top-level bindings only (closure-captured rebinds of an
//! outer function's `let` keep the loud reject; extend to a
//! name-keyed fallback if a cluster surfaces).
//!
//! Rotation 651 — that fallback exists for NULLISH inits. `let t =
//! null; let o = { m() { t = this } }` and `let u = undefined; u = 3`
//! are TS-legal (tsc, strict or not, types a mutable `let x = null` /
//! `= undefined` as `any`: the initializer carries no information),
//! and test262 writes the first shape for every "was `return()`
//! called with the right receiver" check. A nullish init has no type
//! to keep, so the question is not "which family is the rhs" but
//! "is the name ever rebound to something other than null /
//! undefined" — asked NAME-KEYED over the whole program, nested fn
//! bodies included, because a named fn body starts from a fresh stack.
//! A lifted closure body is also walked where the closure is made, with
//! its captured names bound as they are there, so `total = total + v`
//! inside a callback widens the outer `let total`. A same-named binding elsewhere can
//! over-widen this one; that costs typed-lane precision on a
//! binding whose init was `null`, never a wrong answer.
//!
//! RFC 20260927-inferred-slot-any-widen — the TYPE half. An assign
//! whose rhs the checker typed `Any` stores a value of unknown type
//! into the binding, and §13.15.2 stores it as is: `let total = 0;
//! total = total + anyv` holds `"0x"` for a string `anyv`. A typed
//! slot would have to convert it, so such a binding is `any` from
//! declaration too. The checker supplies its `expr_types` and re-runs
//! until this set stops growing (`check_entry::check_to_fixpoint`);
//! the lowerer asks with the checker's final table.

mod facts;
mod push;
mod spec;
mod walk;

pub(crate) use spec::{check_spec, check_spec_stmt};

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;

/// Syntactic family of an init / assign-rhs expression. `None` =
/// unclassifiable (never participates in widening).
#[derive(PartialEq, Eq, Clone)]
enum Family {
    New(String),
    Num,
    Str,
    Bool,
    Null,
    /// The `undefined` literal (spelled `Ident("undefined")` in the
    /// arena; nothing else in the pipeline treats it as a binding).
    Undef,
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
        Expr::Ident(n) if n == "undefined" => Some(Family::Undef),
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

#[derive(Clone)]
enum Binding {
    /// A widen candidate: unannotated mutable `let`. `family` is the
    /// init's syntactic family, `None` when it does not classify (the
    /// syntactic judgment then never widens it; the type half may).
    Tracked {
        init: ExprId,
        family: Option<Family>,
    },
    /// An annotated mutable `let`: an array literal holding an `any`
    /// element stored into it later keeps the dynamic layout when the
    /// annotation resolves to a struct array
    /// (`check_stmt_let_decl::any_layout_slot` decides on the resolved
    /// type; for any other annotation the mark changes nothing), and
    /// the binding has to hold it from its declaration.
    LayoutArr { init: ExprId },
    /// An unannotated `const`: never reassigned, but an array it holds
    /// may be pushed a value of any kind (`push`).
    Fixed { init: ExprId },
    /// Shadows without tracking (const, other annotated, params, catch
    /// params, imports).
    Opaque,
}

/// Collect the init ExprIds of every unannotated mutable `let`
/// whose binding is later reassigned a value of a DIFFERENT
/// syntactic family — such bindings type (and lower) as `any`.
/// `types`: the checker's expression types, which add the type half
/// (module doc); `None` is the syntactic judgment alone.
pub(crate) fn collect_cross_type_widen_inits(
    ast: &Ast,
    types: Option<&HashMap<ExprId, Type>>,
) -> HashSet<ExprId> {
    let mut w = Walker::new(ast, types);
    for s in &ast.stmts {
        w.walk_stmt(s);
    }
    w.out
}

/// The type half over one declaration `decl` the program does not hold
/// yet: a specialization the monomorphizer checks with its own types
/// (a parameter widened to `any` can widen the bindings it reaches).
pub(crate) fn collect_cross_type_widen_inits_in(
    ast: &Ast,
    decl: &crate::ast::Stmt,
    types: &HashMap<ExprId, Type>,
) -> HashSet<ExprId> {
    let mut w = Walker::new(ast, Some(types));
    w.walk_stmt(decl);
    w.out
}

struct Walker<'a> {
    ast: &'a Ast,
    types: Option<&'a HashMap<ExprId, Type>>,
    /// FnDecl names whose declared return type is a synthesized
    /// `__Gen_*` class — the post-desugar spelling of `function*`.
    /// `let it = g()` initializes a generator OBJECT, and the
    /// iterator-helper self-transform (`it = it.filter(cb)`) then
    /// assigns a helper cell into the same slot; without the widen
    /// the checker rejects the reassign (Obj slot, Any value).
    gen_factories: OnceCell<HashSet<String>>,
    /// Names rebound somewhere to a non-nullish rhs — the nullish
    /// rule's name-keyed evidence (module doc).
    non_nullish_rebinds: OnceCell<HashSet<String>>,
    /// Top-level FnDecls by name — where a closure's lifted body lives.
    lifted: OnceCell<facts::Lifted<'a>>,
    /// The lifted bodies of closures, which are walked where each
    /// closure is made, with the bindings it captures there.
    closure_bodies: OnceCell<HashSet<&'a str>>,
    /// The module's top-level bindings, which a named function body
    /// starts from.
    module: HashMap<String, Binding>,
    /// Lifted bodies being walked from their closure site (a closure
    /// made inside its own body is not re-entered).
    entered: HashSet<&'a str>,
    scopes: Vec<HashMap<String, Binding>>,
    out: HashSet<ExprId>,
}

/// `filter` / `map` / `flatMap` / `take` / `drop` — the §27.1.4
/// transforms that answer a NEW iterator-helper cell (the eager
/// consumers answer scalars, which classify on their own).
fn is_iter_transform(name: &str) -> bool {
    matches!(name, "filter" | "map" | "flatMap" | "take" | "drop")
}

impl Walker<'_> {
    fn register_let(&mut self, name: &str, mutable: bool, ann: Option<&str>, init: ExprId) {
        // a scalar annotation over a read of an `any` value holds the
        // box (`check_stmt_let_decl::any_scalar_slot`)
        if let (Some(ann), Some(types)) = (ann, self.types)
            && crate::check_stmt_let_decl::any_scalar_slot::takes_box(ann, init, self.ast, types)
        {
            self.out.insert(init);
        }
        let binding = if mutable && ann.is_some() {
            Binding::LayoutArr { init }
        } else if mutable && ann.is_none() {
            let family = classify(self.ast, init).or_else(|| {
                // A generator-factory call initializes a `__Gen_*`
                // object — classifiable through the factory table.
                match self.ast.get_expr(strip_as(self.ast, init)) {
                    Expr::Call { callee, .. } => match self.ast.get_expr(*callee) {
                        Expr::Ident(n) if self.gen_factories().contains(n) => {
                            Some(Family::New(format!("__Gen_{n}")))
                        }
                        _ => None,
                    },
                    _ => None,
                }
            });
            // A nullish init widens on the name-keyed evidence alone
            // (module doc): the scope walk below still sees the
            // classifiable in-scope rebinds, this catches the rest.
            if matches!(family, Some(Family::Null | Family::Undef))
                && self.non_nullish_rebinds().contains(name)
            {
                self.out.insert(init);
            }
            Binding::Tracked { init, family }
        } else if ann.is_none() {
            Binding::Fixed { init }
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

    /// `name = value` — widen the tracked declaration when the rhs is
    /// typed `Any` or classifies into a different family.
    fn scan_assign(&mut self, target: ExprId, value: ExprId) {
        let Expr::Ident(name) = self.ast.get_expr(target) else {
            return;
        };
        // the innermost binding of the name decides; an opaque one
        // (const, annotated, param) is never widened
        let (init, family) = match self.scopes.iter().rev().find_map(|s| s.get(name).cloned()) {
            Some(Binding::Tracked { init, family }) => (init, family),
            Some(Binding::LayoutArr { init }) => {
                if let Some(types) = self.types
                    && crate::check_stmt_let_decl::any_layout_slot::has_dynamic_elem(
                        value, self.ast, types,
                    )
                {
                    self.out.insert(init);
                }
                return;
            }
            Some(Binding::Opaque | Binding::Fixed { .. }) | None => return,
        };
        if let Some(types) = self.types
            && types.get(&value) == Some(&Type::Any)
            && types.get(&init) != Some(&Type::Any)
        {
            self.out.insert(init);
            return;
        }
        let Some(family) = family else {
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
        if family != rhs || self.object_shapes_disagree(init, value) {
            self.out.insert(init);
        }
    }

    /// Two object literals assigned to the same binding are the same
    /// `Family::Obj`, and for a binding WITH an initializer that is
    /// the right answer: the init gave it a declared type, and
    /// whether the second literal fits is the checker's question
    /// (tsc agrees — `var o = {j(){}}; o = {}` is TS2741).
    ///
    /// A binding written `let x;` has no declared type to fit. tsc
    /// types it as an evolving `any` and accepts every shape;
    /// `desugar_uninit_let` hides the difference by promoting the
    /// first assignment into the init slot, so the two spellings
    /// arrive here looking alike and `Ast::spliced_uninit_inits` is
    /// what tells them apart.
    ///
    /// Keyed on the field NAMES only, and that is a BOUNDARY rather
    /// than agreement with tsc: an evolving `any` accepts every
    /// shape, so tsc takes `val = {toString(){return {}}}` followed
    /// by `val = {toString(){}}` as well, and tr still refuses it
    /// (test262 `built-ins/Symbol/desc-to-string`). Telling those
    /// two apart needs the VALUE types, which this pass runs too
    /// early to have. The module doc's rule applies — a shape the
    /// judgment misses keeps today's loud reject, never a silent
    /// wrong — and widening on names alone is as far as a syntactic
    /// judgment reaches.
    fn object_shapes_disagree(&self, init: ExprId, value: ExprId) -> bool {
        if !self.ast.spliced_uninit_inits.contains(&init) {
            return false;
        }
        match (obj_keys(self.ast, init), obj_keys(self.ast, value)) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }
}

/// The sorted field names of an object literal, or `None` for
/// anything else. Computed keys carry no static name and make the
/// shape unknowable, so a literal holding one answers `None` and
/// never disagrees with anything.
fn obj_keys(ast: &Ast, e: ExprId) -> Option<Vec<String>> {
    let Expr::ObjectLit { fields } = ast.get_expr(strip_as(ast, e)) else {
        return None;
    };
    let mut out = Vec::with_capacity(fields.len());
    for (k, v) in fields {
        if ast.objlit_computed_keys.contains_key(v) {
            return None;
        }
        out.push(format!("{k:?}"));
    }
    out.sort_unstable();
    Some(out)
}
