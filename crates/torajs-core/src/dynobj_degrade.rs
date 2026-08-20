//! P3.2 dynobj-degrade trigger collection — scope-correct pre-pass.
//!
//! `Object.defineProperty(obj, ...)` on a struct-typed binding
//! converts the runtime cell to a DynObj, but the dynobj write-back
//! only fires for `Type::Any` bindings (`emit_any_dynobj_writeback`)
//! — a statically-typed receiver kept pointing at the old struct
//! cell and the defined property landed on an orphan. The fix is to
//! type such bindings as `any` from the start (the P3.2 dynobj-init
//! lane), which needs the receiver declarations known before
//! type-checking (RC-4 F1c).
//!
//! The pre-rewrite collector (`define_receivers`) was a flat arena
//! scan keyed by NAME: any binding sharing a receiver's name degraded
//! module-wide, so a helper fn's `Object.defineProperty(obj, ...)`
//! silently widened every unrelated `var obj = {}` in other scopes —
//! passes gained through name collision, not semantics (the test262
//! passTotal inflation the no-metric-inflation iron rule bans). This
//! walker simulates scopes and keys the result by the DECLARATION
//! site instead: the `LetDecl` init ExprId, which both consumers
//! (`check_stmt_let_decl` / `ssa_lower_stmt_let_decl`) hold at query
//! time. Both sides still recompute from their own `Ast` snapshot,
//! so eid rewrites between check and lower can't strand the set —
//! the same no-drift contract the name keying had.
//!
//! Resolution rules:
//! - A binding tracks iff it could ever degrade: unannotated `let` /
//!   `var` with an ObjectLit init (the consumer's own gate). Params,
//!   annotated lets, imports, catch params shadow opaquely.
//! - Fn bodies never see enclosing scopes: lifted closures reach
//!   outer bindings through `__env` captures the walker cannot link
//!   statically, and a miss here is SILENT WRONG (orphaned define),
//!   so a receiver that resolves to no local binding falls back to
//!   conservative name-keyed marking of every candidate declaration
//!   module-wide — over-broad `any` is slower but never wrong.
//! - A receiver resolving to an opaque binding (a param, an
//!   annotated let) marks nothing: the declaration it shadows is
//!   provably not the receiver, and params were never degradable.
//! - Same-scope re-declarations (the `var obj = ...; var obj = ...`
//!   idiom) share one runtime binding: the slot accumulates every
//!   declaration site and a trigger seen before a later
//!   re-declaration carries forward onto it.

mod walk;

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// One name slot in the scope simulation.
enum Binding {
    /// Declaration sites (init ExprIds) of the unannotated-ObjectLit
    /// `let`s bound to this name in this scope, in source order.
    Tracked(Vec<ExprId>),
    /// Bound by something that can never degrade — shadows without
    /// tracking.
    Opaque,
}

enum Resolution {
    Tracked(Vec<ExprId>),
    Opaque,
    Free,
}

/// Collect the init ExprIds of every `let` declaration whose binding
/// receives a dynobj-degrade trigger, scope-correct:
///
/// - `Object.defineProperty` / `Object.defineProperties` /
///   `Reflect.defineProperty` with the binding as `args[0]`
///   (RC-4 F1c);
/// - a write to a member the declaration's literal doesn't spell,
///   a computed-key write, or a member `delete` on the binding
///   (rotation 203 chunk 2 — the JS expando idiom).
///
/// Consumed by both `check_pipeline` (binding type) and
/// `ssa_lower_fn` / `ssa_lower_synthesize_main` (init-lane routing)
/// so the two sides can't drift.
pub(crate) fn collect_dynobj_degraded_inits(ast: &Ast) -> HashSet<ExprId> {
    collect_degraded_inits(DegradeView {
        stmts: &ast.stmts,
        exprs: &ast.exprs,
        objlit_computed_keys: &ast.objlit_computed_keys,
    })
}

/// The slice-level surface this walker reads. `&Ast` is the shape the
/// check / lower consumers hold, but the AST-desugar phase runs with
/// the arena split into `stmts` / `exprs` locals — a view lets that
/// phase ask the SAME collector instead of re-deriving the trigger
/// set by hand, which is the drift the module doc bans.
#[derive(Clone, Copy)]
pub(crate) struct DegradeView<'a> {
    pub(crate) stmts: &'a [Stmt],
    pub(crate) exprs: &'a [Expr],
    pub(crate) objlit_computed_keys: &'a HashMap<ExprId, ExprId>,
}

