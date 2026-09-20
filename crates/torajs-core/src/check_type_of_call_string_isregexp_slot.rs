//! §22.1.3.{7,8,23} step 3-5 — the needle slot of
//! `String.prototype.{startsWith,endsWith,includes}`, which is the one
//! search slot in this family that is NOT a plain §7.1.17 ToString
//! slot: `IsRegExp` runs FIRST and a regex raises a TypeError before
//! any coercion happens.
//!
//! [`crate::ssa_lower_str_str_tostring_slot`] names the ToString
//! family and says in its own doc why these three are kept off it.
//! What that exclusion left behind was not a different road but NO
//! road: the slot fell through to the raw `lower_expr` and handed
//! whatever it got to a helper whose ABI declares `Str`. This module
//! is the other half — the same "a rule written in one place and
//! applied to half of what it covers" shape the last three rotations
//! kept meeting, here in its third form: the family was named, and
//! the members excluded from it were never given anywhere else to go.
//!
//! ## Which band each spelling lands in
//!
//! `docs/language-spec/ch1-type-runtime-boundary.md` sorts an
//! operation whose spec step throws into three bands, and this slot
//! populates two of them:
//!
//! - **Band B (typed, throw decided at compile time)** — a needle
//!   statically known to be a `RegExp` or a `Symbol`. The spec HAS a
//!   runtime semantics here (it throws), and the type is already in
//!   hand, so the answer is an unconditional raise with no runtime
//!   test at all. Refusing the program instead would be band A, and
//!   band A is only for what the spec gives no runtime meaning.
//! - **Band C (any lane)** — the type is genuinely unknown, so the
//!   `IsRegExp` walk and the coercion both happen at runtime.
//!
//! Band B is what this module decides. It records the throw in
//! `Ast::spec_throws`; `ssa_lower_spec_throw::try_emit_call` emits it
//! and evaluates the operands first, because §13.3.6.1 evaluates them
//! before the call whether or not the call throws.
//!
//! The two messages are the ones the runtime raises for the same
//! input, so what moves is WHEN the decision is made, not what the
//! program prints: the symbol text is `coerce_to_str`'s own
//! (`Cannot convert a symbol to a string`), and the regex text is the
//! per-method one.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, DiagPush, Type};

/// The three methods whose needle slot runs `IsRegExp` before
/// `ToString`, in the order the kernel's message table is keyed by.
/// `indexOf` / `lastIndexOf` / `search` are deliberately absent:
/// their spec steps coerce straight away, which is why they sit on
/// the plain ToString road instead.
///
/// This array is the ONE place the family is listed. The band-B arm
/// below and the lowering's runtime arm both read it, so a method
/// added here reaches the compile-time decision and the runtime road
/// together — which is the whole failure this module exists to stop.
const NEEDLE_METHODS: [&str; 3] = ["startsWith", "endsWith", "includes"];

/// `Some(which)` when argument `i` of `<Str>.<method>(..)` is the
/// IsRegExp-then-ToString needle slot. `which` is the index
/// `__torajs_anyv_reject_regexp_needle` uses to pick its message, so
/// the two sides agree by construction rather than by review.
pub(crate) fn isregexp_slot_which(method: &str, i: usize) -> Option<i64> {
    if i != 0 {
        return None;
    }
    NEEDLE_METHODS
        .iter()
        .position(|m| *m == method)
        .map(|p| p as i64)
}

/// The §22.1.3.{7,8,23} step-5 TypeError text for a regex needle,
/// worded per method the way the engines word it. The kernel spells
/// the same three strings as `CStr` literals for the runtime arm.
fn regexp_needle_msg(method: &str) -> String {
    format!("Argument to String.prototype.{method} cannot be a RegExp")
}

/// §7.1.17 — `ToString` of a Symbol throws. Shared with the lowering
/// so the two spellings of the same failure cannot drift apart.
pub(crate) const SYMBOL_TO_STRING_MSG: &str = "Cannot convert a symbol to a string";

/// Band B for a needle whose type already settles the throw: answer
/// `Some(Boolean)` with the raise recorded, or `None` when the needle
/// is some other type and the ordinary lanes still own the call.
///
/// The remaining arguments are typed here even though the call
/// throws — §13.3.6.1 evaluates them first, so their side effects are
/// observable and the lowering reads `expr_types` to shape each one.
pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if isregexp_slot_which(m_name, 0).is_none() || args.is_empty() || args.len() > 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let needle_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let msg = match needle_ty {
        Type::RegExp => regexp_needle_msg(m_name),
        Type::Symbol => SYMBOL_TO_STRING_MSG.to_string(),
        _ => return None,
    };
    for &a in &args[1..] {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    checker.spec_throws.insert(eid, msg.clone());
    checker.errors.push_warn(msg);
    Some(Ok(Type::Boolean))
}
