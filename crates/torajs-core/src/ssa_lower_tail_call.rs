//! A call in tail position (§15.10.1 IsInTailPosition) through the
//! any lane: `return f(args…)` in a strict function body, where `f`
//! is an `any` value.
//!
//! §15.10.3 PrepareForTailCall wants the caller's frame gone before
//! the callee runs. The ordinary any-call keeps it: its argv is a
//! frame alloca, the callee value and the boxes it made are released
//! after the call, and the runtime dispatcher calls the adapter from
//! a frame of its own. Here the frame's owned locals are released
//! before the call, the arguments go to the runtime's
//! `__torajs_tail_argv`, `__torajs_any_tail_entry` resolves the entry
//! (told whether the site holds a reference on the callee), and the
//! call is the last thing the function does, so codegen leaves
//! through it with a jump (compile/sibcall.rs).
//!
//! Every body borrows its arguments, so nothing the site owns can be
//! handed over: an argument qualifies only when it needs no release —
//! a box of an immediate, or a template object the site cache keeps.
//! Any other argument keeps the ordinary call.
//!
//! The callee is borrowed the same way. A binding whose holder
//! outlives the frame — a parameter the caller passed, a capture the
//! closure's env keeps, the function's own name, a module global — is
//! handed over without a reference, as the ordinary call does.
//! Anything the frame holds (a local, a call's result) would die with
//! the frame while the callee runs, so the site keeps its reference
//! and the resolver answers a shim that makes the call and releases
//! it afterwards. A count above one proves nothing here: the other
//! holder can let go during the call.

use crate::ast::{Expr, ExprId};
use crate::ssa::{
    ANY_SLOT_TAG_HEAP, BlockId, IPred, InstKind, Operand, TAIL_ARGV_SYM, TAIL_ENTRY_SYM,
    THROW_ACTIVE_SYM, Terminator, Type, ValueId,
};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_any_argv::AnyArgv;
use crate::ssa_lower_any_method_call::lower_any_slot;

/// The boxed adapters read this many argv slots.
const MAX_TAIL_ARGS: usize = 8;

/// The tail-position call the `return` being lowered hands to the any
/// lane, and whether the lane took it (its owned locals are released
/// and the call is in place).
#[derive(Default)]
pub(crate) struct TailSite {
    pub(crate) call: Option<ExprId>,
    pub(crate) taken: bool,
    /// The slots of the bindings whose holder outlives the frame:
    /// borrowed parameters, captures and the function's own name.
    pub(crate) outer: std::collections::HashSet<ValueId>,
}

/// Record that the binding in `slot` borrows from a holder that
/// outlives the frame (see the module doc).
pub(crate) fn note_outer_slot(ctx: &mut LowerCtx<'_>, slot: ValueId) {
    ctx.temps.tail.outer.insert(slot);
}

/// Whether `callee` names a binding whose holder outlives the frame:
/// a recorded parameter / capture / own-name slot, or a module global
/// no local shadows.
fn callee_outlives_frame(ctx: &LowerCtx<'_>, callee: ExprId) -> bool {
    let Expr::Ident(name) = ctx.ast.get_expr(callee) else {
        return false;
    };
    match ctx.locals.get(name) {
        Some(info) => ctx.temps.tail.outer.contains(&info.slot),
        None => ctx.globals.contains_key(name),
    }
}

