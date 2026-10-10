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
fn number_domain(ann: Option<&str>) -> bool {
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
        self.json_seed_container(key, ann, &mut seen);
    }

    fn json_seed_container(
        &mut self,
        key: &SlotKey,
        ann: Option<&str>,
        seen: &mut HashSet<String>,
    ) {
        let Some(ann) = ann else { return };
        let ann = ann.trim();
        if let Some(elem) = ann.strip_suffix("[]") {
            let ek = SlotKey::Elem(Box::new(key.clone()));
            self.mark_containerish(key);
            self.mark_containerish(&ek);
            if number_domain(Some(elem)) {
                self.add_container_constraint(ek.clone(), super::magnitude::f64_flow());
            }
            self.json_seed_container(&ek, Some(elem), seen);
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
        self.mark_containerish(key);
        for (fname, fann) in fields {
            let fk = SlotKey::Field(Box::new(key.clone()), fname);
            if number_domain(Some(&fann)) {
                self.add_container_constraint(fk.clone(), super::magnitude::f64_flow());
            }
            self.json_seed_container(&fk, Some(&fann), seen);
        }
    }
}

impl Analysis<'_> {
    /// Seed the number-domain fields of the object type `ann` that the
    /// slot `key` is written an `any` value into. The write casts, and
    /// a cast may rebuild a fresh dynamic object as a cell of the
    /// slot's layout (`ssa_lower_checked_cast::obj`): its property
    /// values are runtime data like JSON text, and a field narrowed
    /// past `number` would reject a fraction the annotation admits.
    pub(super) fn any_object_seed(&mut self, key: &SlotKey, ann: &str) {
        let base = crate::check_type_ann::strip_nullable(ann).trim();
        // a class slot takes the cell as it is, never a rebuilt one
        if self.ast.class_parents.contains_key(base) {
            return;
        }
        let fields = self.object_fields(base);
        if fields.is_empty() {
            return;
        }
        self.mark_containerish(key);
        for (fname, fann) in fields {
            if number_domain(Some(&fann)) {
                let fk = SlotKey::Field(Box::new(key.clone()), fname);
                self.add_container_constraint(fk, super::magnitude::f64_flow());
            }
        }
    }

    /// The fields of the object type `ann`: an object type literal's,
    /// or a declared non-generic type's (a class's among them).
    fn object_fields(&self, ann: &str) -> Vec<(PropKey, String)> {
        let inline = crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(ann)
            .map(|fs| fs.into_iter().map(|(f, a)| (PropKey::from(f), a)).collect());
        inline
            .or_else(|| {
                self.ast.stmts.iter().find_map(|s| match s {
                    Stmt::TypeDecl {
                        name,
                        type_params,
                        fields,
                    } if name == ann && type_params.is_empty() => Some(fields.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default()
    }

    /// The annotation the binding of slot `key` was declared with: a
    /// module `let`'s, a function `let`'s (by name) or a parameter's.
    pub(super) fn binding_ann(&self, key: &SlotKey) -> Option<String> {
        let let_ann = |stmts: &[Stmt], want: &str| {
            let mut found = None;
            crate::ast::walk_stmts(stmts, &mut |s| {
                if let Stmt::LetDecl {
                    name,
                    type_ann: Some(a),
                    ..
                } = s
                    && name == want
                {
                    found = Some(a.clone());
                }
            });
            found
        };
        let decl = |f: &str| {
            self.ast.stmts.iter().find_map(|s| match s {
                Stmt::FnDecl {
                    name, params, body, ..
                } if name == f => Some((params, body)),
                _ => None,
            })
        };
        match key {
            SlotKey::Global(n) => self.ast.stmts.iter().find_map(|s| match s {
                Stmt::LetDecl {
                    name,
                    type_ann: Some(a),
                    ..
                } if name == n => Some(a.clone()),
                _ => None,
            }),
            SlotKey::Local(f, n) => let_ann(decl(f)?.1, n),
            SlotKey::Param(f, n) => (decl(f)?.0.iter()).find(|p| p.name == *n)?.type_ann.clone(),
            _ => None,
        }
    }
}
