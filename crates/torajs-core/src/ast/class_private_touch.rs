//! Which class-member bodies touch a PRIVATE element (RFC
//! 20260917-class-eval-unit K2b cut 2).
//!
//! Two evaluations of the same unit class share a tag and therefore a
//! layout, so running one evaluation's method body against the other's
//! instance reads and writes the right offsets — for PUBLIC members
//! that is exactly right, and `C1.prototype.plain.call(c2)` must keep
//! working. §7.3.31 / §7.3.32 say the opposite for a private element:
//! the receiver has to carry the brand of THIS evaluation, so the same
//! call must throw when the body reaches for `#x`.
//!
//! The call-boundary guard needs that distinction as one bit, and it
//! has to be read where the body no longer is: `invoke_with_this`
//! only has the face. So the answer is baked into the method's
//! `MethodMeta` flags (bit 3) and this pass computes it.
//!
//! Timing is load-bearing. The scan runs right after `desugar_classes`
//! has minted the `__cm_` / `__sm_` / `__new_` top-level FnDecls and
//! BEFORE `lift_arrow_fns` hoists every arrow out of them: after the
//! lift, `m() { return (() => this.#x)() }` keeps its private access
//! in a `__closure_N` the method body no longer contains, and the bit
//! would read false.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, StaticInit, Stmt};
use std::collections::HashSet;

/// A private element's mangled spelling appears in three shapes: the
/// member name (`this.#x` → `Expr::Member { name: "__priv_C__x" }`),
/// a string argument (`#x in o` → `__torajs_priv_in_op("__priv_C__x",
/// o)`), and an identifier (a private method's own body is the
/// top-level `__cm_C____priv_C__m`). One substring test covers all
/// three.
///
/// `__priv__unit` is NOT one of them: K1 spells its hidden
/// per-evaluation class slot as a private-element row precisely so
/// that the reflection filters hide it, and every instance carries it.
fn mentions_private(s: &str) -> bool {
    s.contains("__priv_") && !s.contains(super::class_unit::UNIT_SLOT)
}

/// [`mentions_private`] over a WTF-8 literal (the `#x in o` shape
/// carries the name as a string argument). A lone surrogate cannot
/// spell the prefix, so the lossy view is exact here.
fn literal_mentions_private(s: &torajs_wtf8::Wtf8Buf) -> bool {
    mentions_private(&s.to_string_lossy())
}

#[derive(Default)]
struct Scan {
    hit: bool,
}

impl Scan {
    fn list(&mut self, ast: &Ast, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(ast, s);
        }
    }

    fn stmt(&mut self, ast: &Ast, s: &Stmt) {
        if self.hit {
            return;
        }
        match s {
            Stmt::FnDecl { body, .. } => self.list(ast, body),
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                if let Some(c) = ctor {
                    self.list(ast, &c.body);
                }
                for m in methods.iter().chain(static_methods) {
                    if mentions_private(&m.name.lossy()) {
                        self.hit = true;
                        return;
                    }
                    self.list(ast, &m.body);
                }
                for i in static_init {
                    match i {
                        StaticInit::Block(b) => self.list(ast, b),
                        StaticInit::Field(f) => self.expr(ast, f.init),
                    }
                }
            }
            _ => {}
        }
        for e in stmt_exprs(s) {
            self.expr(ast, e);
        }
        for c in stmt_children_ref(s) {
            self.stmt(ast, c);
        }
    }

    fn expr(&mut self, ast: &Ast, eid: ExprId) {
        if self.hit {
            return;
        }
        match ast.get_expr(eid) {
            Expr::Member { name, .. } if mentions_private(name) => {
                self.hit = true;
                return;
            }
            Expr::Ident(n) if mentions_private(n) => {
                self.hit = true;
                return;
            }
            Expr::String(s) if literal_mentions_private(s) => {
                self.hit = true;
                return;
            }
            // An arrow is still inline here (the pass runs before
            // `lift_arrow_fns`), so its body is reached through its
            // own node exactly as `class_unit::Occurrences` does.
            Expr::ArrowFn { body, .. } => {
                self.list(ast, body);
                return;
            }
            _ => {}
        }
        for c in expr_children(ast, eid) {
            self.expr(ast, c);
        }
    }
}

/// Record every class-member top-level FnDecl whose body (or a
/// parameter default, which is not in the body yet at this point)
/// reaches a private element. See the module doc for why the timing
/// matters.
pub fn collect_private_touch_fns(ast: &mut Ast) {
    let mut out: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        if !(name.starts_with("__cm_") || name.starts_with("__sm_") || name.starts_with("__new_")) {
            continue;
        }
        // A private method's own body is private by construction: its
        // receiver has to carry the brand whatever the body does.
        let mut scan = Scan {
            hit: mentions_private(name),
        };
        scan.list(ast, body);
        for p in params {
            if let Some(d) = p.default {
                scan.expr(ast, d);
            }
        }
        if scan.hit {
            out.insert(name.clone());
        }
    }
    ast.private_touch_fns = out;
}
