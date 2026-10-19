//! M4.3.b — per-fn throw-shape analysis, split out of `ast.rs`
//! (file-size known-debt: ast.rs only shrinks).
//!
//! Describe a fn's throw shape: `direct_throw` is true if any `throw`
//! statement appears anywhere in the body; `called_fns` is the
//! deduplicated list of identifier names referenced as direct call
//! callees (`Expr::Call { callee: Ident(name) }`). The lowerer combines
//! this with a fixed-point closure to compute may_throw transitively.
//!
//! P7.4-a-b — `direct_throw` is ALSO set for IMPLICIT runtime throws the
//! AST has no `throw` node for: a bigint `Div/Mod/Pow/Shl/Shr` (the
//! runtime helper calls `__torajs_throw_range_error` on divide-by-zero
//! / negative-exponent / shift-too-large) and a `BigInt(x)` conversion
//! (RangeError on a non-integer / non-finite argument). Without this a
//! named fn whose only throw is one of these wouldn't be in `may_throw`,
//! so the caller's `emit_throw_check` would be skipped and the real
//! RangeError silently swallowed. The bigint-op test mirrors ssa_lower's
//! runtime dispatch condition exactly — `op ∈ {Div,Mod,Pow,Shl,Shr}`
//! AND BOTH operands type `check::Type::BigInt` per `expr_types` — so it
//! is precise: number `/ % ** << >>` is never marked (no bench-tr
//! hot-path throw-check regression), and every bigint throwing op is
//! (no silent swallow).
//!
//! bug-327 C2.5 — `direct_throw` is ALSO set when the fn calls through
//! a fn-valued binding (`thunk()` where thunk is a param, or `h()`
//! where h is a body-local `let`). The callee is statically unknown, so
//! the fixed-point can't resolve the name against any FnDecl — before
//! this rule the indirect call simply fell out of the analysis and a
//! throwing callback was silently swallowed at the caller's caller.
//! Param / let names are exact here: the typechecker only admits a
//! call through an ident whose binding is fn-typed, and builtin ctor
//! names (parseInt, Number, ...) are neither params nor body lets, so
//! hot conversion calls stay unmarked.
//!
//! Chunk 701 — `direct_throw` is ALSO set for method calls
//! (`Expr::Call { callee: Member { .. } }`, console.* exempt): the
//! method lowerings bottom out in runtime helpers that record pending
//! throws (kind-mismatch mutators, arr_any_to_typed, JSON.parse, OOB
//! index-assign, any-receiver dispatch, ...). Before this rule a
//! runtime TypeError raised inside a named fn unwound the fn body but
//! the caller's throw-check was M4.3.b-skipped: try/catch around the
//! call printed "no throw", a non-void callee answered the ret
//! sentinel 0, and top-level execution continued with exit code 0.
//! Method names are NOT mirrored per-shape (68 in-body throw-check
//! sites across the lowerers; a missed mirror is a silent swallow) —
//! the default is conservative and the exempt list carries the
//! never-throwing hot surfaces instead, because the two directions
//! fail differently: an over-wide exempt entry swallows, an
//! over-narrow default costs one cold TLS-load check.
//!
//! `direct_throw` is ALSO set for a member or index read off a
//! receiver that may be nullish (`scan.rs`, `nullish.rs`): the
//! lowering guards such a read with a runtime null check that records
//! a TypeError, and a fn whose only raise was that guard was pruned at
//! its callers — the throw surfaced at exit, not in the caller's
//! `try`. The receiver sources mirror the lowering's own, and a
//! narrowing on the path (`if (t === undefined) return`) keeps the
//! check off the callers of a fn that cannot reach the raise.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

mod keyed;
mod nullish;
pub(crate) mod obj_cast;
mod scan;
mod shapes;

pub(crate) use crate::ast_throw_info_fixpoint::compute_may_throw_fns;
pub(crate) use nullish::ProgramNames;
pub(crate) use obj_cast::{class_field_ann, class_of, elem_ann, returns_any_into_obj, written_ann};
pub(crate) use scan::Scan;
pub(crate) use shapes::ident_read_raises;
use shapes::is_property_store;

