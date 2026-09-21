//! §7.2 middle tier at a builtin's callback slot — the argument is
//! statically known not to be callable, and the spec has a runtime
//! answer for that.
//!
//! `[].find({})` and `xs.sort(null)` are legal JavaScript. §23.1.3.9
//! step 1 and §23.1.3.30 step 1 both say IsCallable, and both say
//! *throw a TypeError* when it is false — at run time, after the
//! receiver and the arguments have evaluated. tr refused them at
//! compile time instead, which is the static-subset answer, not this
//! language's.
//!
//! Neither is it the type-erasure answer. bun has to build the
//! callback, reach the call, ask IsCallable and only then know; tr
//! knows while compiling that `{}` is not a function, and emits the
//! throw with no check at all. That is the third answer
//! `docs/language-spec/ch1-type-runtime-boundary.md` §1 describes,
//! applied to a callback slot rather than to `1n & 1`.
//!
//! The message is the one the runtime kernel raises for the same
//! call, so the two spellings of the same program agree: writing the
//! receiver `as any` sends it down the any lane, where
//! `callback_resolve::resolve` answers None and `method_call`'s
//! `not_callable` prints exactly this.
//!
//! What counts as "statically not callable" is deliberately the
//! narrow set: [`Type::is_uncallable_value`] — the same predicate
//! the callee position already uses for `true()` / `/re/()` — plus
//! the structural object type and the array type, which is what an
//! object or array literal argument types as. `Any` is unknown and
//! belongs to the any lane; `Function` is callable; a class object
//! reaches a callback slot as something none of these name, and
//! stays a refusal until it has a measured answer.
//!
//! **And the method has to be named, because the slot type does not
//! decide this.** §23.1.3.x says *throw*; §27.2.5.4 says the exact
//! opposite — a `then` handler that is not callable is REPLACED by
//! the identity function, and `p.then({})` is a promise that
//! resolves. A predicate reading only `(param_ty, arg_ty)` would
//! have turned that refusal into a wrong throw, which is the trade
//! this whole area exists to refuse. So the list below is the set
//! of methods whose own clause reads "If IsCallable(callbackfn) is
//! false, throw a TypeError exception", and nothing rides along.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;

/// The runtime's own words for a value that resolved and cannot be
/// invoked (`torajs_anyvalue::method_call::not_callable`), so the
/// two spellings of one program agree: write the receiver `as any`
/// and the any lane prints exactly this.
const MSG: &str = "value is not a function on this any receiver";

/// `Some(message)` when this call is certain to reach a spec step
/// that asks IsCallable of an argument and to fail it.
pub(super) fn message_for(
    ast: &Ast,
    callee: ExprId,
    param_ty: &Type,
    arg_ty: &Type,
) -> Option<String> {
    if !matches!(param_ty, Type::Function(..)) {
        return None;
    }
    let Expr::Member { name, .. } = ast.get_expr(callee) else {
        return None;
    };
    if !matches!(
        name.as_str(),
        "map"
            | "filter"
            | "forEach"
            | "every"
            | "some"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "flatMap"
            | "reduce"
            | "reduceRight"
            | "sort"
            | "toSorted"
    ) {
        return None;
    }
    // §23.1.3.30 step 1 / §23.1.3.34 name `undefined` explicitly as
    // "no comparator", so those two do not throw on it. (Their own
    // arm answers that case before this one is reached; the row is
    // here so reading this predicate alone cannot mislead.)
    if matches!(arg_ty, Type::Undefined | Type::Void)
        && matches!(name.as_str(), "sort" | "toSorted")
    {
        return None;
    }
    let certain =
        arg_ty.is_uncallable_value() || matches!(arg_ty, Type::Struct(_) | Type::Array(_));
    if !certain {
        return None;
    }
    Some(MSG.to_string())
}
