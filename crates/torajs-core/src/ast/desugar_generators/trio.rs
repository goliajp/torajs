//! The `next()` / `return()` / `throw()` methods of a `__Gen_*` class,
//! moved out of `desugar_generators` for file size.

use crate::ast::{Ast, ClassMethod, Stmt};

/// The `next()` / `return()` / `throw()` trio every `__Gen_*` class
/// carries, built together because they share the step-record
/// annotation and the yield type.
///
/// J.4 — `next()` takes an optional `__yield_arg: <yield_ty> =
/// <zero>` parameter and stashes it in `this.__sent` before
/// re-entering the state machine. YieldInto-expanded
/// `let v = this.__sent` sites read that field to receive the value
/// passed to `g.next(arg)`.
///
/// The default is NOT `default_init_for_type(&yield_ty)` for the any
/// lane, even though §27.5.1.1.1 says a bare `next()` sends
/// `undefined` and an `any` slot could hold it. `apply_default_args`
/// groups method defaults by NAME ("next"), so every `__Gen` class's
/// `__yield_arg` default has to stay shape-uniform — the first-seen
/// class's default ExprId pads every `it.next()` call site whose
/// receiver it cannot resolve, and an any-lane `undefined` leaks
/// into a number-lane `next()` there. The visible cost is that the
/// two spellings contradict each other: `it.next()` sends `0` where
/// `it.next(undefined)` sends `undefined`.
///
/// Splitting the lanes has been tried twice and does not work
/// piecemeal — rotations 305 and 306, both reverted, both measured;
/// see RFC 20260805-generator-value-channels § "Splitting the lanes".
/// The lanes come together only by making BOTH able to carry
/// `undefined`, which is that RFC's subject.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_iterator_method_trio(
    ast: &mut Ast,
    yield_ty: &str,
    step_ann: &str,
    next_body: Vec<Stmt>,
    has_try_regions: bool,
    has_finally_ret: bool,
    is_async_gen: bool,
) -> (ClassMethod, ClassMethod, ClassMethod) {
    let next_name = if is_async_gen {
        crate::ast::desugar_generators_async::MACHINE_NEXT
    } else {
        "next"
    };
    let yield_arg_default_id = ast.add_expr(
        crate::ast::desugar_generators_methods::yield_arg_undefined(yield_ty),
    );
    let next_method = crate::ast::desugar_generators_methods::build_next_method(
        ast,
        yield_arg_default_id,
        yield_ty,
        step_ann,
        next_body,
    );
    let return_method = crate::ast::desugar_generators_methods::build_return_method(
        ast,
        yield_ty,
        step_ann,
        has_finally_ret,
        is_async_gen,
        next_name,
    );
    let throw_method = crate::ast::desugar_generators_methods::build_throw_method(
        ast,
        yield_ty,
        step_ann,
        has_try_regions,
        next_name,
    );
    (next_method, return_method, throw_method)
}
