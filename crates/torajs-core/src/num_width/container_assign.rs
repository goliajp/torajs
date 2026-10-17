//! W4 — element / field assignment constraints for the container
//! face (`xs[i] = e`, `o.x = e`), split from `container_walk.rs`.

use super::{Analysis, Scope, SlotKey};
use crate::ast::PropKey;
use crate::ast::{Expr, ExprId};

impl<'a> Analysis<'a> {
    /// Element / field write through an Assign target (`xs[i] = e`,
    /// `o.x = e`). An untrackable index-assign receiver poisons every
    /// container query — conservative, never silent-wrong.
    pub(super) fn container_assign(&mut self, target: ExprId, value: ExprId, scope: &Scope) {
        let t = self.ast.get_expr(target).clone();
        match &t {
            Expr::Index { obj, .. } => match self.container_key_of(*obj, scope) {
                Some(rk) => {
                    self.mark_containerish(&rk);
                    let ek = SlotKey::Elem(Box::new(rk));
                    let w = self.flow(value, scope);
                    self.add_container_constraint(ek.clone(), w);
                    // A function value assigned into an element: same
                    // projection wiring as a `push` (638-13).
                    self.fn_value_flow(&ek, value, scope);
                    if let Some(vk) = self.container_key_of(value, scope) {
                        self.uf.union(&ek, &vk);
                    }
                }
                None => self.container_poison = true,
            },
            Expr::Member { obj, name } => {
                // Growing `length` leaves holes that an in-bounds read
                // then lands on, and a number[] hole holds the F64
                // sentinel — seed like `delete xs[i]`. `length = 0`
                // only truncates.
                if name == "length"
                    && !matches!(self.ast.get_expr(value), Expr::Number(n) if *n == 0.0)
                {
                    self.seed_index_read_elem(*obj, scope, false);
                }
                if let Some(rk) = self.container_key_of(*obj, scope) {
                    self.mark_containerish(&rk);
                    let fk = SlotKey::Field(Box::new(rk), PropKey::from(name));
                    if self.expr_types.get(&value) == Some(&crate::check::Type::Any)
                        && let Some(ann) = crate::ast_throw_info::class_field_ann(
                            self.ast,
                            self.expr_types,
                            *obj,
                            name,
                        )
                    {
                        self.any_object_seed(value, &fk, &ann);
                    } else if let Some(ann) = self.member_field_ann(*obj, name, scope) {
                        self.any_member_seed(value, &fk, &ann);
                    }
                    let w = self.flow(value, scope);
                    self.add_container_constraint(fk.clone(), w);
                    // F5 — fn value assigned into a field: same sig
                    // projection wiring as the struct-literal fill.
                    self.fn_value_flow(&fk, value, scope);
                    if let Some(vk) = self.container_key_of(value, scope) {
                        self.uf.union(&fk, &vk);
                    }
                }
                self.feed_setter_value(*obj, name, value, scope);
            }
            _ => {}
        }
    }
}
