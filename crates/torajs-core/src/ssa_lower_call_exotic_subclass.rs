//! Exotic-subclass factory magics (RFC
//! 20260730-exotic-backed-class-instance blades 1-2).
//!
//! An exotic-parent class's factory mints a REAL builtin cell through
//! a zero-arg `__torajs_<builtin>_subclass_alloc_self()` magic — the
//! class resolves from the enclosing `__new_<C>` fn name (the same
//! channel `write_class_tag` reads) — and its ctor's `super(v)`
//! rewrites to the builtin's one-argument semantics kernel. Both
//! shapes are shared across builtins; only the runtime kernel FuncId
//! (and Array's extra `len` operand) differ per tag, resolved here.

use crate::ast::ExprId;
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Route an exotic-subclass magic by name; `None` = not one of ours
/// (the class-synth dispatcher keeps walking).
pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, name: &str, args: &[ExprId]) -> Option<Operand> {
    match name {
        "__torajs_arr_subclass_alloc_self" => {
            let f = ctx.intrinsics.arr_subclass_alloc;
            lower_alloc_self(ctx, args, f, true)
        }
        "__torajs_number_wrapper_subclass_alloc_self" => {
            let f = ctx.intrinsics.number_wrapper_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_string_wrapper_subclass_alloc_self" => {
            let f = ctx.intrinsics.string_wrapper_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_boolean_wrapper_subclass_alloc_self" => {
            let f = ctx.intrinsics.boolean_wrapper_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_function_subclass_alloc_self" => {
            let f = ctx.intrinsics.function_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_map_subclass_alloc_self" => {
            let f = ctx.intrinsics.map_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_set_subclass_alloc_self" => {
            let f = ctx.intrinsics.set_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_promise_subclass_alloc_self" => {
            let f = ctx.intrinsics.promise_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_promise_subclass_super" => {
            let f = ctx.intrinsics.promise_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_map_subclass_super" => {
            let f = ctx.intrinsics.map_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_set_subclass_super" => {
            let f = ctx.intrinsics.set_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_regex_subclass_alloc_self" => {
            let f = ctx.intrinsics.regex_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_regex_subclass_super" => {
            let f = ctx.intrinsics.regex_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_weakmap_subclass_alloc_self" => {
            let f = ctx.intrinsics.weakmap_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_weakset_subclass_alloc_self" => {
            let f = ctx.intrinsics.weakset_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_date_subclass_alloc_self" => {
            let f = ctx.intrinsics.date_subclass_alloc;
            lower_alloc_self(ctx, args, f, false)
        }
        "__torajs_weakmap_subclass_super" => {
            let f = ctx.intrinsics.weakmap_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_weakset_subclass_super" => {
            let f = ctx.intrinsics.weakset_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_date_subclass_super" => {
            let f = ctx.intrinsics.date_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_arr_subclass_super_len" => {
            let f = ctx.intrinsics.arr_subclass_super_len;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_arr_subclass_super_elems" => {
            let f = ctx.intrinsics.arr_subclass_super_elems;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_date_subclass_super_components" => {
            let f = ctx.intrinsics.date_subclass_super_components;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_number_wrapper_subclass_super" => {
            let f = ctx.intrinsics.number_wrapper_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_string_wrapper_subclass_super" => {
            let f = ctx.intrinsics.string_wrapper_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        "__torajs_boolean_wrapper_subclass_super" => {
            let f = ctx.intrinsics.boolean_wrapper_subclass_super;
            lower_super_one_arg(ctx, args, f)
        }
        _ => None,
    }
}

/// The factory's mint site: resolve the class from the enclosing
/// `__new_<C>` fn, call the per-builtin subclass-alloc kernel (which
/// resolves the prototype from the classmeta registry and answers the
/// boxed instance). Array's kernel carries an extra initial-length
/// operand (always 0 — `super(len)` resizes afterwards).
fn lower_alloc_self(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    kernel: FuncId,
    with_len: bool,
) -> Option<Operand> {
    if !args.is_empty() {
        return None;
    }
    let tag = ctx
        .f
        .name
        .strip_prefix("__new_")
        .and_then(|cname| ctx.class_name_to_tag.get(cname).copied())?;
    let mut call_args = vec![Operand::ConstI64(tag as i64)];
    if with_len {
        call_args.push(Operand::ConstI64(0));
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(kernel, call_args),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

/// Ctor-side `super(v)` — both operands any-world (the kernel runs
/// the builtin ctor's coercion: ToNumber-and-validate for Array len
/// per §23.1.2.1, `[[*Data]] = To*(v)` for the wrappers).
fn lower_super_one_arg(ctx: &mut LowerCtx<'_>, args: &[ExprId], kernel: FuncId) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let this_raw = ctx.lower_expr(args[0]);
    let this_op = ctx.box_to_any_from_expr(args[0], this_raw);
    let val_raw = ctx.lower_expr(args[1]);
    let val_op = ctx.box_to_any_from_expr(args[1], val_raw);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(kernel, vec![this_op, val_op]),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}
