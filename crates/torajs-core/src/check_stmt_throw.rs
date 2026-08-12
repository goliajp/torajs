//! `Stmt::Throw(eid)` typecheck pulled out of
//! [`crate::check::Checker::check_stmt`]'s `Stmt::Throw` arm as
//! chunk-98 of the check_stmt decomp. First check_stmt sibling.
//!
//! M4.3 — accept any 8-byte-shaped throw value. The global
//! `throw_value` slot is i64; ptr-shaped values (Str / Obj / Arr /
//! FnSig / Closure) are reinterpreted at catch time per the catch
//! param's type annotation. `type_of` has side effects (it runs
//! `consume` on consuming-arg positions inside any sub-Call), so
//! evaluate it once and reuse the result for both the type check
//! AND the throw consume.
//!
//! Accepted throw value types:
//! - `Number` / `String` / `Boolean` (M4 primitives)
//! - `Array<T>` / `Struct<...>` (heap shapes — pointer reinterpret)
//! - `Null` / `Nullable<T>` — `throw null` is valid JS; null lowers
//!   to the same 0-sentinel pointer shape; catch annotation
//!   determines interpretation.
//! - `Undefined` (P7.2a) — lowers to ANY_UNDEF=5 / payload 0 in the
//!   throw match; catch `: any` reconstructs via `any_box(5, 0)`.
//!   Pre-P7.2a fell through to the M4-era "8-byte-shaped" reject.
//! - `Any` (P4.7) — tagged throw_set / take_tag substrate records
//!   the dynamic tag so catch `: any` reconstructs an Any-box.
//!
//! Non-Copy types: throw consumes the value via `consume_escape`
//! (heap is now owned by the catch site; borrowed aliases may
//! escape — retain-at-throw gives the catch its own +1).

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, DiagPush, Type};

pub(crate) fn check(checker: &mut Checker, ast: &Ast, eid: ExprId) {
    let t_result = checker.type_of(ast, eid);
    match &t_result {
        Ok(Type::Number) | Ok(Type::String) | Ok(Type::Boolean) => {}
        // RFC 20260715-nominal-class-identity — `throw new Error(...)`
        // types as `ClassRef("Error")` now; a class instance is a heap
        // pointer, the same 8-byte shape as the struct behind it.
        Ok(Type::Array(_)) | Ok(Type::Struct(_)) | Ok(Type::ClassRef(_)) => {}
        // Rotation 375 — `throw function () {…}` (the try-statement
        // 12.14 family): a closure is a heap cell, the same 8-byte
        // pointer shape; the lowering's refcounted fallback packs it
        // ANY_HEAP=4 and a `catch (e: any)` reconstructs the box, so
        // `e()` rides the any-call lane.
        Ok(Type::Function(..)) => {}
        Ok(Type::Null) | Ok(Type::Nullable(_)) => {}
        Ok(Type::Undefined) => {}
        Ok(Type::Any) => {}
        Ok(other) => checker
            .errors
            .push_err(format!("throw value must be 8-byte-shaped, got {other:?}")),
        Err(e) => checker.errors.push_err(e.clone()),
    }
    if let Ok(t) = t_result
        && !t.is_copy()
    {
        checker.consume_escape(ast, eid);
    }
}
