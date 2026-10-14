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
        field_inits: Vec<(PropKey, ExprId)>,
        ctor: Option<ClassCtor>,
    ) -> Option<ClassCtor> {
        if field_inits.is_empty() {
            return ctor;
        }
        let mut prefix: Vec<Stmt> = Vec::new();
        for (fname, init_expr) in &field_inits {
            if crate::ast::private_brand::is_field_add(&self.ast, *init_expr) {
                prefix.push(Stmt::Expr(*init_expr));
                continue;
            }
            let this_ref = self.ast.add_expr(Expr::This);
            // A `__ccm_<n>__` sentinel field becomes a KEYED write
            // into the expando dict (lhs built in the sibling).
            let lhs = if fname.starts_with("__ccm_") {
                self.computed_field_init_lhs(class_name, fname, this_ref)
            } else if let Some(ident) = fname.as_str() {
                self.ast.add_expr(Expr::Member {
                    obj: this_ref,
                    name: ident.to_string(),
                })
            } else {
                // A key with a lone surrogate is not an identifier —
                // it is written the way the program could have:
                // `this["\uD800"] = init`, the keyed-write lane.
                let key = self
                    .ast
                    .add_expr(Expr::String(fname.clone().into_wtf8buf()));
                self.ast.add_expr(Expr::Index {
                    obj: this_ref,
                    index: key,
                })
            };
            let assign = self.ast.add_expr(Expr::Assign {
                target: lhs,
                value: *init_expr,
            });
            // K6 residue — the class expressions the initializer
            // minted declare right in front of it, so they evaluate
            // per construction with the initializer's scope in reach.
            if let Some(decls) = self.field_init_classes.remove(&init_expr.0) {
                prefix.extend(decls);
            }
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
                // The default-materialization guards belong past the
                // initializers (§10.2.2 order); the pass reads this.
                self.ast
                    .ctor_field_init_end
                    .insert(class_name.to_string(), at + prefix.len());
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

    /// Parse an INSTANCE field's initializer, keeping the class
    /// expressions it mints for the ctor prefix (K6 residue): the
    /// buffer flag routes them to the use-site-local list, and the
    /// slice minted here is parked under the initializer's ExprId.
    pub(super) fn parse_instance_field_init(&mut self) -> Result<ExprId, String> {
        let mark = self.synth_classes_local.len();
        let saved = std::mem::replace(&mut self.in_instance_field_init, true);
        let init = self.parse_field_initializer();
        self.in_instance_field_init = saved;
        let init = init?;
        let decls = self.synth_classes_local.split_off(mark);
        if !decls.is_empty() {
            self.field_init_classes.insert(init.0, decls);
        }
        Ok(init)
    }

    /// §15.7 FieldDefinition — `Initializer[+In, ~Yield, ~Await]`. The
    /// initializer drops the enclosing `[Await]`, so inside an async body
    /// `await` reads as a name there again and `x = await 1` is a name
    /// followed by a stray number; the Module goal still reserves the
    /// name through the site the admission records.
    pub(super) fn parse_field_initializer(&mut self) -> Result<ExprId, String> {
        let saved_await = std::mem::replace(&mut self.await_allowed, false);
        let saved_reserved = std::mem::replace(&mut self.await_reserved, false);
        let init = self.parse_assign();
        self.await_allowed = saved_await;
        self.await_reserved = saved_reserved;
        init
    }

    /// RFC 20260916-private-element-brand slice 2b — §7.3.34 step 3
    /// adds each private field right after its own initializer ran, in
    /// declaration order. When the member just parsed declared a
    /// private instance field and some initializer precedes it, mark
    /// that moment with `__torajs_priv_field_add("__priv_C__x")` in
    /// the initializer list, which the prefix emits as a statement and
    /// the class desugar turns into the install. A field no
    /// initializer precedes is installed with the class's elements
    /// start instead (`ast/private_brand.rs`), so it needs no marker.
    pub(super) fn push_private_field_add(
        &mut self,
        fields: &[(PropKey, String)],
        field_inits: &mut Vec<(PropKey, ExprId)>,
        fields_mark: usize,
    ) {
        let Some((key, _)) = fields.get(fields_mark) else {
            return;
        };
        let Some(n) = key.as_str().filter(|n| n.starts_with("__priv_")) else {
            return;
        };
        if field_inits.is_empty() {
            return;
        }
        let callee = self
            .ast
            .add_expr(Expr::Ident(crate::ast::private_brand::FIELD_ADD.into()));
        let name = self.ast.add_expr(Expr::String(n.into()));
        let call = self.ast.add_expr(Expr::Call {
            callee,
            args: vec![name],
        });
        field_inits.push((key.clone(), call));
    }

    /// Index of the first statement in `body` that IS a `super(...)`
    /// call. Only the statement level is examined: a super buried in a
    /// branch has no single "after" to speak of, and that shape is a
    /// recorded boundary elsewhere too.
    pub(super) fn super_stmt_index(&self, body: &[Stmt]) -> Option<usize> {
        body.iter().position(|s| {
            matches!(s, Stmt::Expr(eid) if matches!(self.ast.get_expr(*eid), Expr::Super { .. }))
        })
    }
}
