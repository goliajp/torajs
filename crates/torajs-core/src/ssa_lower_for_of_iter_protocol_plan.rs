//! The iterator plan behind a user-class `for-of`: which class the
//! `@@iterator` method returns, its `next` and how to call it, and
//! where `value` / `done` sit in the IteratorResult struct it answers.
//! Moved verbatim out of [`crate::ssa_lower_for_of_iter_protocol`]
//! (rotation 551 file-size); the loop emit stays there.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{FuncId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

/// Default-value ExprIds of the iterator's `next` beyond the leading
/// `__this` param. The call-site default-padding pass (`apply_default_args`)
/// only rewrites AST `it.next()` calls; this protocol emits the call
/// straight at SSA, so a `next` with defaulted params (a desugared
/// generator's `next(__yield_arg = 0)`, which stashes the value passed
/// to `g.next(v)`) would be invoked one argument short. for-of never
/// sends a value, so the declared default IS the argument per
/// ES §27.5.3.2 (`next()` with no arg).
fn next_param_defaults(ctx: &LowerCtx, next_fn: &str) -> Vec<ExprId> {
    for s in &ctx.ast.stmts {
        let Stmt::FnDecl { name, params, .. } = s else {
            continue;
        };
        if name != next_fn {
            continue;
        }
        return params
            .iter()
            .skip(1)
            .map(|p| {
                p.default.unwrap_or_else(|| {
                    panic!(
                        "ssa-lower: for-of protocol — `{next_fn}` param `{}` has no default; an iterator's next() must be callable with no arguments",
                        p.name
                    )
                })
            })
            .collect();
    }
    Vec::new()
}

/// Everything the loop emit needs about the resolved iterator: its
/// `next` FuncId + the call shape to invoke it with, and where
/// `value` / `done` sit inside the IteratorResult struct it returns.
pub(crate) struct IterPlan {
    pub(crate) next_fid: FuncId,
    /// Default-value ExprIds for `next`'s params past `__this`.
    pub(crate) next_defaults: Vec<ExprId>,
    /// `next`'s declared param types (leading `__this` included).
    pub(crate) next_param_tys: Vec<Type>,
    /// What the `next()` Call instruction itself answers — under
    /// `for await` over an async generator that is `Type::Promise`,
    /// and `step_ret_ty` is what it settles to.
    pub(crate) next_call_ty: Type,
    pub(crate) step_ret_ty: Type,
    pub(crate) value_ty: Type,
    pub(crate) value_off: u64,
    pub(crate) done_off: u64,
}

/// The class an `@@iterator` method RETURNS, by name.
///
/// RFC 20260715-nominal-class-identity — a class instance names its
/// class. This used to scan the alias table for any entry whose
/// `Type::Obj` StructId matched, which is structural: two classes with
/// the same field shape share a StructId, so the scan picked whichever
/// the HashMap happened to yield first and the SAME program compiled
/// to a different iterator between runs. The declared return type of
/// the `@@iterator` method is the nominal answer, and the generator
/// desugar fills it in too (§27.5.1.5 — a generator is its own
/// iterable, so its method returns its own class).
fn iter_class_name(ctx: &LowerCtx, src_class: &str) -> Option<String> {
    let iter_fn = format!("__cm_{src_class}____sym_Symbol_iterator__");
    for st in &ctx.ast.stmts {
        let Stmt::FnDecl {
            name, return_type, ..
        } = st
        else {
            continue;
        };
        if name != &iter_fn {
            continue;
        }
        let declared = return_type.as_deref()?;
        return ctx
            .ast
            .class_parents
            .contains_key(declared)
            .then(|| declared.to_string());
    }
    None
}

pub(crate) fn resolve_iter_plan(
    ctx: &mut LowerCtx,
    iter_sid: crate::ssa::StructId,
    src_class: &str,
    is_await: bool,
) -> IterPlan {
    let Some(iter_cname) = iter_class_name(ctx, src_class) else {
        panic!(
            "ssa-lower: for-of protocol — `{src_class}[Symbol.iterator]()` must declare the iterator class it returns (sid={} resolved to no registered class)",
            iter_sid.0
        );
    };
    let next_fn = format!("__cm_{iter_cname}__next");
    let Some(&next_fid) = ctx.fn_table.get(&next_fn) else {
        panic!(
            "ssa-lower: for-of protocol — iter class `{iter_cname}` must declare `next(): IteratorResult<T>` (fn `{next_fn}` not registered)"
        );
    };
    let next_defaults = next_param_defaults(ctx, &next_fn);
    let next_param_tys: Vec<Type> = ctx
        .fn_sig_ids
        .get(&next_fid)
        .map(|sid| ctx.fn_sigs[sid.0 as usize].0.clone())
        .unwrap_or_default();
    let next_call_ty = ctx.f_ret_type_hint(next_fid);
    // §27.6 — an async generator's steps answer Promises. Under
    // `for await` that is expected and gets unwrapped per step;
    // without it the loop is a plain for-of over an object with no
    // `[Symbol.iterator]`, which is a TypeError in JS, so say which
    // form was needed rather than reporting the Promise as a shape
    // surprise.
    let awaited = (is_await && next_call_ty == Type::Promise)
        .then(|| crate::ssa_lower_for_of_iter_protocol_await::awaited_step_ty(ctx, &next_fn))
        .flatten();
    let step_ret_ty = awaited.unwrap_or(next_call_ty);
    let Type::Obj(step_sid) = step_ret_ty else {
        if next_call_ty == Type::Promise {
            panic!(
                "ssa-lower: for-of protocol — `{iter_cname}.next()` answers a Promise (an async iterator); the loop must be written `for await (... of ...)`"
            );
        }
        panic!(
            "ssa-lower: for-of protocol — `{iter_cname}.next()` must return an IteratorResult-shaped struct, got {step_ret_ty:?}"
        );
    };

    let step_layout = &ctx.struct_layouts[step_sid.0 as usize];
    let value_field = step_layout
        .iter()
        .enumerate()
        .find(|(_, (n, _))| n == "value");
    let done_field = step_layout
        .iter()
        .enumerate()
        .find(|(_, (n, _))| n == "done");
    let Some((value_idx, (_, value_ty))) = value_field.map(|(i, p)| (i, p.clone())) else {
        panic!(
            "ssa-lower: for-of protocol — step struct missing `value` field (got {step_layout:?})"
        );
    };
    let Some((done_idx, (_, done_ty))) = done_field.map(|(i, p)| (i, p.clone())) else {
        panic!(
            "ssa-lower: for-of protocol — step struct missing `done` field (got {step_layout:?})"
        );
    };
    if !matches!(done_ty, Type::Bool) {
        panic!("ssa-lower: for-of protocol — step.done must be boolean, got {done_ty:?}");
    }
    IterPlan {
        next_fid,
        next_defaults,
        next_param_tys,
        next_call_ty,
        step_ret_ty,
        value_ty,
        value_off: OBJ_HEADER_SIZE + (value_idx as u64) * 8,
        done_off: OBJ_HEADER_SIZE + (done_idx as u64) * 8,
    }
}
