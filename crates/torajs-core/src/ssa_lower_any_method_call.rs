//! `recv.name(args…)` where the receiver types as `any` —
//! Any-method-call RFC 20260704 C1 lowering.
//!
//! The checker's route_early arm admits the call as `any`; this
//! dispatcher arm (first in `ssa_lower_call::lower`'s cascade)
//! packs it for the runtime method dispatcher:
//!
//! - method NAME interns to an `ANY_METHOD_*` id at compile time
//!   (torajs-rc `any_method_id`) — the runtime switches on an
//!   integer for the built-in Str/Arr arms; the name also rides as
//!   an interned static Str (C3a-2) so the dynobj arm probes user
//!   properties (`o.f(x)`) by key.
//! - each argument boxes to a NaN-box AnyValue into a stack argv
//!   (`AllocaBytes(argc*8)` in the entry block). Ledger per the
//!   chunk-496 three-shape rule: `box_to_any` is TRANSFER, so a
//!   borrowed operand (Ident / Member) rc-incs first; temps hand
//!   their reference to the slot; already-`any` operands pass
//!   through unboxed (borrowed, not dec'd after).
//! - after the call every slot WE boxed rc-decs — the runtime
//!   borrows argv (per-method glue incs what it keeps), so the
//!   dec releases exactly the box's reference.
//! - `recv_slot`: an Ident receiver's variable slot pointer rides
//!   along so growth-relocating methods (push) write the fresh
//!   block pointer back; every other receiver shape passes NULL.

mod admit;
mod argv;

use admit::non_any_recv_admitted;
pub(crate) use argv::{pack_any_argv, pack_any_argv_padded};
use torajs_rc::any_method_id;

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `callee(args…)` as an any-receiver method call.
/// Returns `None` unless the callee is a Member read off an
/// `any`-typed object.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    // Rotation 591 — §13.3.6.2 reads the base off the REFERENCE, and
    // a type assertion does not consume one: `(o.f as any)()` calls
    // with `this === o` exactly as `o.f()` does. The shape test
    // therefore peels `As`, while every TYPE test below keeps the
    // unpeeled `callee` — the assertion is what widened the read to
    // Any, and that widening is the admission this lane wants.
    let shape = ctx.peel_as_wrappers(callee);
    let Expr::Member { obj, name } = ctx.ast.get_expr(shape) else {
        return None;
    };
    // A NULLISH receiver (`undefined.toString()`) rides this lane
    // too: the checker admitted the site (§13.2.3 GetValue is a
    // runtime TypeError) and the recv box below carries it into the
    // kernel, whose nullish arm raises the catchable TypeError.
    let recv_is_nullish = ctx
        .expr_types
        .get(obj)
        .is_some_and(|t| t.is_nullish_value());
    if !recv_is_nullish
        && !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Any))
        && !non_any_recv_admitted(ctx, callee, obj, name)
    {
        return None;
    }
    let obj = *obj;
    let name = name.clone();

    let recv = ctx.lower_expr(obj);
    // Backfill chunk 2 — an `xs as any` receiver reaches here as a
    // typed-Arr SSA value (the As cast is a pass-through for heap
    // values), so this call IS its typed→any coercion boundary:
    // mark the elem kind or the kind-aware arms (fill / splice /
    // concat spread) see UNSET. Self-gates on the operand's SSA
    // type — already-Any receivers no-op.
    ctx.emit_arr_mark_kind(&recv);
    // RFC 20260725-str-method-value-reify — a typed Closure receiver
    // (reified method-value binding on its .call/.apply/.bind) boxes
    // at this any-lane boundary. Cluster #4 widened this to EVERY
    // non-Any receiver (Arr / Str / f64 / fn addr — the concrete
    // receiver of a catch-all-Any member call). Borrow-shaped box
    // (box_to_any is RC-NEUTRAL, the kernel borrows the receiver) —
    // no release.
    let recv = if matches!(ctx.operand_ty(&recv), Type::Any) {
        recv
    } else {
        ctx.box_to_any(recv)
    };
    // Rotation 550 — an owned receiver (`mk(i).toString(boom())`) is
    // live across every argument's lower; park it so an arg's throw
    // path releases it (40MB / 600k caught throws before).
    let recv_tok = ctx.park_owned_temp(obj, &recv);
    // Ident receivers ride their variable slot along so
    // growth-relocating methods write the fresh pointer back —
    // local alloca or K.3 top-level global slot (the same two
    // shapes as index-assign's WriteBack).
    let recv_slot = if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        if let Some(info) = ctx.locals.get(n) {
            Operand::Value(info.slot)
        } else if ctx.globals.contains_key(n) {
            let name = n.clone();
            let gref = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
            Operand::Value(gref)
        } else {
            Operand::ConstPtrNull
        }
    } else {
        Operand::ConstPtrNull
    };

    let result = emit_any_method_call(ctx, recv, &name, recv_slot, args);
    ctx.unpark_owned_temp(recv_tok);
    // A Call-shaped receiver (`s.bold().italics()`) is an owned Any
    // temp the runtime only borrowed — release it or every chained
    // hop leaks its intermediate (the optcall sibling already keeps
    // this account). Borrow shapes (Ident / Member) self-gate false.
    ctx.release_owned_temp(obj, &recv);
    // The result is an OWNED Any already in hand — the throw path
    // must release it (mint-and-throw kernels like non-`g` matchAll
    // answer a fresh cell alongside the pending TypeError; the
    // catch can't know about it).
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Some(Operand::Value(result))
}

/// The dispatcher call itself over an already-boxed receiver — the
/// method-call arm above and the typed constructor-static lane
/// (`ssa_lower_call_typedarray_static`, whose receiver is the boxed
/// interned ctor cell) share it. `recv` is borrowed by the kernel;
/// the answer is an OWNED Any the caller's throw check releases on
/// the propagate edge.
pub(crate) fn emit_any_method_call(
    ctx: &mut LowerCtx<'_>,
    recv: Operand,
    name: &str,
    recv_slot: Operand,
    args: &[ExprId],
) -> crate::ssa::ValueId {
    let mid = any_method_id(name);
    // C3a-2 — the method name rides along as an interned static Str
    // (rc no-op) so the runtime's dynobj arm can probe user
    // properties by key; built-in ids keep the integer fast path.
    let name_str = ctx.intern_string_literal(name);
    let packed = pack_any_argv(ctx, args);
    let argv = packed.argv;
    let argc = args.len();
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_call,
            vec![
                recv,
                Operand::ConstI64(mid),
                Operand::Value(name_str),
                Operand::ConstI64(0),
                recv_slot,
                Operand::Value(argv),
                Operand::ConstI64(argc as i64),
            ],
        ),
        Type::Any,
        None,
    );
    // Release the boxes' references BEFORE the throw check (the
    // runtime has returned — argv is dead either way, and the
    // throw-propagate branch must not leak the boxes). The runtime
    // borrowed argv; per-method glue inc'd whatever it stored.
    packed.release(ctx);
    result
}
