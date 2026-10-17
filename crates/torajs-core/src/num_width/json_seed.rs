//! ②.7 / W4 follow-up — JSON.parse number-face seeding. JSON text is
//! runtime data: no static analysis can prove a `number`-faced slot
//! fed by `JSON.parse` stays integral, and the cursor-driven typed
//! parser makes a narrow face WORSE than a bit-pun — `parse_int`
//! consumes `2` of `2.5` and leaves the cursor on `.5`, deranging
//! every later field. Every number-domain face reachable from the
//! parse target (scalar slot, array elems, named-type fields,
//! nested) seeds F64 unconditionally; explicit `: i64` / `: f64`
//! spellings keep the user's choice (the lowering consumer gates on
//! the annotation, same discipline as W1). The scalar top-level face
//! duplicates lowering's T-02 promotion — harmless overlap, one
//! source of truth here.

use super::{Analysis, SlotKey};
use crate::ast::PropKey;
use crate::ast::{Expr, ExprId, Stmt};
use std::collections::HashSet;

/// `number`-domain spelling: `number` or absent (inference).
pub(super) fn number_domain(ann: Option<&str>) -> bool {
    matches!(ann, None | Some("number"))
}

impl<'a> Analysis<'a> {
    /// Is this expr the `JSON.parse(text)` call shape? Mirrors
    /// lowering's `is_json_parse_call`.
    pub(super) fn is_json_parse(&self, eid: ExprId) -> bool {
        let Expr::Call { callee, args } = self.ast.get_expr(eid) else {
            return false;
        };
        if args.len() != 1 {
            return false;
        }
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
            return false;
        };
        name == "parse" && matches!(self.ast.get_expr(*obj), Expr::Ident(ns) if ns == "JSON")
    }

    /// Seed every number-domain face reachable from a JSON.parse
    /// target slot, walking the annotation spelling.
    pub(super) fn json_parse_seed(&mut self, key: &SlotKey, ann: Option<&str>) {
        if number_domain(ann) {
            self.add_constraint(key.clone(), super::magnitude::f64_flow());
        }
        let mut seen: HashSet<String> = HashSet::new();
        self.seed_number_faces(key, ann, &mut seen);
    }

    /// Seed the number-domain faces below the slot `key` of type
    /// `ann`: array elements, and the fields of an object type at any
    /// depth. A class has one layout, shared by every instance, so its
    /// fields seed on the class's own aggregation point (`Class(C)`),
    /// which the layout reads; a slot's own key would only widen a twin
    /// layout that no longer names the class.
    fn seed_number_faces(&mut self, key: &SlotKey, ann: Option<&str>, seen: &mut HashSet<String>) {
        let Some(ann) = ann else { return };
        let ann = crate::check_type_ann::strip_nullable(ann).trim();
        if let Some(elem) = ann.strip_suffix("[]") {
            let ek = SlotKey::Elem(Box::new(key.clone()));
            self.mark_containerish(key);
            self.mark_containerish(&ek);
            if number_domain(Some(elem)) {
                self.add_container_constraint(ek.clone(), super::magnitude::f64_flow());
            }
            self.seed_number_faces(&ek, Some(elem), seen);
            return;
        }
        // Named-type or object-type-literal spelling — seed each
        // field's face.
        if !seen.insert(ann.to_string()) {
            return;
        }
        let fields = self.object_fields(ann);
        if fields.is_empty() {
            return;
        }
        let holder = if self.ast.class_parents.contains_key(ann) {
            SlotKey::Class(ann.to_string())
        } else {
            key.clone()
        };
        self.mark_containerish(&holder);
        for (fname, fann) in fields {
            let fk = SlotKey::Field(Box::new(holder.clone()), fname);
            if number_domain(Some(&fann)) {
                self.add_container_constraint(fk.clone(), super::magnitude::f64_flow());
            }
            self.seed_number_faces(&fk, Some(&fann), seen);
        }
    }
}

