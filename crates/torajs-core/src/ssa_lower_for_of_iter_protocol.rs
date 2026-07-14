//! `LowerCtx::lower_for_of_iter_protocol` extracted from
//! [`crate::ssa_lower::LowerCtx`] (chunk 143).
//!
//! Pre-extract this method was 218 LOC inline on `LowerCtx` (over
//! the 200-line god-fn hard limit per `torajs-file-size-debt`).
//! Body verbatim moved here as a free fn taking `&mut LowerCtx`;
//! the method-side stays as a thin 3-line delegate so the
//! call-site shape in `Stmt::ForOf` lowering is unchanged.
//!
//! Lowers a P5.3 Phase B `for-of` over a user-class iterator
//! protocol receiver. Conceptually:
//!
//! ```text
//! let __it = obj.__sym_Symbol_iterator__()
//! while (true) {
//!   let __step = __it.next()
//!   if (__step.done) break
//!   let v = __step.value
//!   <body>
//! }
//! ```
//!
//! `src_op` is the receiver value (`Type::Obj(sid)`). `iter_fid`
//! is the resolved `__cm_<src_class>____sym_Symbol_iterator__`.
//! The returned iter is `Type::Obj(iter_sid)`; we look up its
//! class via `aliases` to find `__cm_<iter_class>__next`, and
//! the returned step struct provides `.done` / `.value` via
//! direct field loads. v is marked `moved + borrowed` so the
//! end-of-body drop pass doesn't double-dec the step's owned rc.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{FuncId, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx, OBJ_HEADER_SIZE};

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
struct IterPlan {
    next_fid: FuncId,
    /// Default-value ExprIds for `next`'s params past `__this`.
    next_defaults: Vec<ExprId>,
    /// `next`'s declared param types (leading `__this` included).
    next_param_tys: Vec<Type>,
    step_ret_ty: Type,
    value_ty: Type,
    value_off: u64,
    done_off: u64,
}

