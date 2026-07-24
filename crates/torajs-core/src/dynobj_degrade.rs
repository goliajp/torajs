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

use crate::ast::{Ast, Expr, ExprId};

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
    let mut w = Walker {
        ast,
        scopes: vec![HashMap::new()],
        out: HashSet::new(),
        fallback_names: HashSet::new(),
        registry: HashMap::new(),
    };
    for s in &ast.stmts {
        w.walk_stmt(s);
    }
    for n in &w.fallback_names {
        if let Some(eids) = w.registry.get(n) {
            w.out.extend(eids.iter().copied());
        }
    }
    w.out
}

struct Walker<'a> {
    ast: &'a Ast,
    /// Scope stack of the CURRENT fn only — fn bodies swap in a fresh
    /// stack (see module doc on closure captures).
    scopes: Vec<HashMap<String, Binding>>,
    out: HashSet<ExprId>,
    /// Receiver names that resolved to no binding in their fn — after
    /// the walk, every candidate declaration under these names marks.
    fallback_names: HashSet<String>,
    /// Every candidate declaration in the module, by name — the
    /// fallback marking target set.
    registry: HashMap<String, Vec<ExprId>>,
}

impl Walker<'_> {
    fn register_let(&mut self, name: &str, unannotated: bool, init: ExprId) {
        let candidate = unannotated && matches!(self.ast.get_expr(init), Expr::ObjectLit { .. });
        if !candidate {
            self.shadow(name);
            return;
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

    fn scan_defineproperty(&mut self, callee: ExprId, args: &[ExprId]) {
        let ast = self.ast;
        let Expr::Member { obj, name } = ast.get_expr(callee) else {
            return;
        };
        if !matches!(name.as_str(), "defineProperty" | "defineProperties") {
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
            Resolution::Tracked(eids) => self.out.extend(eids),
            Resolution::Opaque => {}
            Resolution::Free => {
                self.fallback_names.insert(recv.clone());
            }
        }
    }

    /// Rotation 203 chunk 2 — dynamic-member-write trigger: a write
    /// to a member the declaration's own literal doesn't spell
    /// (`obj.newField = v`) or a computed-key write (`obj[k] = v`)
    /// on a tracked binding degrades the declaration, giving the
    /// test262-pervasive "expando on a plain object" idiom a legal
    /// lane (JS object semantics; the strict struct checker rejects
    /// it otherwise). No `Free` fallback here: unlike defineProperty
    /// (where a missed degrade orphans the define = silent wrong),
    /// an unmarked dynamic write is rejected loudly by the checker,
    /// so under-marking is safe and precision wins.
    fn scan_dynamic_write(&mut self, target: ExprId) {
        let ast = self.ast;
        match ast.get_expr(target) {
            Expr::Member { obj, name } => {
                let Expr::Ident(recv) = ast.get_expr(*obj) else {
                    return;
                };
                let Resolution::Tracked(eids) = self.resolve(recv) else {
                    return;
                };
                let field = name.clone();
                for eid in eids {
                    if !self.init_has_field(eid, &field) {
                        self.out.insert(eid);
                    }
                }
            }
            Expr::Index { obj, .. } => {
                let Expr::Ident(recv) = ast.get_expr(*obj) else {
                    return;
                };
                if let Resolution::Tracked(eids) = self.resolve(recv) {
                    self.out.extend(eids);
                }
            }
            _ => {}
        }
    }

    /// Rotation 203 chunk 2 — `delete obj.k` / `delete obj[k]` on a
    /// tracked binding degrades it unconditionally: removing even an
    /// own field breaks the static struct layout, so the binding must
    /// live on the dynobj lane. Same no-fallback rationale as
    /// [`Self::scan_dynamic_write`].
    fn scan_delete(&mut self, operand: ExprId) {
        let ast = self.ast;
        let (Expr::Member { obj, .. } | Expr::Index { obj, .. }) = ast.get_expr(operand) else {
            return;
        };
        let Expr::Ident(recv) = ast.get_expr(*obj) else {
            return;
        };
        if let Resolution::Tracked(eids) = self.resolve(recv) {
            self.out.extend(eids);
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
        let Expr::ObjectLit { fields } = self.ast.get_expr(init) else {
            return false;
        };
        fields.iter().any(|(n, _)| {
            n == field
                || n.strip_prefix("__getter_") == Some(field)
                || n.strip_prefix("__setter_") == Some(field)
                || n.strip_prefix("__async_") == Some(field)
        })
    }
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
    fn dynamic_write_has_no_free_fallback() {
        // an unresolved fn-body write receiver is rejected loudly by
        // the checker if unmarked — precision wins over fallback here
        let src = r#"
let obj = { x: 1 };
function f() {
    obj.newField = 2;
}
"#;
        let got = collect(src);
        let decls = decl_inits(src, "obj");
        assert!(!got.contains(&decls[0]));
    }
}
