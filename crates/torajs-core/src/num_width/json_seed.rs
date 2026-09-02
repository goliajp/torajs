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

use super::{Analysis, SlotKey, W};
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
            self.add_constraint(key.clone(), W::F64);
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
                self.add_container_constraint(ek.clone(), W::F64);
            }
            self.json_seed_container(&ek, Some(elem), seen);
            return;
        }
        // Named-type spelling — seed each field's face.
        if !seen.insert(ann.to_string()) {
            return;
        }
        let fields: Vec<(String, String)> = self
            .ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::TypeDecl {
                    name,
                    type_params,
                    fields,
                } if name == ann && type_params.is_empty() => Some(fields.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if fields.is_empty() {
            return;
        }
        self.mark_containerish(key);
        for (fname, fann) in fields {
            let fk = SlotKey::Field(Box::new(key.clone()), PropKey::from(fname));
            if number_domain(Some(&fann)) {
                self.add_container_constraint(fk.clone(), W::F64);
            }
            self.json_seed_container(&fk, Some(&fann), seen);
        }
    }
}