impl<'a> DegradeView<'a> {
    fn get_expr(&self, id: ExprId) -> &'a Expr {
        &self.exprs[id.0 as usize]
    }
}

/// View-based core of [`collect_dynobj_degraded_inits`].
pub(crate) fn collect_degraded_inits(view: DegradeView<'_>) -> HashSet<ExprId> {
    let mut w = Walker {
        view,
        scopes: vec![HashMap::new()],
        out: HashSet::new(),
        fallback_names: HashSet::new(),
        fallback_introspect_names: HashSet::new(),
        fallback_write_fields: HashSet::new(),
        fallback_write_names: HashSet::new(),
        registry: HashMap::new(),
    };
    for s in view.stmts {
        w.walk_stmt(s);
    }
    for n in &w.fallback_names {
        if let Some(eids) = w.registry.get(n) {
            w.out.extend(eids.iter().copied());
        }
    }
    // introspection fallback skips accessor-bearing declarations —
    // their static-lane introspection is correct and the accessor
    // any-lane is a loud not-yet-supported boundary
    let introspect_hits: Vec<ExprId> = w
        .fallback_introspect_names
        .iter()
        .filter_map(|n| w.registry.get(n))
        .flatten()
        .copied()
        .filter(|e| !w.init_has_accessor_field(*e))
        .collect();
    w.out.extend(introspect_hits);
    // free-receiver writes (rotation 205) — expando spellings keep
    // the own-field gate; computed writes and deletes mark every
    // candidate under the name
    let write_hits: Vec<ExprId> = w
        .fallback_write_fields
        .iter()
        .filter_map(|(recv, field)| w.registry.get(recv).map(|eids| (eids, field)))
        .flat_map(|(eids, field)| {
            eids.iter()
                .copied()
                .filter(|e| !w.init_has_field(*e, field))
                .collect::<Vec<_>>()
        })
        .collect();
    w.out.extend(write_hits);
    for n in &w.fallback_write_names {
        if let Some(eids) = w.registry.get(n) {
            w.out.extend(eids.iter().copied());
        }
    }
    w.out
}

struct Walker<'a> {
    view: DegradeView<'a>,
    /// Scope stack of the CURRENT fn only — fn bodies swap in a fresh
    /// stack (see module doc on closure captures).
    scopes: Vec<HashMap<String, Binding>>,
    out: HashSet<ExprId>,
    /// Define-family receiver names that resolved to no binding in
    /// their fn — after the walk, every candidate declaration under
    /// these names marks.
    fallback_names: HashSet<String>,
    /// Introspection-family receivers that resolved `Free` — applied
    /// like `fallback_names` but skipping accessor-bearing decls.
    fallback_introspect_names: HashSet<String>,
    /// `(receiver, field)` of member writes whose receiver resolved
    /// `Free` — applied like `fallback_names` but keeping the
    /// own-field gate (a write the declaration's literal spells is a
    /// static-lane store, not an expando).
    fallback_write_fields: HashSet<(String, String)>,
    /// Computed-key-write / member-delete receivers that resolved
    /// `Free` — applied unconditionally like `fallback_names`.
    fallback_write_names: HashSet<String>,
    /// Every candidate declaration in the module, by name — the
    /// fallback marking target set.
    registry: HashMap<String, Vec<ExprId>>,
}

