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
use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type};
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
    /// What the `next()` Call instruction itself answers — under
    /// `for await` over an async generator that is `Type::Promise`,
    /// and `step_ret_ty` is what it settles to.
    next_call_ty: Type,
    step_ret_ty: Type,
    value_ty: Type,
    value_off: u64,
    done_off: u64,
}

/// Resolve the iterator class behind `iter_sid`, its `next` method,
/// and the field offsets of the IteratorResult struct `next` returns.
/// One §7.4.6 IteratorNext — build `next()`'s argument list, call it,
/// and let a throw out.
///
/// `next()` is user code (a generator body runs here), and an aborted
/// fn returns the throw sentinel, so reading `.done` off the result
/// before the check is a wild deref: `for (const v of gen)` over a
/// generator that throws on its first step was a SIGSEGV.
#[allow(clippy::too_many_arguments)]
fn emit_next_call(
    ctx: &mut LowerCtx,
    iter_load: crate::ssa::ValueId,
    next_fid: FuncId,
    next_defaults: &[ExprId],
    next_param_tys: &[Type],
    next_call_ty: Type,
    step_ret_ty: Type,
) -> crate::ssa::ValueId {
    let mut next_argv: Vec<Operand> = Vec::with_capacity(1 + next_defaults.len());
    next_argv.push(Operand::Value(iter_load));
    for d in next_defaults {
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
            next_defaults,
            &mut next_argv[1..],
        )
    } else {
        Vec::new()
    };
    let step_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(next_fid, next_argv),
        next_call_ty,
        None,
    );
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    // ES §7.4.6 IteratorNext — a step that throws forwards the abrupt
    // completion. `next()` is user code (a generator body runs here), and
    // an aborted fn returns the throw sentinel, so reading `.done` off it
    // is a wild deref: `for (const v of gen)` over a generator that throws
    // on its first step was a SIGSEGV.
    ctx.emit_throw_check(None);
    if next_call_ty == step_ret_ty {
        return step_val;
    }
    // §14.7.5.10 — the async form awaits the step before reading it.
    crate::ssa_lower_for_of_iter_protocol_await::emit_await_step(ctx, step_val, step_ret_ty)
}

/// The loop's shared exit — §7.4.9 IteratorClose gated on how the loop
/// stopped, then the iterator's own release.
///
/// Both exits arrive at one block, so the close cannot be decided by
/// predecessor; `done_slot` carries which one it was (0 = stopped
/// early). This lane keeps its iterator in a TYPED local rather than
/// an `Any` park slot, so it closes through the value-shaped face —
/// the box is rc-neutral and the stake stays in the slot, released
/// below either way. An iterator declaring no `return` closes as a
/// no-op.
fn emit_loop_exit(
    ctx: &mut LowerCtx,
    after: crate::ssa::BlockId,
    iter_slot: crate::ssa::ValueId,
    done_slot: crate::ssa::ValueId,
    iter_ret_ty: Type,
) {
    ctx.cur_block = after;
    let close_blk = ctx.f.add_block();
    let release_blk = ctx.f.add_block();
    let done_flag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(done_slot), 0),
        Type::I64,
        None,
    );
    let stopped_early = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(done_flag), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(stopped_early),
            then_blk: close_blk,
            else_blk: release_blk,
        },
    );

    let teardown = crate::ssa_lower_for_of_teardown::ForOfTeardown::Typed {
        iter_slot,
        iter_ret_ty,
    };
    ctx.cur_block = close_blk;
    crate::ssa_lower_for_of_teardown::emit_close(ctx, &teardown);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(release_blk));

    ctx.cur_block = release_blk;
    crate::ssa_lower_for_of_teardown::emit_release(ctx, &teardown);
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

fn resolve_iter_plan(
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

pub(crate) fn lower_for_of_iter_protocol(
    ctx: &mut LowerCtx,
    src_op: Operand,
    iter_fid: FuncId,
    var_name: &str,
    body: &Stmt,
    src_class: &str,
    is_await: bool,
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

    let plan = resolve_iter_plan(ctx, iter_sid, src_class, is_await);
    let IterPlan {
        next_fid,
        next_defaults,
        next_param_tys,
        next_call_ty,
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

    // ES §7.4.9 — a `return()` is owed only on an EARLY stop. Both
    // exits share `after`, so the natural one records itself and the
    // close is gated on the flag; a `break` leaves it 0. Mirror of the
    // `any` lane's shape (`ssa_lower_for_of_any_iter`).
    let done_slot = ctx.alloca(Type::I64, Some("__forof_proto_done"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(done_slot), 0),
    );

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let exhausted = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
    let iter_load = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(iter_ret_ty, Operand::Value(iter_slot), 0),
        iter_ret_ty,
        None,
    );
    let step_val = emit_next_call(
        ctx,
        iter_load,
        next_fid,
        &next_defaults,
        &next_param_tys,
        next_call_ty,
        step_ret_ty,
    );
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
            then_blk: exhausted,
            else_blk: body_blk,
        },
    );

    // The iterator reported done — it closed itself.
    ctx.cur_block = exhausted;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(1), Operand::Value(done_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));

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
    // RFC 20260901-scope-exit-drops — body frame already pushed and
    // only closed on fall-through: a jump out owes it (depth = index).
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: header,
            brk: after,
            scope_depth: ctx.scope_stack.len() - 1,
        });
    ctx.for_of_teardown_stack
        .push(crate::ssa_lower_for_of_teardown::ForOfTeardown::Typed {
            iter_slot,
            iter_ret_ty,
        });
    ctx.lower_stmt(body);
    ctx.for_of_teardown_stack.pop();
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

    emit_loop_exit(ctx, after, iter_slot, done_slot, iter_ret_ty);
    let _ = ctx.scope_stack.pop().expect("for-of-proto iter scope");
    let _ = ctx.shadow_stack.pop().expect("shadow frame");
}
