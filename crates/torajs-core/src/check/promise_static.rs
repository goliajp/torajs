//! `Promise.resolve(v)` / `Promise.reject(v)` static-method
//! typecheck. Extracted from `check.rs::type_of_inner` Call arm
//! (2026-06-03, god-file decomp — P10.5-A2 prerequisite per
//! `file-size.md` "先拆再加" rule).
//!
//! Both statics share return-type inference (arg-driven T extraction
//! → `Promise<T>`). The accepted T whitelist tracks v0.5 MVP shape:
//! number / string / boolean / array / struct / Date / RegExp /
//! Nullable / Promise<T> (thenable absorption). T-15.g.5 history
//! lived here; T-19.b/d/f extensions all landed inside this dispatch.
//!
//! P10.5-A2 will widen the `reject` arm to accept Type::Any so the
//! async-fn body try/catch wrapper (P10.5-A1) can shadow
//! `__async_err` as spec-correct `any` instead of the narrow MVP
//! `inner_ty` workaround.

use super::{Checker, Type};
use crate::ast::{Ast, Expr, ExprId};

impl Checker {
    /// Returns `None` if `callee` is not `Promise.resolve` or
    /// `Promise.reject`; otherwise returns the typecheck verdict.
    pub(crate) fn check_promise_resolve_reject_static(
        &mut self,
        ast: &Ast,
        callee: ExprId,
        args: &[ExprId],
    ) -> Option<Result<Type, String>> {
        let Expr::Member {
            obj: ns_id,
            name: m_name,
        } = ast.get_expr(callee)
        else {
            return None;
        };
        if m_name != "resolve" && m_name != "reject" {
            return None;
        }
        let Expr::Ident(ns) = ast.get_expr(*ns_id) else {
            return None;
        };
        if ns != "Promise" {
            return None;
        }
        // P10.2-A1 — 0-arg form: `Promise.resolve()` ≡
        // `Promise.resolve(undefined)` per ES spec
        // §27.2.4.6 (Promise.reject() symmetrically
        // rejects with undefined). Inner T = Undefined.
        if args.is_empty() {
            return Some(Ok(Type::Promise(Box::new(Type::Undefined))));
        }
        // S263 — Promise.{resolve,reject}(value, ...trailing)
        // trailing-arg ignore per ES §27.2.4.{7,8}. Spec reads
        // only args[0]; trailing silently drops. ssa_lower.rs's
        // 1-arg arm only references args[0] so trailing args
        // naturally drop at lower time (the args.len() guard
        // there widens to `>= 1` to admit this shape).
        for &arg in args.iter().skip(1) {
            if let Err(e) = self.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        let arg_ty = match self.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let inner = match &arg_ty {
            Type::Number | Type::String | Type::Boolean => arg_ty.clone(),
            // T-19.b (v0.5.0) — extended to accept heap-typed
            // T: Array, Struct (Object literal), Date, RegExp.
            // Runtime owns one rc on the inner value; drop
            // dispatches via __torajs_value_drop_heap.
            Type::Array(_) | Type::Struct(_) | Type::Date | Type::RegExp => arg_ty.clone(),
            // T-19.d (v0.5.0) — Nullable<T> + bare null. The
            // resolver returns the underlying T at SSA shape
            // (null is the in-band 0 sentinel), so the runtime
            // path is the same. We surface the inner type as
            // Promise<T | null> so caller sites stay
            // explicitly-nullable-aware.
            Type::Nullable(_) => arg_ty.clone(),
            // Bare `null` literal — promote to Type::Nullable
            // of an inferred T. For MVP just use Nullable<String>
            // which round-trips at the i64-ptr ABI; user code
            // typically uses `let p: Promise<T | null> = ...`
            // to make the intent explicit, in which case the
            // arg_ty above is already Nullable.
            Type::Null => Type::Nullable(Box::new(Type::String)),
            // T-19.f (v0.5.0) — thenable absorption.
            // `Promise.resolve(Promise<T>)` returns the
            // inner Promise<T> per spec (the resolved
            // value of the outer Promise IS the inner
            // Promise's resolved value). Type system
            // collapses Promise<Promise<T>> → Promise<T>
            // so caller sites see a flat shape; the
            // runtime side detects the nested-Promise
            // arg via type_tag and unwraps state +
            // value (rc-aware) instead of treating the
            // inner ptr as an i64 value.
            Type::Promise(boxed_inner) => (**boxed_inner).clone(),
            // P10.5-A2 — `Promise.reject` accepts Type::Any so the
            // async-fn body try/catch wrapper (desugar_async, P10.5-A1)
            // can shadow `__async_err` as spec-correct `any` per ES
            // §27.7.3.6 AsyncFunctionBody. Return type contextually
            // unifies with the enclosing fn's expected `Promise<T>`
            // (caller sees a Promise of the declared T; reject's
            // reason has no static type since it's the rejection
            // pathway). Standalone `Promise.reject(<any>)` outside a
            // typed context surfaces as `Promise<Any>`.
            //
            // Runtime path is the existing heap variant
            // (`__torajs_promise_alloc_rejected_heap`), since Any
            // lowers to an opaque pointer at SSA (24-byte boxed-any
            // heap struct via `__torajs_any_box`). Drop dispatches
            // universally via `__torajs_value_drop_heap` reading the
            // box's heap header. `Promise.resolve(any)` is
            // intentionally still rejected — unblock target is the
            // catch-side flow only.
            Type::Any if m_name == "reject" => match &self.expected_return {
                Some(Type::Promise(t)) => (**t).clone(),
                _ => Type::Any,
            },
            // P10.7 — `Promise.resolve(<any>)` lands a `Promise<any>`
            // wrapping the original value. Default-Any async fns
            // (`async function foo() { return e }`) desugar to
            // `return Promise.resolve(e as any)`; without this the
            // resolve call still trips the v0.5 MVP whitelist and
            // surfaces as a typecheck error before the `.then` /
            // outer-`await` chain can route through Type::Any. The
            // runtime path is the existing
            // `__torajs_promise_alloc_fulfilled_heap` (Type::Any
            // lowers to an opaque pointer at SSA — NaN-box AnyValue
            // is i64-sized).
            Type::Any if m_name == "resolve" => Type::Any,
            other => {
                return Some(Err(format!(
                    "Promise.{m_name}: T must be number / string / boolean / array / struct / Date / RegExp / nullable / Promise<T> in v0.5 MVP (got {other:?})"
                )));
            }
        };
        Some(Ok(Type::Promise(Box::new(inner))))
    }
}
