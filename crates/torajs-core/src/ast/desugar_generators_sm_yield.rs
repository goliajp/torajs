//! The yield step of the generator state machine: the operand held in
//! a per-state local, the resume label persisted, and the `{ value,
//! done: false }` step returned. Split out of `desugar_generators_sm`
//! for file size; consumed by [`GenSm::lower`].

use super::desugar_generators_sm::GenSm;
use super::{Expr, ExprId, Stmt};

/// Prefix of the per-yield local that holds the evaluated operand
/// across the resume-label store (`emit_yield_return`); the suffix is
/// the resume state, so every arm's binding is its own.
const YIELD_OPERAND_LOCAL: &str = "__gen_yv_";

impl GenSm<'_> {
    /// Persist the resume label into `this.__state`. Only a YIELD does
    /// this: it is the one exit that leaves the generator resumable.
    /// See [`super::desugar_generators_sm::RESUME_LOCAL`] for why
    /// nothing else may write the field.
    fn emit_set_state(&mut self, target: usize) -> Stmt {
        let this_id = self.ast.add_expr(Expr::This);
        let m = self.ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__state".into(),
        });
        let lit = self.ast.add_expr(Expr::Number(target as f64));
        let assign = self.ast.add_expr(Expr::Assign {
            target: m,
            value: lit,
        });
        Stmt::Expr(assign)
    }

    /// The operand is evaluated into a per-state local BEFORE the
    /// resume label is persisted. A throw while the operand is being
    /// built is an abrupt completion of the body at the yield
    /// (§27.5.3.7 step 3 evaluates the operand first; the generator
    /// is COMPLETED, RESUME_LOCAL doc); storing the label first left
    /// the machine resumable at the post-yield arm, so the next
    /// `next()` ran the statements after the yield — or read the
    /// never-assigned step of a `yield*` (`__gfstep_N.done` on
    /// undefined, the TypeError the D4 driver surfaced when
    /// `__agen_step_call` threw synchronously).
    pub(super) fn emit_yield_return(&mut self, val: ExprId, next: usize) -> Vec<Stmt> {
        let set = self.emit_set_state(next);
        // §27.6.3.8 AsyncGeneratorYield step 5 — an ASYNC generator
        // awaits what it yields, so `yield Promise.resolve(3)` steps
        // with 3 and `yield Promise.reject(e)` REJECTS the step's
        // promise instead of resolving with the rejected promise
        // itself. A sync generator yields verbatim (§27.5.3.7) and
        // must keep doing so — `g().next().value` on a yielded
        // promise IS the promise. The mark makes the read
        // type-dispatched (a promise unwraps, anything else is
        // identity), the same one the parser's `await e` and the
        // async `return(v)` (§27.6.3.7 step 8.d) mint.
        //
        // `yield*` needs no separate arm: the parser desugars
        // delegation into plain yields, and AsyncGeneratorYield is
        // the step §27.6.3.9 routes each delegated value through too.
        let val = if self.is_async_gen {
            let read = self.ast.add_expr(Expr::Member {
                obj: val,
                name: "value".into(),
            });
            self.ast.await_value_reads.insert(read);
            read
        } else {
            val
        };
        // P10.7 — Default-Any yield: route through `Expr::As { …,
        // ty_ann: "any" }` so the step's `value: any` field write
        // picks up the existing box-to-Any path (NaN-box AnyValue).
        // Explicit-T generators (`yield_ty != "any"`) keep their
        // direct write — both branches still produce the same
        // step shape from the user's perspective.
        // Rotation 640 — a typed lane casts too: the value of a `yield`
        // expression is `T | undefined` now (`sent_slot_ann`, an `any`
        // for a scalar `T`), and yielding it back (`let v = yield e;
        // yield v`) has to land in the step's `value: T` slot — the
        // checker refused the struct otherwise ("expects Struct value:
        // Number, got Nullable(Number)"). TypeScript's own type for
        // that value is `any`, so the assertion adds nothing the
        // program did not already claim.
        // The local carries the operand's OWN type (no annotation) and
        // the cast sits on the read, so the step literal sees the same
        // shape it did when the operand was written there directly:
        // annotating the local `number` gave a typed generator's I64
        // step slot an F64 (`generator-005-bidirectional`, the JIT
        // lane's `materialize_operand_gpr … holding Fpr`).
        // An object literal (or an array of them) is the exception: held
        // under the declared yield type, it is stored in that type's
        // layout even when it spells the fields in another order, which
        // the step's cast then reads as is.
        let held_name = format!("{YIELD_OPERAND_LOCAL}{next}");
        let literal = match self.ast.get_expr(val) {
            Expr::ObjectLit { .. } => true,
            Expr::Array(els) => {
                !els.is_empty()
                    && els
                        .iter()
                        .all(|e| matches!(self.ast.get_expr(*e), Expr::ObjectLit { .. }))
            }
            _ => false,
        };
        let hold = Stmt::LetDecl {
            mutable: false,
            name: held_name.clone(),
            type_ann: (literal && self.yield_ty != "any").then(|| self.yield_ty.clone()),
            init: val,
            is_var: false,
        };
        let held = self.ast.add_expr(Expr::Ident(held_name));
        let step_ann = if self.yield_ty == "any" {
            "any".to_string()
        } else {
            self.yield_ty.clone()
        };
        let val_for_step = self.ast.add_expr(Expr::As {
            expr: held,
            ty_ann: step_ann,
        });
        let done = self.ast.add_expr(Expr::Bool(false));
        let obj = self.ast.add_expr(Expr::ObjectLit {
            fields: vec![("value".into(), val_for_step), ("done".into(), done)],
        });
        vec![hold, set, Stmt::Return(Some(obj))]
    }
}