/// Resolve the iterator class behind `iter_sid`, its `next` method,
/// and the field offsets of the IteratorResult struct `next` returns.
fn resolve_iter_plan(ctx: &LowerCtx, iter_sid: crate::ssa::StructId) -> IterPlan {
    let mut iter_cname: Option<String> = None;
    for (n, ty) in ctx.aliases.iter() {
        if matches!(ty, Type::Obj(s) if s.0 == iter_sid.0) && ctx.ast.class_parents.contains_key(n)
        {
            iter_cname = Some(n.clone());
            break;
        }
    }
    let Some(iter_cname) = iter_cname else {
        panic!(
            "ssa-lower: for-of protocol — iter class sid={} not in aliases (P5.3 Phase B requires the iter to be a registered user class)",
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
    let step_ret_ty = ctx.f_ret_type_hint(next_fid);
    let Type::Obj(step_sid) = step_ret_ty else {
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
        step_ret_ty,
        value_ty,
        value_off: OBJ_HEADER_SIZE + (value_idx as u64) * 8,
        done_off: OBJ_HEADER_SIZE + (done_idx as u64) * 8,
    }
}

pub(crate) fn lower_for_of_iter_protocol(
    ctx: &mut LowerCtx,
    src_op: Operand,
    iter_fid: FuncId,
    var_name: &str,
    body: &Stmt,
    src_class: &str,
) {
    let iter_ret_ty = ctx.f_ret_type_hint(iter_fid);
    let Type::Obj(iter_sid) = iter_ret_ty else {
        panic!(
            "ssa-lower: for-of protocol on class `{src_class}` — `[Symbol.iterator]()` must return a class instance, got {iter_ret_ty:?}"
        );
    };
    let iter_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(iter_fid, vec![src_op]),
        iter_ret_ty,
        None,
    );
    // `[Symbol.iterator]()` is user code and can throw; what an aborted
    // fn answers is a sentinel, not an iterator (see the `next()` check
    // below).
    ctx.emit_throw_check(None);

    let plan = resolve_iter_plan(ctx, iter_sid);
    let IterPlan {
        next_fid,
        next_defaults,
        next_param_tys,
        step_ret_ty,
        value_ty,
        value_off,
        done_off,
    } = plan;

    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let iter_slot = ctx.alloca(iter_ret_ty, Some("__forof_it"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(iter_val), Operand::Value(iter_slot), 0),
    );

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
    let iter_load = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(iter_ret_ty, Operand::Value(iter_slot), 0),
        iter_ret_ty,
        None,
    );
    let mut next_argv: Vec<Operand> = Vec::with_capacity(1 + next_defaults.len());
    next_argv.push(Operand::Value(iter_load));
    for d in &next_defaults {
        let op = ctx.lower_expr(*d);
        next_argv.push(op);
    }
    // Widen / box each defaulted arg into its declared param lane (an
    // `any`-yield generator's numeric-zero placeholder default has to
    // reach an Any param boxed).
    let coerce_owned = if next_param_tys.len() == next_argv.len() {
        crate::ssa_lower_call_terminal::coerce_args_by_param_tys(
            ctx,
            &next_param_tys[1..],
            &next_defaults,
            &mut next_argv[1..],
        )
    } else {
        Vec::new()
    };
    let step_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(next_fid, next_argv),
        step_ret_ty,
        None,
    );
    for owned in coerce_owned {
        let ty = ctx.operand_ty(&owned);
        ctx.emit_drop_value(owned, ty);
    }
    // ES §7.4.6 IteratorNext — a step that throws forwards the abrupt
    // completion. `next()` is user code (a generator body runs here), and
    // an aborted fn returns the throw sentinel, so reading `.done` off it
    // is a wild deref: `for (const v of gen)` over a generator that throws
    // on its first step was a SIGSEGV.
    ctx.emit_throw_check(None);
    let done_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Bool, Operand::Value(step_val), done_off),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(done_val),
            then_blk: after,
            else_blk: body_blk,
        },
    );

    ctx.cur_block = body_blk;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let v_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(value_ty, Operand::Value(step_val), value_off),
        value_ty,
        None,
    );
    let v_slot = ctx.alloca(value_ty, Some(var_name));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v_val), Operand::Value(v_slot), 0),
    );
    {
        let cur_depth = ctx.scope_stack.len() - 1;
        if let Some(prev) = ctx.locals.get(var_name).copied()
            && prev.scope_depth < cur_depth
        {
            ctx.shadow_stack
                .last_mut()
                .expect("shadow frame")
                .push((var_name.to_string(), prev));
        }
        ctx.locals.insert(
            var_name.to_string(),
            LocalInfo {
                slot: v_slot,
                ty: value_ty,
                moved: true,
                borrowed: true,
                scope_depth: cur_depth,
            },
        );
        ctx.scope_stack
            .last_mut()
            .expect("scope frame")
            .push(var_name.to_string());
    }
    ctx.loop_stack.push((header, after));
    ctx.lower_stmt(body);
    let body_open = ctx.cur_open();
    ctx.loop_stack.pop();
    let step_frame = ctx.scope_stack.pop().expect("for-of-proto body scope");
    let step_shadows = ctx.shadow_stack.pop().expect("shadow frame");
    if body_open {
        for name in &step_frame {
            let info = match ctx.locals.get(name) {
                Some(i) => *i,
                None => continue,
            };
            if info.moved || info.ty.is_copy() || ctx.stack_alloced_locals.contains(name) {
                continue;
            }
            let val = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                info.ty,
                None,
            );
            ctx.emit_drop_value(Operand::Value(val), info.ty);
        }
        ctx.emit_drop_value(Operand::Value(step_val), step_ret_ty);
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    }
    for n in &step_frame {
        ctx.locals.remove(n);
    }
    for (n, prev) in step_shadows {
        ctx.locals.insert(n, prev);
    }

    ctx.cur_block = after;
    let iter_load_drop = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(iter_ret_ty, Operand::Value(iter_slot), 0),
        iter_ret_ty,
        None,
    );
    ctx.emit_drop_value(Operand::Value(iter_load_drop), iter_ret_ty);
    let _ = ctx.scope_stack.pop().expect("for-of-proto iter scope");
    let _ = ctx.shadow_stack.pop().expect("shadow frame");
}