impl Walker<'_> {
    fn register_let(&mut self, name: &str, unannotated: bool, init: ExprId) {
        let candidate = unannotated && matches!(self.view.get_expr(init), Expr::ObjectLit { .. });
        if !candidate {
            self.shadow(name);
            return;
        }
        // RFC 20260725-objlit-computed-key 刀 2 — a literal carrying
        // a computed-key field degrades unconditionally: only the
        // dynobj lane can evaluate the key (ToPropertyKey) and name
        // the property at runtime.
        if let Expr::ObjectLit { fields } = self.view.get_expr(init)
            && fields
                .iter()
                .any(|(_, v)| self.view.objlit_computed_keys.contains_key(v))
        {
            self.out.insert(init);
        }
        self.registry
            .entry(name.to_string())
            .or_default()
            .push(init);
        let scope = self.scopes.last_mut().expect("scope stack never empty");
        match scope.get_mut(name) {
            Some(Binding::Tracked(eids)) => {
                // same-scope re-declaration shares one runtime binding:
                // a trigger already seen on an earlier site carries onto
                // this one
                if eids.iter().any(|e| self.out.contains(e)) {
                    self.out.insert(init);
                }
                eids.push(init);
            }
            _ => {
                scope.insert(name.to_string(), Binding::Tracked(vec![init]));
            }
        }
    }

    fn shadow(&mut self, name: &str) {
        self.scopes
            .last_mut()
            .expect("scope stack never empty")
            .insert(name.to_string(), Binding::Opaque);
    }

    fn resolve(&self, name: &str) -> Resolution {
        for scope in self.scopes.iter().rev() {
            match scope.get(name) {
                Some(Binding::Tracked(eids)) => return Resolution::Tracked(eids.clone()),
                Some(Binding::Opaque) => return Resolution::Opaque,
                None => {}
            }
        }
        Resolution::Free
    }

    /// `Object.*` / `Reflect.*` static calls whose first-arg receiver
    /// must live on the dynobj lane: the define family mutates through
    /// the dynobj write-back (RC-4 F1c), and the descriptor / proto
    /// introspection family answers wrongly on a struct-typed receiver
    /// (rotation 203 chunk 3 — the r205 pass→bug collateral: gOPD /
    /// getPrototypeOf silently mis-answered once the name-collision
    /// degrade stopped masking them). Both families share the
    /// conservative resolution: a `Free` receiver falls back to
    /// name-keyed marking because a miss is SILENT WRONG either way.
    fn scan_object_static_call(&mut self, callee: ExprId, args: &[ExprId]) {
        let ast = self.view;
        let Expr::Member { obj, name } = ast.get_expr(callee) else {
            return;
        };
        let is_define = matches!(name.as_str(), "defineProperty" | "defineProperties");
        let is_introspect = matches!(
            name.as_str(),
            "getOwnPropertyDescriptor"
                | "getOwnPropertyDescriptors"
                | "getOwnPropertyNames"
                | "getOwnPropertySymbols"
                | "getPrototypeOf"
                | "setPrototypeOf"
        );
        if !is_define && !is_introspect {
            return;
        }
        let Expr::Ident(ns) = ast.get_expr(*obj) else {
            return;
        };
        if ns != "Object" && ns != "Reflect" {
            return;
        }
        let Some(first) = args.first() else {
            return;
        };
        let Expr::Ident(recv) = ast.get_expr(*first) else {
            return;
        };
        match self.resolve(recv) {
            // introspection skips accessor-bearing decls: their
            // static-lane introspection answers correctly (the
            // objlit-accessor descriptor substrate) and the accessor
            // any-lane is a loud not-yet-supported boundary
            Resolution::Tracked(eids) => {
                let hits: Vec<ExprId> = eids
                    .into_iter()
                    .filter(|e| is_define || !self.init_has_accessor_field(*e))
                    .collect();
                self.out.extend(hits);
            }
            Resolution::Opaque => {}
            Resolution::Free => {
                if is_define {
                    self.fallback_names.insert(recv.clone());
                } else {
                    self.fallback_introspect_names.insert(recv.clone());
                }
            }
        }
    }

    /// Rotation 203 chunk 2 — dynamic-member-write trigger: a write
    /// to a member the declaration's own literal doesn't spell
    /// (`obj.newField = v`) or a computed-key write (`obj[k] = v`)
    /// on a tracked binding degrades the declaration, giving the
    /// test262-pervasive "expando on a plain object" idiom a legal
    /// lane (JS object semantics; the strict struct checker rejects
    /// it otherwise).
    ///
    /// Rotation 205 — a `Free` receiver falls back to name-keyed
    /// marking like the define family: the loud reject the original
    /// no-fallback stance relied on turns out to gate the pervasive
    /// `var obj = {}; function f() { obj.x = 1 }` idiom (the fn-body
    /// receiver is always `Free` here, and without the degrade the
    /// binding never reaches the named-fn-visible Any-global lane).
    /// The expando spelling keeps the own-field gate through the
    /// fallback; over-marking costs any-lane precision, never
    /// correctness.
    fn scan_dynamic_write(&mut self, target: ExprId) {
        let ast = self.view;
        match ast.get_expr(target) {
            Expr::Member { obj, name } => {
                // r293 — `as` layers are value pass-throughs:
                // `(box as any).k = v` writes the same binding
                // `box.k = v` does, so the receiver resolves through
                // them (S13_A10-adjacent "layout: []" family).
                let Expr::Ident(recv) = ast.get_expr(strip_as(ast, *obj)) else {
                    return;
                };
                match self.resolve(recv) {
                    Resolution::Tracked(eids) => {
                        for eid in eids {
                            if !self.init_has_field(eid, name) {
                                self.out.insert(eid);
                            }
                        }
                    }
                    Resolution::Opaque => {}
                    Resolution::Free => {
                        self.fallback_write_fields
                            .insert((recv.clone(), name.clone()));
                    }
                }
            }
            Expr::Index { obj, .. } => {
                let Expr::Ident(recv) = ast.get_expr(strip_as(ast, *obj)) else {
                    return;
                };
                match self.resolve(recv) {
                    Resolution::Tracked(eids) => self.out.extend(eids),
                    Resolution::Opaque => {}
                    Resolution::Free => {
                        self.fallback_write_names.insert(recv.clone());
                    }
                }
            }
            _ => {}
        }
    }

    /// Rotation 203 chunk 2 — `delete obj.k` / `delete obj[k]` on a
    /// tracked binding degrades it unconditionally: removing even an
    /// own field breaks the static struct layout, so the binding must
    /// live on the dynobj lane. Rotation 205 — a `Free` receiver
    /// takes the same name-keyed fallback as
    /// [`Self::scan_dynamic_write`].
    fn scan_delete(&mut self, operand: ExprId) {
        let ast = self.view;
        let (Expr::Member { obj, .. } | Expr::Index { obj, .. }) = ast.get_expr(operand) else {
            return;
        };
        let Expr::Ident(recv) = ast.get_expr(strip_as(ast, *obj)) else {
            return;
        };
        match self.resolve(recv) {
            Resolution::Tracked(eids) => self.out.extend(eids),
            Resolution::Opaque => {}
            Resolution::Free => {
                self.fallback_write_names.insert(recv.clone());
            }
        }
    }

    /// Does the declaration's literal spell `field` as an own member?
    /// Accessor pairs and async method shorthands are stored under
    /// synthesized names (`get s() {}` → `__setter_s` / `__getter_s`,
    /// `async m() {}` → `__async_m` — see parser/object_literal.rs),
    /// and all spell the user-visible property: a write to an
    /// accessor property is a setter invocation on the static lane,
    /// not an expando.
    fn init_has_field(&self, init: ExprId, field: &str) -> bool {
        let Expr::ObjectLit { fields } = self.view.get_expr(init) else {
            return false;
        };
        fields.iter().any(|(n, _)| {
            n == field
                || n.strip_prefix("__getter_") == Some(field)
                || n.strip_prefix("__setter_") == Some(field)
                || n.strip_prefix("__async_") == Some(field)
        })
    }

    /// Does the declaration's literal carry an accessor member
    /// (`get x()` / `set x()` — stored as `__getter_x` / `__setter_x`)?
    fn init_has_accessor_field(&self, init: ExprId) -> bool {
        matches!(
            self.view.get_expr(init),
            Expr::ObjectLit { fields } if fields
                .iter()
                .any(|(n, _)| n.starts_with("__getter_") || n.starts_with("__setter_"))
        )
    }
}