impl Analysis<'_> {
    /// Seed the number-domain faces of the object type `ann` that the
    /// slot `key` is written an `any` value into. The write casts, and
    /// a cast may copy a dynamic object into a cell of the slot's
    /// layout (`ssa_lower_checked_cast::obj`): its property values are
    /// runtime data like JSON text, and a field narrowed past `number`
    /// would reject a fraction the annotation admits (RFC 20260922
    /// §4.4 item 1). A write at `site` in a body that never runs casts
    /// nothing.
    pub(super) fn any_object_seed(&mut self, site: ExprId, key: &SlotKey, ann: &str) {
        let (ast, retargets, types) = (self.ast, self.retargets, self.expr_types);
        let idle =
            (self.idle).get_or_init(|| super::idle_bodies::IdleBodies::of(ast, retargets, types));
        if idle.contains(site) {
            return;
        }
        let mut seen: HashSet<String> = HashSet::new();
        self.seed_number_faces(key, Some(ann), &mut seen);
    }

    /// [`Self::any_object_seed`] for the members of the object literal
    /// `lit` written into the slot `key` of the object type `ann`: a
    /// member handed an `any` value is cast into the type the field
    /// declares (`ssa_lower_object_lit::declared`), at any depth of
    /// nested literals. Any other value seeds nothing.
    pub(super) fn any_member_seed(&mut self, lit: ExprId, key: &SlotKey, ann: &str) {
        let Expr::ObjectLit { fields } = self.ast.get_expr(lit) else {
            return;
        };
        let declared = self.object_fields(crate::check_type_ann::strip_nullable(ann).trim());
        for (name, v) in fields.clone() {
            let Some((_, fann)) = declared.iter().find(|(k, _)| *k == name) else {
                continue;
            };
            let fk = SlotKey::Field(Box::new(key.clone()), name);
            if self.expr_types.get(&v) == Some(&crate::check::Type::Any) {
                self.any_object_seed(v, &fk, fann);
            } else {
                self.any_member_seed(v, &fk, fann);
            }
        }
    }

    /// The annotation the program wrote for the field `name` of the
    /// receiver `obj`: a class instance's declaration, or the field the
    /// receiver's own written annotation declares
    /// (`ast_throw_info::written_ann`).
    pub(super) fn member_field_ann(
        &self,
        obj: ExprId,
        name: &str,
        scope: &super::Scope,
    ) -> Option<String> {
        if let Some(ann) =
            crate::ast_throw_info::class_field_ann(self.ast, self.expr_types, obj, name)
        {
            return Some(ann);
        }
        let binding = |n: &str| self.binding_anns(&self.resolve(n, scope)?).pop();
        let class_of = |e| crate::ast_throw_info::class_of(self.expr_types, e);
        let owner = crate::ast_throw_info::written_ann(self.ast, obj, &binding, &class_of)?;
        let fields = self.object_fields(crate::check_type_ann::strip_nullable(&owner).trim());
        (fields.into_iter())
            .find(|(k, _)| k.as_wtf8().as_bytes() == name.as_bytes())
            .map(|(_, a)| a)
    }

    /// The fields of the object type `ann`: an object type literal's,
    /// or a declared type's (a class's among them; a generic
    /// instance's with its type arguments written in).
    pub(super) fn object_fields(&self, ann: &str) -> Vec<(PropKey, String)> {
        let inline = crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(ann)
            .map(|fs| fs.into_iter().map(|(f, a)| (PropKey::from(f), a)).collect());
        // the same declaration the cast at the write reads, so a field the
        // cast checks is one the walk seeds
        inline
            .or_else(|| crate::ast_throw_info::obj_cast::instance_fields(self.ast, ann))
            .unwrap_or_default()
    }

    /// The annotations the binding of slot `key` was declared with: the
    /// top level's `let`s by its name (a block's among them), or the
    /// `let`s and parameter of its function by its name. Every
    /// same-named `let` of a body shares the slot, so this answers each
    /// of them, a superset of the one declaration a cast reads at its
    /// write (`LowerCtx::binding_ann`).
    pub(super) fn binding_anns(&self, key: &SlotKey) -> Vec<String> {
        let decl = |f: &str| {
            self.ast.stmts.iter().find_map(|s| match s {
                Stmt::FnDecl {
                    name, params, body, ..
                } if name == f => Some((params, body)),
                _ => None,
            })
        };
        let mut out = Vec::new();
        let mut take = |s: &Stmt, want: &str| {
            if let Stmt::LetDecl {
                name,
                type_ann: Some(a),
                ..
            } = s
                && name == want
            {
                out.push(a.clone());
            }
        };
        match key {
            SlotKey::Global(n) => {
                let top = self.ast.stmts.iter();
                for s in top.filter(|s| !matches!(s, Stmt::FnDecl { .. })) {
                    crate::ast::walk_stmts(std::slice::from_ref(s), &mut |s| take(s, n));
                }
            }
            SlotKey::Local(f, n) => {
                if let Some((_, body)) = decl(f) {
                    crate::ast::walk_stmts(body, &mut |s| take(s, n));
                }
            }
            SlotKey::Param(f, n) => {
                let p = decl(f).and_then(|(ps, _)| ps.iter().find(|p| p.name == *n));
                out.extend(p.and_then(|p| p.type_ann.clone()));
            }
            _ => {}
        }
        out
    }

    /// The element annotations of the array `recv` reads, for an `any`
    /// value appended to it (`ast_throw_info::written_ann`).
    pub(super) fn pushed_elem_anns(&self, recv: ExprId, scope: &super::Scope) -> Vec<String> {
        let anns = match self.ast.get_expr(recv) {
            Expr::Ident(n) => {
                (self.resolve(n, scope)).map_or(Vec::new(), |k| self.binding_anns(&k))
            }
            _ => {
                let binding = |n: &str| self.binding_anns(&self.resolve(n, scope)?).pop();
                let class_of = |e| crate::ast_throw_info::class_of(self.expr_types, e);
                let ann = crate::ast_throw_info::written_ann(self.ast, recv, &binding, &class_of);
                ann.into_iter().collect()
            }
        };
        (anns.iter())
            .filter_map(|a| crate::ast_throw_info::elem_ann(a))
            .collect()
    }
}
