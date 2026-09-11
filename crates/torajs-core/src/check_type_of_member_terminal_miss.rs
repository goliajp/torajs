//! The terminal-miss posture for a member read — what
//! `obj.name` answers once every per-type-family arm in
//! [`crate::check_type_of_member`] has declined.
//!
//! Its sibling answers "which family claims this name"; this file
//! answers "and if none does, then what". Split out at rotation 620,
//! when the inferred-anchor posture pushed the file past the size cap.

use crate::ast::{Ast, ExprId};
use crate::check::Type;

/// Terminal-miss posture — what a member read answers once every
/// family arm has declined. Extracted from `check` (rotation 422,
/// fn-size cap). The postures, in order:
/// - `lenient_missing` (desugar-minted default-guarded load) → Any.
/// - CLASS INSTANCE receiver (RFC 20260804 blade 4) → Any: §10.1.8.1
///   [[Get]] on an absent property answers undefined; the lowering
///   boxes the receiver and rides the any-member lane (an expando /
///   prototype write may have landed the name). A `__priv_<cls>__`
///   miss stays a compile-time reject — §13.1 private references
///   resolve lexically, and the prefix is parser-minted.
/// - ZERO-field anonymous Struct (`var o = {}`, rotation 346) → Any:
///   it declares no surface a typo could miss and only grows through
///   runtime writes.
/// - Namespace-import binding (`import * as ns`, rotation 422) →
///   Undefined: a compiler-synthesized ObjectLit, so the
///   anonymous-struct typo posture does not apply — §10.4.6.8 [[Get]]
///   on a non-exported name answers undefined, and namespaces are not
///   extensible, so the static answer IS the runtime answer.
/// - NON-empty anonymous Struct keeps the loud reject: its fields ARE
///   statically known, so a miss is overwhelmingly a typo (recorded
///   diagnostic-posture boundary).
/// The three yes/no facts the terminal answer needs from the checker.
/// One struct rather than three bools: the call takes four arguments
/// instead of six, and a caller cannot transpose them.
pub(crate) struct MissFacts {
    /// The receiver's family says a miss is not an error here.
    pub(crate) lenient_missing: bool,
    /// We are checking a synthesized spec body.
    pub(crate) spec_body: bool,
    /// `name` is a `__priv_` reference some class body really
    /// declares — see [`crate::check_private_name`].
    pub(crate) declared_private: bool,
}

pub(crate) fn answer(
    ast: &Ast,
    obj: &ExprId,
    name: &str,
    obj_ty: &Type,
    facts: &MissFacts,
) -> Result<Type, String> {
    let MissFacts {
        lenient_missing,
        spec_body,
        declared_private,
    } = *facts;
    if lenient_missing {
        return Ok(Type::Any);
    }
    // NULLISH receiver (`undefined.toString` / `null.constructor`)
    // → Any: §13.2.3 GetValue on a nullish base is a RUNTIME
    // TypeError, reached when the read evaluates — not a compile
    // stop. The lowering boxes the receiver into the any-member /
    // any-method lane, whose kernels raise the catchable TypeError.
    if obj_ty.is_nullish_value() {
        return Ok(Type::Any);
    }
    if matches!(obj_ty, Type::ClassRef(_)) {
        if name.starts_with("__priv_") {
            // A STATIC private reached through an instance receiver is
            // legal code whose brand check fails at RUN time (§7.3.32)
            // — it misses the instance member list for the same reason
            // an undeclared name does, so the list is not the question
            // to ask. See `check_private_name`.
            if declared_private {
                return Ok(Type::Any);
            }
            let field = crate::check_private_name::raw_private_name(name);
            return Err(format!(
                "private field `#{field}` is not declared in this class"
            ));
        }
        return Ok(Type::Any);
    }
    if matches!(obj_ty, Type::Struct(f) if f.is_empty()) {
        return Ok(Type::Any);
    }
    if matches!(obj_ty, Type::Struct(_))
        && let crate::ast::Expr::Ident(n) = ast.get_expr(*obj)
        && ast.namespace_bindings.contains_key(n)
    {
        return Ok(Type::Undefined);
    }
    // Object-rest binding (`{a, ...rest} = o; rest.a`) → Any: the
    // rest object's Struct shape is only the source-minus-omit
    // anchor; the object is ordinary and extensible, so the miss is
    // a runtime [[Get]] — the lowering boxes the receiver onto the
    // any-member probe (hit → value, true miss → undefined).
    if matches!(obj_ty, Type::Struct(_))
        && let crate::ast::Expr::Ident(n) = ast.get_expr(*obj)
        && ast.obj_rest_names.contains(n)
    {
        return Ok(Type::Any);
    }
    // ECMASCRIPT GLOBAL OBJECT receiver (`Math.nope` /
    // `Reflect.enumerate`) → Any; tr's host handles keep the loud
    // reject. Rationale in the sibling's module doc.
    if let Type::Object(tag) = obj_ty
        && crate::check_type_of_member_global_miss::answers_undefined(tag)
    {
        return Ok(Type::Any);
    }
    // A monomorph specialization body's Struct receiver is an
    // INFERRED anchor, not a declared shape: `function f(o)` says
    // nothing about `o`, and the anchor is whatever one call site
    // passed. §10.1.8.1 [[Get]] on an absent property answers
    // undefined, and the lowering already implements that here (the
    // layout miss boxes the receiver onto the any-member probe) — so
    // the reject was never enforced, only DISCARDED
    // (`check_monomorph_uninferred`). A discarded error is worse than
    // no error: `type_of` records a type only on Ok, so it left this
    // read and every expression the `?` skipped past it with no entry
    // in `expr_types`, and the lowering reads exactly that table to
    // decide truthiness, `=== undefined` vs `=== null`, array element
    // and object field types. `f({x:9})` on `o.q ? a : b` took the
    // TRUE branch, and `o.q && x` answered false.
    //
    // A DECLARED `function f(o: {x: number})` keeps the loud reject —
    // that shape is a contract, and tsc refuses `o.q` on it too. The
    // zero-field arm above is this same posture already, unconditional
    // because `{}` declares no surface a typo could miss.
    //
    // The same holds for a PRIMITIVE anchor, and for the same reason:
    // `function f(n)` says nothing about `n`, and §10.1.8.1 [[Get]]
    // on a primitive's absent property answers undefined. Only the
    // Struct shape was admitted, so `f(1).nope` and `f(true).nope`
    // stayed rejected — and, being a specialization body, REJECTED
    // meant a blank in `expr_types`, which the member ladder read as
    // "not an object" and the call cascade as "unsupported member
    // call shape: nope". `(1 as any).nope` and `(1 as any).nope()`
    // answered undefined and threw the right TypeError all along.
    if spec_body
        && matches!(
            obj_ty,
            Type::Struct(_)
                | Type::Number
                | Type::Boolean
                | Type::BigInt
                | Type::String
                | Type::Symbol
        )
    {
        return Ok(Type::Any);
    }
    Err(format!("no member `.{name}` on type {obj_ty:?}"))
}
