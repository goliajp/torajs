//! Field-initializer injection into the constructor — the tail of
//! class parsing, split from `parse_class_decl_member.rs` at the
//! size cap.
//!
//! Everything else in that file ANSWERS what a member is; this
//! decides where the answers run. The two were only ever adjacent.

use super::*;

impl<'a> Parser<'a> {
    /// Prepend `this.<n> = <init>` stmts for each collected field
    /// initializer (V3-18), synthesizing an empty ctor if none was
    /// declared so the inits still run on `new C(...)`.
    pub(super) fn finalize_class_field_inits(
        &mut self,
        class_name: &str,
        field_inits: Vec<(String, ExprId)>,
        ctor: Option<ClassCtor>,
    ) -> Option<ClassCtor> {
        if field_inits.is_empty() {
            return ctor;
        }
        let mut prefix: Vec<Stmt> = Vec::new();
        for (fname, init_expr) in &field_inits {
            let this_ref = self.ast.add_expr(Expr::This);
            // A `__ccm_<n>__` sentinel field becomes a KEYED write
            // into the expando dict (lhs built in the sibling).
            let lhs = if fname.starts_with("__ccm_") {
                self.computed_field_init_lhs(class_name, fname, this_ref)
            } else {
                self.ast.add_expr(Expr::Member {
                    obj: this_ref,
                    name: fname.clone(),
                })
            };
            let assign = self.ast.add_expr(Expr::Assign {
                target: lhs,
                value: *init_expr,
            });
            prefix.push(Stmt::Expr(assign));
        }
        Some(match ctor {
            Some(c) => {
                // §10.2.11 — the initializers run once `this` exists,
                // so in a derived class they belong AFTER the
                // `super(...)`, not at the top. In front of it, an
                // initializer reading an inherited field saw the slot
                // before the base ctor filled it (`e = this.v + 1`
                // answered 1 against a base `v = 5`). No super call in
                // the body means either a base class or a ctor that
                // never initializes `this` at all — the top is right
                // for the first and moot for the second.
                let at = self.super_stmt_index(&c.body).map_or(0, |i| i + 1);
                let mut body = c.body;
                body.splice(at..at, prefix);
                ClassCtor {
                    params: c.params,
                    body,
                }
            }
            None => {
                // The class declared no ctor; this one exists only to
                // hold the initializers. Record that, so the derived
                // default-ctor synthesis still treats the class as
                // having the implicit ctor it actually has.
                self.ast
                    .field_init_synth_ctors
                    .insert(class_name.to_string());
                ClassCtor {
                    params: Vec::new(),
                    body: prefix,
                }
            }
        })
    }

    /// Index of the first statement in `body` that IS a `super(...)`
    /// call. Only the statement level is examined: a super buried in a
    /// branch has no single "after" to speak of, and that shape is a
    /// recorded boundary elsewhere too.
    fn super_stmt_index(&self, body: &[Stmt]) -> Option<usize> {
        body.iter().position(|s| {
            matches!(s, Stmt::Expr(eid) if matches!(self.ast.get_expr(*eid), Expr::Super { .. }))
        })
    }
}
