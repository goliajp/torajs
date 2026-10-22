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
        if m_name != "resolve" && m_name != "reject" && m_name != "withResolvers" {
            return None;
        }
        let Expr::Ident(ns) = ast.get_expr(*ns_id) else {
            return None;
        };
        if ns != "Promise" {
            return None;
        }
        // §27.2.4.8 (ES2024) — `Promise.withResolvers()` reads no
        // arguments (trailing exprs typecheck-and-drop, S273 idiom).
        // The result `{promise, resolve, reject}` is a runtime-minted
        // dynobj — surfaces as `any`, every member rides the any lane.
        if m_name == "withResolvers" {
            for &arg in args {
                if let Err(e) = self.type_of(ast, arg) {
                    return Some(Err(e));
                }
            }
            return Some(Ok(Type::Any));
        }
        // §27.2.4.6 — a rejected promise never fulfils, so its T is
        // unconstrained by the reason: TS types this
        // `Promise.reject<T = never>(reason?: any): Promise<T>`, and
        // the reason's own type says nothing about T. Reading T off
        // the argument (which `resolve` must do) made
        // `const p: Promise<number> = Promise.reject(new Error("x"))`
        // a type error. This lattice has no `never`, but `Any` plays
        // bottom at every assignment site (`check_assignable` answers
        // true for a `from` of Any before anything else) and `Promise`
        // is covariant there, so `Promise<Any>` is assignable to every
        // `Promise<T>` — the role `never` fills.
        //
        // The argument is still typechecked below for its own errors.
        if m_name == "reject" {
            for &arg in args {
                if let Err(e) = self.type_of(ast, arg) {
                    return Some(Err(e));
                }
            }
            return Some(Ok(Type::Promise(Box::new(Type::Any))));
        }
        // P10.2-A1 — 0-arg form: `Promise.resolve()` ≡
        // `Promise.resolve(undefined)` per ES spec §27.2.4.6. Inner
        // T = Undefined.
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
            Type::Array(_) | Type::Struct(_) | Type::Date | Type::RegExp => {
                self.resolved_object(ast, arg_ty.clone())
            }
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
            // rotation 234 — a class instance (`Promise.reject(new
            // Error("boom"))`, the test262 async family's canonical
            // rejection reason; any user class rides the same shape).
            // A ClassRef is a nominal heap struct at SSA, so the
            // runtime side is the existing heap adopt path — this
            // whitelist was the only gate.
            Type::ClassRef(_) => self.resolved_object(ast, arg_ty.clone()),
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
            // §27.2.4.7 — `Promise.resolve(undefined)` settles with
            // exactly what the zero-arg form settles with, so it gets
            // the same `Promise<Undefined>` that arm above answers.
            // RFC 20260713 blade 4 typed it `Promise<any>` for the
            // async tail-safety default, but nothing then produced an
            // `any` VALUE for it to hold: the lowering stored the
            // primitive 0 while `.value` decoded an Any pointer, so
            // `await Promise.resolve(undefined)` read a tag that does
            // not exist (`[unknown-any-tag]`, `typeof` → "object").
            Type::Undefined | Type::Void if m_name == "resolve" => Type::Undefined,
            other => {
                return Some(Err(format!(
                    "Promise.{m_name}: T must be number / string / boolean / array / struct / Date / RegExp / nullable / Promise<T> in v0.5 MVP (got {other:?})"
                )));
            }
        };
        Some(Ok(Type::Promise(Box::new(inner))))
    }

    /// The fulfilment type of `Promise.resolve(v)` for an object `v`
    /// whose type shows no `then`. §27.2.4.7 PromiseResolve hands it to
    /// the resolve function, which adopts it when a run-time
    /// `Get(v, "then")` answers a callable (§27.2.1.3.2 steps 9-13): it
    /// fulfils as itself only when the program is proven to give no
    /// object a `then`, the rule `await` follows (`Checker::awaited`).
    /// Otherwise what the promise settles with has no type.
    ///
    /// A type that shows a `then` keeps its own type here: the lowering
    /// takes the job road for it all the same, and `await` answers
    /// `any` for such an inner.
    fn resolved_object(&self, ast: &Ast, ty: Type) -> Type {
        let has_fn = |f: &str| self.globals.contains_key(f);
        if super::awaited::may_be_thenable(&ty, ast, &has_fn) || self.proto_shadow.then_closed(ast)
        {
            ty
        } else {
            Type::Any
        }
    }
}