/// The return value, offering the call it makes to the any lane's
/// tail-position form; `true` when the lane took it, having released
/// the frame's owned locals before the call.
pub(crate) fn lower_return_value(
    ctx: &mut LowerCtx<'_>,
    maybe: Option<ExprId>,
    lower: fn(&mut LowerCtx<'_>, ExprId) -> Operand,
) -> (Option<Operand>, bool) {
    ctx.temps.tail.call = maybe.and_then(|eid| site(ctx, eid));
    let v = maybe.map(|eid| lower(ctx, eid));
    ctx.temps.tail.call = None;
    (v, std::mem::take(&mut ctx.temps.tail.taken))
}

/// The call `return <eid>` makes in tail position, when the lane can
/// take it: a strict, synchronous body returning `any`, no enclosing
/// try / finally / for-of, and arguments whose types can box to
/// immediates (the lowered values are checked in [`emit`]) or a
/// tagged template's site object.
pub(crate) fn site(ctx: &LowerCtx<'_>, eid: ExprId) -> Option<ExprId> {
    if ctx.f.ret != Type::Any
        || ctx.is_main_fn
        || ctx.suspend_body
        || !ctx.try_stack.is_empty()
        || !ctx.try_finally_stack.is_empty()
        || !ctx.for_of_teardown_stack.is_empty()
    {
        return None;
    }
    let Expr::Call { callee, args } = ctx.ast.get_expr(eid) else {
        return None;
    };
    let fits = args.len() <= MAX_TAIL_ARGS
        && crate::ast::source_argc(ctx.ast, eid, args) <= MAX_TAIL_ARGS
        && !matches!(ctx.ast.get_expr(*callee), Expr::Member { .. })
        && args.iter().all(|&a| arg_type_fits(ctx, a));
    (fits && !crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx)).then_some(eid)
}

fn arg_type_fits(ctx: &LowerCtx<'_>, a: ExprId) -> bool {
    use crate::check::{Lit, Type as T};
    if is_template_object(ctx, a) {
        return true;
    }
    matches!(
        ctx.expr_types.get(&a),
        Some(
            T::Any
                | T::Number
                | T::Boolean
                | T::Null
                | T::Undefined
                | T::Literal(Lit::Num(_) | Lit::Bool(_))
        )
    )
}

fn is_template_object(ctx: &LowerCtx<'_>, a: ExprId) -> bool {
    matches!(ctx.ast.get_expr(a), Expr::Call { callee, .. }
        if matches!(ctx.ast.get_expr(*callee),
            Expr::Ident(n) if n == crate::parser::TEMPLATE_OBJECT_CALLEE))
}

/// Lower the any-call `eid` the `return` marked. Answers the call's
/// result; the lane is taken unless an argument's lowered value turns
/// out to need a release, in which case the ordinary call is emitted
/// over the arguments already in hand.
pub(crate) fn emit(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    recv: Operand,
    args: &[ExprId],
) -> Operand {
    ctx.temps.tail.call = None;
    let recv_tok = ctx.park_owned_temp(callee, &recv);
    let mut slots = Vec::with_capacity(args.len());
    let mut parked = Vec::new();
    for &aid in args {
        let (v, owned) = lower_any_slot(ctx, aid);
        if owned {
            parked.push(ctx.push_throw_temp(v, Type::Any));
        }
        slots.push((v, owned));
    }
    let argc = crate::ast::source_argc(ctx.ast, eid, args) as i64;
    if !slots
        .iter()
        .all(|&(v, owned)| needs_no_release(ctx, v, owned))
    {
        let argv = ctx.f.append_inst(
            BlockId(0),
            InstKind::AllocaBytes((args.len().max(1) * 8) as u64),
            Type::Ptr,
            Some("__amc_argv"),
        );
        store_slots(ctx, &slots, argv);
        let boxed = slots.iter().map(|&(v, owned)| owned.then_some(v)).collect();
        let packed = AnyArgv::parked(argv, boxed, parked);
        return crate::ssa_lower_any_call::finish(ctx, callee, recv, recv_tok, packed, argc);
    }
    for t in parked {
        ctx.pop_throw_temp(t);
    }
    ctx.unpark_owned_temp(recv_tok);
    // a callee the frame holds keeps the site's reference through the
    // call (a borrowed one is retained before the locals let go of
    // it); one held outside the frame is borrowed as any call's is
    let borrowed = ctx.owned_temp_ty(callee, &recv).is_none();
    let owned = !(borrowed && callee_outlives_frame(ctx, callee));
    if owned && borrowed {
        let b = ctx.cur_block;
        ctx.emit_any_rc_inc_in(b, recv);
    }
    ctx.emit_drops_for_owned_locals();
    let argv = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(TAIL_ARGV_SYM.into()),
        Type::Ptr,
        None,
    );
    store_slots(ctx, &slots, argv);
    let resolver = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(TAIL_ENTRY_SYM.into()),
        Type::Ptr,
        None,
    );
    let resolve = crate::ssa_lower::intern_fn_sig(
        ctx.fn_sigs,
        vec![Type::Any, Type::I64, Type::I64],
        Type::Ptr,
    );
    let owned = Operand::ConstI64(owned as i64);
    let entry = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(
            resolve,
            Operand::Value(resolver),
            vec![recv, Operand::ConstI64(argc), owned],
        ),
        Type::Ptr,
        None,
    );
    let sig = crate::ssa_lower::intern_fn_sig(
        ctx.fn_sigs,
        vec![Type::Any, Type::Ptr, Type::I64],
        Type::Any,
    );
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(
            sig,
            Operand::Value(entry),
            vec![recv, Operand::Value(argv), Operand::ConstI64(argc)],
        ),
        Type::Any,
        None,
    );
    emit_bare_probe(ctx);
    ctx.temps.tail.taken = true;
    Operand::Value(result)
}

