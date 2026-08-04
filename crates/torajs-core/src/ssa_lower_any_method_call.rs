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
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Any)) {
        // RFC 20260713-array-proto-residual blade 2 — the
        // `<any>.toString.call(x)` family: the member sugar arms
        // type the read as a concrete Function, but the read
        // lowers to a runtime any cell, so .call / .apply / .bind
        // on it ride this any lane (checker mirror in
        // route_early.rs).
        let is_fn_surface = matches!(name.as_str(), "call" | "apply" | "bind")
            && matches!(
                ctx.expr_types.get(obj),
                Some(crate::check::Type::Function(..))
            );
        let sugar_fn_on_any = is_fn_surface
            && matches!(
                ctx.ast.get_expr(*obj),
                Expr::Member { obj: inner, .. }
                    if matches!(ctx.expr_types.get(inner), Some(crate::check::Type::Any))
            );
        // RFC 20260725-str-method-value-reify — the same surfaces on
        // a reified String-receiver method value (a `builtin_mv`
        // binding or the inline `s.slice.call(…)` form) ride the any
        // lane too: the runtime's `call_target` re-dispatches the
        // carried mid with the thisArg as receiver (checker mirror
        // in route_early.rs).
        let builtin_mv = is_fn_surface
            && (matches!(
                ctx.ast.get_expr(*obj),
                Expr::Ident(n) if ctx.builtin_mv_locals.contains_key(n)
            ) || crate::ssa_lower_stmt_let_decl_general::builtin_mv_member_init_mid(ctx, *obj)
                .is_some());
        // Cluster #4 (test262) — a CONCRETE receiver whose member
        // read types Any: the per-family member tables' catch-all
        // answered the read (`arr.hasOwnProperty` / `fn.caller` /
        // an expando property), and the checker's general tail
        // admitted the call as Any (mirror gate — both key on the
        // callee expr's Any record). The receiver boxes at this
        // any-lane boundary below; the runtime dispatcher answers
        // by tag.
        let any_member_read = matches!(ctx.expr_types.get(&callee), Some(crate::check::Type::Any));
        if !sugar_fn_on_any && !builtin_mv && !any_member_read {
            return None;
        }
    }
    let obj = *obj;
    let name = name.clone();
    let mid = any_method_id(&name);
    // C3a-2 — the method name rides along as an interned static Str
    // (rc no-op) so the runtime's dynobj arm can probe user
    // properties by key; built-in ids keep the integer fast path.
    let name_str = ctx.intern_string_literal(&name);

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

    let (argv, boxed_slots) = pack_any_argv(ctx, args);
    let argc = args.len();

    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_call,
            vec![
                recv.clone(),
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
    for slot in boxed_slots.into_iter().flatten() {
        ctx.emit_drop_value(slot, Type::Any);
    }
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

/// Box the call arguments into a stack argv per the chunk-496
/// three-shape ledger (see module doc) — shared by the method-call
/// arm above and the bare any-call arm
/// ([`crate::ssa_lower_any_call`]). Returns the argv alloca plus
/// the slots WE boxed (the caller rc-decs each one after the call).
pub(crate) fn pack_any_argv(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> (crate::ssa::ValueId, Vec<Option<Operand>>) {
    let argc = args.len();
    let argv = ctx.f.append_inst(
        crate::ssa::BlockId(0),
        InstKind::AllocaBytes((argc.max(1) * 8) as u64),
        Type::Ptr,
        Some("__amc_argv"),
    );
    let mut boxed_slots: Vec<Option<Operand>> = Vec::with_capacity(argc);
    for (i, &aid) in args.iter().enumerate() {
        // A regex literal is a BORROW, not a temp — `lower_expr`
        // answers the fn-scope LICM-cached RegExp (hoisted compile,
        // shared by every occurrence of the same `(pattern, flags)`
        // pair), so the box must take its own reference or the
        // post-call dec frees the cached cell out from under the
        // second occurrence. `new RegExp(...)` stays a per-call
        // fresh temp (Expr::New — not matched here).
        let is_borrow = matches!(
            ctx.ast.get_expr(aid),
            Expr::Ident(_) | Expr::Member { .. } | Expr::Regex { .. }
        );
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
            let slot_val = ctx.box_to_any(dynobj);
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(slot_val.clone(), Operand::Value(argv), (i * 8) as u64),
            );
            boxed_slots.push(Some(slot_val));
            continue;
        }
        let raw = ctx.lower_expr(aid);
        let raw_ty = ctx.operand_ty(&raw);
        let (slot_val, we_boxed) = if raw_ty == Type::Any {
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
            if is_borrow && raw_ty.is_refcounted() {
                ctx.emit_rc_inc(raw.clone());
            }
            // Frontend-type-aware boxing — an `undefined` literal
            // argument must ride as ANY_UNDEF, not the ANY_NULL the
            // type-blind ConstPtrNull encoding would pick (bun
            // parity: `s.anchor(undefined)` renders "undefined").
            (ctx.box_to_any_from_expr(aid, raw), true)
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(slot_val.clone(), Operand::Value(argv), (i * 8) as u64),
        );
        boxed_slots.push(if we_boxed { Some(slot_val) } else { None });
    }
    (argv, boxed_slots)
}