pub(crate) fn fn_throw_info(
    ast: &Ast,
    name: &str,
    params: &[Param],
    body: &[Stmt],
    expr_types: &HashMap<ExprId, crate::check::Type>,
    widths: &crate::num_width::WidthTable,
    assigned: ProgramNames<'_>,
) -> crate::ast_throw_info_fixpoint::FnThrow {
    let mut sc = Scan::new(ast, name, params, expr_types, widths, assigned);
    sc.fresh_recv = crate::ast::fresh_this::fresh_this_writes(ast, name, params, body).recv;
    sc.stmts(body);
    crate::ast_throw_info_fixpoint::FnThrow {
        direct: sc.direct,
        called: sc.called,
        nonint_read: sc.nonint_read,
        code_write: sc.code_write,
    }
}

impl Scan<'_> {
    /// `this.f` where `this` is a fresh receiver the lowering stores
    /// without a guard.
    fn fresh_store(&self, target: ExprId) -> bool {
        matches!(self.ast.get_expr(target), Expr::Member { obj, .. } if self.fresh_recv.contains(obj))
    }

    pub(crate) fn expr(&mut self, eid: ExprId) {
        let ast = self.ast;
        self.code_write |= keyed::spells_reflect_set(ast, eid);
        match ast.get_expr(eid) {
            Expr::Elision => {}
            Expr::Call { callee, args } => {
                crate::ast_throw_info_call::scan_call(self, *callee, args);
                self.direct |= self.args_cast(*callee, args) || self.entry_test_raises(eid, args);
            }
            Expr::BinOp { op, left, right } => {
                // Band B — the checker decided this pair can only throw
                // (`Ast::spec_throws`), and the lowering emits that raise
                // unconditionally. Both operands are typed primitives
                // (BigInt against Number is the shape), so the any-lane
                // test does NOT catch it: without this line a named fn
                // whose only raise is `1n & 1` is M4.3.b-skipped at its
                // caller and the throw is swallowed crossing the fn
                // boundary. Same reason `ident_read_raises` exists.
                if ast.spec_throws.contains_key(&eid) {
                    self.direct = true;
                }
                if crate::ast_throw_info_binop::binop_raises(
                    *op,
                    self.expr_types.get(left),
                    self.expr_types.get(right),
                ) {
                    self.direct = true;
                }
                self.expr(*left);
                // the right operand only runs when the left decided it
                match op {
                    crate::ast::BinOp::LAnd => self.under(*left, true, |sc| sc.expr(*right)),
                    crate::ast::BinOp::LOr => self.under(*left, false, |sc| sc.expr(*right)),
                    _ => self.expr(*right),
                }
            }
            Expr::Unary { expr, .. } => {
                // Band B, same reason as the BinOp arm above.
                self.direct |= ast.spec_throws.contains_key(&eid);
                self.expr(*expr)
            }
            Expr::Member { obj, name } => {
                // RFC 20260713 blade 6 — an any-receiver member READ lowers
                // to __torajs_any_member_get_tag, which records a pending
                // TypeError on a null/undefined receiver. Before this rule
                // a named fn whose only throw source was `p.x` (p: any) was
                // M4.3.b-skipped at the caller: the pending throw was
                // silently dropped, the read answered garbage NaN-box bits
                // (`[unknown-any-tag]`), and a later drop walk could
                // SIGSEGV. Mirrors the chunk-701 method-call rule. A typed
                // receiver raises only through the nullish guard, which
                // `receiver_raises` answers from the lowering's sources.
                // a class instance's field read runs a getter once the
                // program can install one there (`accessor_field_names`)
                if matches!(self.expr_types.get(obj), Some(crate::check::Type::Any))
                    || self.receiver_raises(*obj)
                    || (ast.accessor_field_names.may(name)
                        && obj_cast::object_ty(self.expr_types.get(obj)))
                {
                    self.direct = true;
                }
                self.expr(*obj)
            }
            Expr::Assign { target, value } => {
                // Every member store can raise — see `is_member_store`
                // for why the receiver's type cannot tell us otherwise,
                // and for the three whitelist arms this one replaces.
                // The one exception is positional too: a constructor's
                // leading write to a declared field of an instance
                // nothing has seen yet, which the lowering stores
                // unguarded on the same proof.
                if is_property_store(ast, *target) && !self.fresh_store(*target) {
                    self.direct = true;
                }
                self.code_write |= self.keyed_write_carries_code(*target, *value);
                self.direct |= self.store_casts(*target, *value);
                self.expr(*target);
                self.expr(*value);
            }
            Expr::Index { obj, index } => self.index_read(*obj, *index),
            Expr::Array(elems) => {
                for e in elems {
                    self.expr(*e);
                }
            }
            Expr::ObjectLit { fields } => {
                for (k, e) in fields {
                    self.direct |= shapes::spread_runs_getters(ast, self.expr_types, k, *e);
                    self.expr(*e);
                }
            }
            // ArrowFn / Closure bodies are walked separately (their own
            // FnDecls — lifted by lift_arrow_fns); from this fn's
            // perspective the closure body's calls don't propagate to the
            // outer fn's may_throw bit until the outer fn actually invokes
            // the closure (which is itself a Call → tracked above). Same
            // reasoning applies to the bigint-throw bit: a throwing bigint
            // op INSIDE the closure belongs to the closure's own lifted
            // FnDecl, not the enclosing fn.
            Expr::ArrowFn { .. } | Expr::Closure { .. } => {}
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    self.expr(*a);
                }
            }
            Expr::NewDynamic { callee, args } => {
                self.expr(*callee);
                for a in args {
                    self.expr(*a);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                self.under(*cond, true, |sc| sc.expr(*then_branch));
                self.under(*cond, false, |sc| sc.expr(*else_branch));
            }
            Expr::Spread { expr } => crate::ast_throw_info_call::scan_spread(self, *expr),
            Expr::As { expr, ty_ann } => {
                self.direct |= obj_cast::lit_casts_member(ast, self.expr_types, ty_ann, *expr);
                self.expr(*expr)
            }
            Expr::TypeOf { expr } | Expr::Delete { expr } => self.expr(*expr),
            Expr::InstanceOf { expr, rhs } => {
                self.expr(*expr);
                self.expr(*rhs);
            }
            Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.expr(*left);
                self.expr(*right);
            }
            Expr::OptChain { obj, name } => {
                if ast.accessor_field_names.may(name)
                    && obj_cast::object_ty(self.expr_types.get(obj))
                {
                    self.direct = true;
                }
                self.expr(*obj)
            }
            Expr::OptIndex { obj, index } => self.opt_index_read(*obj, *index),
            // `f?.(…)` — the callee is a dynamic value (any_call raises a
            // catchable TypeError on non-callables; a statically-plain
            // callee delegates to the Call lowering): conservative
            // may-throw, same as the Index/OptChain-callee rule above.
            Expr::OptCall { callee, args } => {
                self.direct = true;
                self.expr(*callee);
                for a in args {
                    if let Expr::Closure { fn_name, .. } = ast.get_expr(*a) {
                        self.record(fn_name);
                    }
                    self.expr(*a);
                }
            }
            Expr::PostIncr { target, .. } => {
                // `o.k++` reads, adds and STORES, so it is on the same
                // rule as the Assign arm above (this arm only ever named
                // `xs.length--`).
                //
                // Measured, so it is not claimed as a fix: `o.hit++` on a
                // FROZEN class instance still answers "after" with this
                // marking in place, while the plain `o.hit = 5` spelling
                // of it throws. The `++` store lane does not consult the
                // §10.1.9 writable gate at all, so there is no pending
                // throw for a caller check to find — the gap is in the
                // lowering, one layer below this file, and is registered
                // separately. Marking is still right here: when that lane
                // does raise, the caller must not have pruned its check.
                if is_property_store(ast, *target) {
                    self.direct = true;
                }
                self.expr(*target)
            }
            Expr::This | Expr::NewTarget => {}
            Expr::Ident(_) if ident_read_raises(ast, eid) => {
                self.direct = true;
            }
            Expr::Ident(_)
            | Expr::String(_)
            | Expr::Number(_)
            | Expr::BigInt { .. }
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Uninit
            | Expr::Regex { .. } => {}
        }
    }
}