fn store_slots(ctx: &mut LowerCtx<'_>, slots: &[(Operand, bool)], argv: ValueId) {
    for (i, &(v, _)) in slots.iter().enumerate() {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(v, Operand::Value(argv), (i * 8) as u64),
        );
    }
}

/// The pending-throw probe after the call, whose propagate edge only
/// signals: the frame's locals were released before the call, and a
/// callee that throws answers a value nobody owns.
fn emit_bare_probe(ctx: &mut LowerCtx<'_>) {
    let b = ctx.cur_block;
    let g = ctx.f.append_inst(
        b,
        InstKind::GlobalRef(THROW_ACTIVE_SYM.into()),
        Type::Ptr,
        None,
    );
    let flag = ctx.f.append_inst(
        b,
        InstKind::Load(Type::I64, Operand::Value(g), 0),
        Type::I64,
        None,
    );
    let test = InstKind::ICmp(IPred::Ne, Operand::Value(flag), Operand::ConstI64(0));
    let c = ctx.f.append_inst(b, test, Type::Bool, None);
    let throw_blk = ctx.f.add_block();
    let cont = ctx.f.add_block();
    ctx.f.set_term(
        b,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk: throw_blk,
            else_blk: cont,
        },
    );
    ctx.cur_block = throw_blk;
    ctx.emit_propagate_ret();
    ctx.cur_block = cont;
}

/// Whether the slot holds nothing the site would release after the
/// call: an owned box of an immediate (any-lane arithmetic with an
/// immediate operand is a Number, or undefined with a throw pending,
/// §13.15.3), or a borrowed site template object.
fn needs_no_release(ctx: &LowerCtx<'_>, v: Operand, owned: bool) -> bool {
    let Operand::Value(x) = v else {
        return false;
    };
    let mut defs = (ctx.f.blocks.iter())
        .flat_map(|b| b.insts.iter())
        .filter(|i| i.result == Some(x));
    let (Some(def), None) = (defs.next(), defs.next()) else {
        return false;
    };
    match &def.kind {
        InstKind::BoxAny(t, _) => *t != ANY_SLOT_TAG_HEAP,
        InstKind::BoxInt32(_) | InstKind::BoxF64(_) => true,
        InstKind::Call(f, a) if *f == ctx.intrinsics.any_arith => {
            let imm = |i: usize| matches!(a.get(i), Some(Operand::ConstI64(t)) if *t != ANY_SLOT_TAG_HEAP);
            a.len() == 5 && (imm(1) || imm(3))
        }
        InstKind::Call(f, _) => !owned && *f == ctx.intrinsics.template_object_end,
        _ => false,
    }
}
