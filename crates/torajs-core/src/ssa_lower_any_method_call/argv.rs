//! The stack argv an any-lane call boxes its arguments into.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_any_argv::AnyArgv;

/// Box the call arguments into a stack argv per the chunk-496
/// three-shape ledger (see module doc) — shared by the method-call
/// arm above and the bare any-call arm
/// ([`crate::ssa_lower_any_call`]). Returns the argv alloca plus
/// the slots WE boxed, each parked for the throw edges of the later
/// args' lowers; the caller's [`AnyArgv::release`] drops them after
/// the call (rotation 550).
pub(crate) fn pack_any_argv(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> AnyArgv {
    pack_any_argv_padded(ctx, args, 0)
}

/// [`pack_any_argv`] with the buffer padded to at least `min_slots`
/// slots, the extra ones holding `undefined` — the fixed-width argv a
/// boxed dual entry reads its param count out of unconditionally.
/// The runtime dispatcher supplies that padding itself; a direct
/// entry call (551-03 closure retarget) has to bring it.
pub(crate) fn pack_any_argv_padded(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    min_slots: usize,
) -> AnyArgv {
    let argc = args.len();
    let argv = ctx.f.append_inst(
        crate::ssa::BlockId(0),
        InstKind::AllocaBytes((argc.max(min_slots).max(1) * 8) as u64),
        Type::Ptr,
        Some("__amc_argv"),
    );
    for i in argc..min_slots {
        let undef_box = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BoxAny(5, Operand::ConstI64(0)),
            Type::Any,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(
                Operand::Value(undef_box),
                Operand::Value(argv),
                (i * 8) as u64,
            ),
        );
    }
    let mut packed = AnyArgv::new(argv, argc);
    for (i, &aid) in args.iter().enumerate() {
        let (slot_val, we_boxed) = lower_any_slot(ctx, aid);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(slot_val, Operand::Value(argv), (i * 8) as u64),
        );
        packed.push_slot(ctx, if we_boxed { Some(slot_val) } else { None });
    }
    packed
}

/// One argument's argv slot value, and whether the box is ours to
/// release after the call (an owned stake) or a borrow that rides
/// verbatim.
pub(crate) fn lower_any_slot(ctx: &mut LowerCtx<'_>, aid: ExprId) -> (Operand, bool) {
    // RFC 20260717-objlit-anylane-recv knife 2g — an inline
    // ObjectLit argument at an any-lane call site rides the
    // dynobj lane (mirror of the direct-call promotions:
    // stmt_let_decl / call_terminal 62b46f13 / setPrototypeOf
    // fa05bb71). Lowered as a struct it reaches the dispatcher
    // as a TAG_OBJ cell the Any-gated kernels misdecode
    // (`f = Object.entries; f({x:5})` answered `[null]`). The
    // fresh dynobj is owned and its one stake rides the box
    // (box_to_any's Ptr arm is a pure encode), so the slot goes
    // through boxed_slots and the caller's post-call drop
    // releases it. Gated on `objlit_promotable`: a literal
    // carrying a nominal-`this` face (method or accessor) keeps
    // the pre-existing struct route — promoting it would hit
    // lower_dynobj_init's loud-reject guard and turn working
    // struct-lane shapes into compile errors (gate regression
    // error-proto-tostring-001).
    if matches!(ctx.ast.get_expr(aid), Expr::ObjectLit { .. }) && ctx.objlit_promotable(aid) {
        let dynobj = ctx.lower_dynobj_init(aid);
        return (ctx.box_to_any(dynobj), true);
    }
    let raw = ctx.lower_expr(aid);
    let raw_ty = ctx.operand_ty(&raw);
    // Who owns the heap this slot points at is NOT a question
    // this site keeps its own roster for. The roster it did keep
    // — `Ident | Member | Regex` — read `c as any` as a shape it
    // had never heard of and therefore as a fresh temp, so the
    // box took no reference and the post-call release freed the
    // binding's only stake: `buf.resize(c as any)` handed the
    // next allocation `c`'s bytes, and the read after it saw
    // whatever moved in (rotation 572; `Object.keys(c)`'s result
    // array answering as `c.valueOf()` is what named it). The
    // shared answer peels casts, and it asks AFTER the lower so
    // the owned-member-read records exist.
    //
    // A regex literal is the one shape where this site differs:
    // `lower_expr` hands back the fn-scope LICM-cached RegExp
    // (hoisted compile, shared by every occurrence of the same
    // `(pattern, flags)` pair), so the box must take its own
    // reference or the post-call dec frees the cached cell out
    // from under the second occurrence. `new RegExp(...)` stays
    // a per-call fresh temp (Expr::New — not matched here).
    let is_borrow = !ctx.expr_is_fresh_owned(aid)
        || matches!(
            ctx.ast.get_expr(ctx.peel_as_wrappers(aid)),
            Expr::Regex { .. }
        );
    if raw_ty == Type::Any {
        // An operand that is already an Any rides verbatim — the
        // runtime borrows argv, so nothing is boxed here. It can
        // still be OWNED: an any-member read mints its result,
        // and so does an inner any-call, and with no post-call
        // release that stake belongs to nobody. The one that made
        // it visible was `new Promise(executor)`, whose desugar
        // passes the settle pair as `__ex(__pr.resolve,
        // __pr.reject)` — ~885 bytes stranded per mint, because
        // each leaked closure holds an env that holds the cell.
        // Binding the same read to a `const` first never leaked,
        // which is what a missing temp release looks like.
        let owned = ctx.expr_is_fresh_owned(aid);
        (raw, owned)
    } else {
        // Frontend-type-aware boxing — an `undefined` literal
        // argument must ride as ANY_UNDEF, not the ANY_NULL the
        // type-blind ConstPtrNull encoding would pick (bun
        // parity: `s.anchor(undefined)` renders "undefined").
        (ctx.box_owned_from_expr(aid, raw, is_borrow), true)
    }
}