/// Peel `as` layers off a receiver expression — TS casts are static
/// assertions, the runtime binding underneath is what degrades (r293).
fn strip_as(ast: DegradeView<'_>, mut e: ExprId) -> ExprId {
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Stmt;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn collect(src: &str) -> HashSet<ExprId> {
        let tokens = tokenize(src).expect("lex");
        let ast = parse(src, &tokens).expect("parse");
        collect_dynobj_degraded_inits(&ast)
    }

    /// The init eids of every unannotated-ObjectLit let named `name`,
    /// in source order — lets tests name declarations positionally.
    fn decl_inits(src: &str, name: &str) -> Vec<ExprId> {
        let tokens = tokenize(src).expect("lex");
        let ast = parse(src, &tokens).expect("parse");
        let mut out = Vec::new();
        collect_lets(&ast, &ast.stmts, name, &mut out);
        out
    }

    fn collect_lets(ast: &Ast, stmts: &[Stmt], name: &str, out: &mut Vec<ExprId>) {
        for s in stmts {
            match s {
                Stmt::LetDecl {
                    name: n,
                    type_ann,
                    init,
                    ..
                } if n == name
                    && type_ann.is_none()
                    && matches!(ast.get_expr(*init), Expr::ObjectLit { .. }) =>
                {
                    out.push(*init)
                }
                Stmt::FnDecl { body, .. } => collect_lets(ast, body, name, out),
                Stmt::Block(v) | Stmt::Multi(v) => collect_lets(ast, v, name, out),
                _ => {}
            }
        }
    }

    #[test]
    fn direct_receiver_marks_its_decl() {
        let src = r#"
let obj = {};
Object.defineProperty(obj, "x", { value: 1 });
"#;
        let got = collect(src);
        let decls = decl_inits(src, "obj");
        assert_eq!(decls.len(), 1);
        assert!(got.contains(&decls[0]));
    }

    #[test]
    fn local_receiver_does_not_leak_to_other_scopes() {
        // the rotation-200 inflation shape: a helper's local receiver
        // must not degrade an unrelated same-named top-level binding
        let src = r#"
function helper() {
    let obj = {};
    Object.defineProperty(obj, "x", { value: 1 });
}
let obj = { a: 1 };
"#;
        let got = collect(src);
        let decls = decl_inits(src, "obj");
        assert_eq!(decls.len(), 2);
        assert!(got.contains(&decls[0]), "helper-local decl marks");
        assert!(!got.contains(&decls[1]), "top-level decl must not");
    }

    #[test]
    fn param_receiver_marks_nothing() {
        let src = r#"
function f(obj) {
    Object.defineProperty(obj, "x", { value: 1 });
}
let obj = {};
"#;
        let got = collect(src);
        let decls = decl_inits(src, "obj");
        assert_eq!(decls.len(), 1);
        assert!(!got.contains(&decls[0]));
    }

    #[test]
    fn free_receiver_falls_back_to_name_marking() {
        // a fn-body receiver that resolves to no local binding could
        // be a closure capture of ANY same-named decl — conservative
        // name fallback keeps the never-silent-wrong contract
        let src = r#"
let obj = {};
function f() {
    Object.defineProperty(obj, "x", { value: 1 });
}
"#;
        let got = collect(src);
        let decls = decl_inits(src, "obj");
        assert_eq!(decls.len(), 1);
        assert!(got.contains(&decls[0]));
    }

    #[test]
    fn block_shadow_resolves_innermost() {
        let src = r#"
let obj = { a: 1 };
{
    let obj = {};
    Object.defineProperty(obj, "x", { value: 1 });
}
"#;
        let got = collect(src);
        let decls = decl_inits(src, "obj");
        assert_eq!(decls.len(), 2);
        assert!(!got.contains(&decls[0]), "outer decl must not mark");
        assert!(got.contains(&decls[1]), "inner decl marks");
    }

    #[test]
    fn annotated_or_non_objlit_receivers_never_mark() {
        let src = r#"
let a: any = {};
Object.defineProperty(a, "x", { value: 1 });
let b = [1];
Object.defineProperty(b, "y", { value: 1 });
"#;
        assert!(collect(src).is_empty());
    }

    #[test]
    fn unknown_member_write_marks_but_own_field_write_does_not() {
        let src = r#"
let a = { x: 1 };
a.x = 2;
let b = { x: 1 };
b.newField = 2;
"#;
        let got = collect(src);
        let a = decl_inits(src, "a");
        let b = decl_inits(src, "b");
        assert!(!got.contains(&a[0]), "own-field write stays static");
        assert!(got.contains(&b[0]), "expando write degrades");
    }

    #[test]
    fn computed_write_and_delete_mark() {
        let src = r#"
let a = { x: 1 };
let k = "y";
a[k] = 2;
let b = { x: 1 };
delete b.x;
"#;
        let got = collect(src);
        let a = decl_inits(src, "a");
        let b = decl_inits(src, "b");
        assert!(got.contains(&a[0]), "computed-key write degrades");
        assert!(got.contains(&b[0]), "own-field delete degrades");
    }

    #[test]
    fn free_expando_write_falls_back_to_name_marking() {
        // rotation 205 — the pervasive named-fn expando idiom: the
        // fn-body receiver resolves Free, and without the fallback
        // the binding never reaches the Any-global lane
        let src = r#"
let obj = {};
function f() {
    obj.x = 1;
}
"#;
        let got = collect(src);
        let decls = decl_inits(src, "obj");
        assert_eq!(decls.len(), 1);
        assert!(got.contains(&decls[0]));
    }

    #[test]
    fn free_own_field_write_does_not_mark() {
        // the own-field gate holds through the Free fallback: a write
        // the literal spells is a static-lane store, not an expando
        let src = r#"
let s = { a: 1 };
function f() {
    s.a = 2;
}
"#;
        let got = collect(src);
        let decls = decl_inits(src, "s");
        assert_eq!(decls.len(), 1);
        assert!(!got.contains(&decls[0]));
    }

    #[test]
    fn free_computed_write_and_delete_mark() {
        let src = r#"
let a = { x: 1 };
let b = { x: 1 };
function f() {
    a["y"] = 2;
    delete b.x;
}
"#;
        let got = collect(src);
        let a = decl_inits(src, "a");
        let b = decl_inits(src, "b");
        assert!(got.contains(&a[0]), "free computed-key write degrades");
        assert!(got.contains(&b[0]), "free delete degrades");
    }

    #[test]
    fn computed_key_field_marks_unconditionally() {
        // RFC 20260725-objlit-computed-key 刀 2 — only the dynobj
        // lane can evaluate the key, so the literal degrades at
        // declaration
        let src = r#"
let k = "x";
let obj = { [k]: 1 };
let plain = { a: 1 };
"#;
        let got = collect(src);
        let obj = decl_inits(src, "obj");
        let plain = decl_inits(src, "plain");
        assert!(got.contains(&obj[0]), "computed-key literal degrades");
        assert!(!got.contains(&plain[0]), "plain literal stays static");
    }

    #[test]
    fn introspection_receiver_marks_like_defineproperty() {
        // the descriptor / proto introspection family mis-answers on
        // a struct receiver (r205 pass→bug collateral) — receivers
        // degrade under the same resolution rules as defineProperty
        let src = r#"
let a = { x: 1 };
Object.getOwnPropertyDescriptor(a, "x");
let b = { y: 1 };
Object.getPrototypeOf(b);
let c = { z: 1 };
function f(c) {
    Object.getOwnPropertyNames(c);
}
"#;
        let got = collect(src);
        let a = decl_inits(src, "a");
        let b = decl_inits(src, "b");
        let c = decl_inits(src, "c");
        assert!(got.contains(&a[0]), "gOPD receiver degrades");
        assert!(got.contains(&b[0]), "getPrototypeOf receiver degrades");
        assert!(!got.contains(&c[0]), "param receiver stays opaque");
    }

    #[test]
    fn introspection_skips_accessor_bearing_literals() {
        // static-lane introspection over accessor literals is correct
        // (objlit-accessor-enum-001 family); degrading them would hit
        // the accessor any-lane loud boundary
        let src = r#"
let g = { a: 1, get v(): number { return 2; } };
Object.getOwnPropertyNames(g);
let h = { a: 1, get v(): number { return 2; } };
Object.defineProperty(h, "x", { value: 1 });
"#;
        let got = collect(src);
        let g = decl_inits(src, "g");
        let h = decl_inits(src, "h");
        assert!(!got.contains(&g[0]), "introspection skips accessors");
        assert!(got.contains(&h[0]), "define family still marks");
    }

    #[test]
    fn accessor_property_write_is_not_expando() {
        // `set s()` is stored as `__setter_s` in the literal; the
        // write is a setter invocation on the static lane (gate
        // regression: objlit-accessor-001)
        let src = r#"
let c = { set s(n: number) { } };
c.s = 21;
"#;
        let got = collect(src);
        let decls = decl_inits(src, "c");
        assert_eq!(decls.len(), 1);
        assert!(!got.contains(&decls[0]));
    }

    #[test]
    fn free_write_fallback_is_field_keyed_not_name_keyed() {
        // a free expando write on one name must not drag an
        // unrelated same-field decl in: the fallback keys on
        // (receiver, field), not the field alone
        let src = r#"
let a = { x: 1 };
let b = {};
function f() {
    b.x = 2;
}
"#;
        let got = collect(src);
        let a = decl_inits(src, "a");
        let b = decl_inits(src, "b");
        assert!(!got.contains(&a[0]), "unrelated receiver must not mark");
        assert!(got.contains(&b[0]), "the written receiver marks");
    }
}
