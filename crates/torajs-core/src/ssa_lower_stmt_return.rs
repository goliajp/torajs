//! `Stmt::Return` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 149).
//!
//! Pre-extract this arm was 168 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! Pipeline:
//!
//! 1. **Lower the optional return value** with borrowed-ident
//!    retain (Swift ARC +0-parameter / +1-result) — `return s` for
//!    a non-Copy borrowed local takes a +1 stake so the caller's
//!    scope-end drop doesn't release the owner's reference.
//! 2. **Mark every non-Copy local touched by the return expression
//!    as moved** via `consume_all_idents_in_return` so the end-of-fn
//!    drop walk doesn't dangle the returned pointer.
//! 3. **Try-with-finally routing** (review #0001) — if a finally is
//!    active, stash the value in a lazy-alloc'd `__pending_ret`
//!    slot + set `__pending_flag`, then `br` to the innermost
//!    finally. The finally tail dispatches: still-wrapped → br next
//!    outer; outermost → load + ret.
//! 4. **Boundary coercions** (no finally on the stack):
//!    - Any-typed return slot with concrete value → `box_to_any`
//!      (P0.9 — ABI).
//!    - Any-typed value returned where declared return is numeric
//!      → `coerce_any_to_number` (P7.2b — fixes B-throw-2's
//!      pointer-as-primitive corruption).
//!    - i64 ↔ f64 promotion / narrowing per declared return.
//!    - Substr → Str materialization (caller may rely on declared
//!      return type for slot layout — e.g. flatMap's dst_elem_ty).
//!    - Array<Substr> → Array<Str> materialization (symmetric).
//!    - Array<Any> → Array<T> decode (S8.5), the same one the
//!      let-decl lane has paid since chunk 698.
//! 5. **Void-return guard** — arrow expression-body desugar wraps
//!    a trailing void Call in `Stmt::Return(Some(eid))`; the dummy
//!    operand must not feed `Terminator::Ret` if the fn is void
//!    (LLVM verify rejects `ret i64 0` from a void fn).
//! 6. Emit owned-local drops + set the block terminator to
//!    `Ret(coerced)`.

use crate::ast::Expr;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// The return-VALUE leg of [`lower`] — the annotation-consuming
/// widen / declared-layout pin, the borrowed-read retain family
/// (borrowed locals, escape-boxed bindings, K.3 globals, Arr elem
/// reads), and the consume walk. Extracted verbatim when the parent
/// crossed the 200-line cap (file-size debt, rotation 343).
fn lower_return_value(ctx: &mut LowerCtx, eid: crate::ast::ExprId) -> Operand {
    // `Arr<Any>`-ret fn takes the annotation-consuming widen
    // (mirror of lower_let_init_val / assign_member's chunk 614
    // arm): without it the literal lowered through the typed
    // fast path, the block never got FLAG_ARR_ANY, and every
    // kind-aware Arr<Any> reader saw UNSET (undefined) while its
    // raw scalar slots misread as NaN-boxes elsewhere.
    let v = if let Expr::Array(els) = ctx.ast.get_expr(eid)
        && let Type::Arr(arr_id) = ctx.f.ret
        && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
    {
        let ids: Vec<crate::ast::ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else {
        // A direct ObjectLit returned from a declared-Obj fn pins
        // the declared layout, same as the let-decl chunk 780 arm:
        // without the hint resolve_objlit_layout registers an anon
        // layout off the field exprs' own types (`{ value: x }`
        // with `x: any` got an Any slot), the caller reads the
        // slot at the declared width, and the box bits misread as
        // a raw scalar (`{ value: number }` return answered NaN;
        // a generator's step struct hit the same lane through the
        // desugared `return { value, done }`).
        if let Expr::ObjectLit { .. } = ctx.ast.get_expr(eid)
            && let Type::Obj(sid) = ctx.f.ret
        {
            ctx.let_declared_obj_layout = Some(sid);
        }
        let v = ctx.lower_expr(eid);
        ctx.let_declared_obj_layout = None;
        v
    };
    // Peel value-transparent `As` wrappers before judging ownership.
    // `lower_as_cast` hands back the inner operand untouched for a
    // heap source and for an already-Any one, so the INNER read is
    // what decides who holds the stake — the same peel
    // `expr_is_fresh_owned` (rotation 384) and
    // `ssa_lower_object_lit::lower_regular_field` already do. Judging
    // the `As` node instead skipped BOTH legs at once: no retain for a
    // borrowed / boxed / global read, and no moved mark for an owned
    // local, so `return s as any` let the fn-exit drop walk release
    // the very cell it had just handed the caller. Two decs against
    // one stake — `const s = "ab" + "cd"; return s as any` read back
    // the next allocation's bytes (`x199`) on the second use, where
    // the uncast `return s` is correct.
    let mut src_eid = eid;
    while let Expr::As { expr, .. } = ctx.ast.get_expr(src_eid) {
        src_eid = *expr;
    }
    let needs_retain = if let Expr::Ident(name) = ctx.ast.get_expr(src_eid) {
        ctx.locals
            .get(name)
            .is_some_and(|info| info.borrowed && info.ty.is_refcounted())
            // Rotation 326 — an escape-boxed binding in its
            // OWNING frame (`info.borrowed == false`; the box
            // holds the payload's stake). Reading the ident
            // answers the box's payload as a borrow, and the
            // frame's exit still drops the whole box — so a
            // `return fib` handed the caller the payload while
            // the box release charged its only stake (a
            // self-referential escaping closure was the census
            // shape: zero incs, two decs on the env cell).
            || (ctx.boxed_noncopy_lets.contains(name)
                && ctx
                    .locals
                    .get(name)
                    .is_some_and(|info| info.ty.is_refcounted()))
            // Cluster #4 follow-up (rotation 235) — a K.3 global
            // slot read is ALWAYS a borrow (pure GlobalRef+Load,
            // the slot keeps its stake), so returning it takes
            // the same +1 the borrowed-local arm pays: the
            // caller owns every return value. Without it a
            // discarded `f()` freed the cell under the live slot
            // (Symbol probe e1b SIGSEGV; the Str shape was the
            // same double-dec that only happened not to crash).
            || (ctx.locals.get(name).is_none()
                && ctx
                    .globals
                    .get(name)
                    .is_some_and(|t| t.is_refcounted()))
    } else {
        false
    };
    if needs_retain {
        ctx.emit_rc_inc(v.clone());
    }
    // A join (`?:`, `??`, `&&`, `||`) over borrows answers a borrow of
    // whichever arm ran, and the consume walk marks none of the arms'
    // locals moved (it cannot know which one leaves). The caller owns
    // every return value, so the answer takes its own stake here and
    // both arms keep their scope drops. A join its lowering already
    // unified to owned (`expr_transfers_ownership`) carries that stake.
    let is_join = matches!(
        ctx.ast.get_expr(src_eid),
        Expr::Ternary { .. } | Expr::Nullish { .. }
    ) || matches!(ctx.ast.get_expr(src_eid), Expr::BinOp { op, .. }
        if matches!(op, crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr));
    if is_join && !ctx.expr_transfers_ownership(src_eid) {
        let ty = ctx.operand_ty(&v);
        ctx.emit_owned_result_inc(v.clone(), ty);
    }
    // RFC 20260708-closure-argv-face (generalized chunk 674) —
    // `return a[i]` where `a` is a local Arr<Any> hands back a
    // box BORROWING the array's elem stake (get_any_boxed is a
    // borrow read). The historical blanket fix marked `a` moved
    // — skipping its scope drop and stranding one array per
    // call (probe ag8: named fn `return a[0]` leaked; the
    // materialized `__torajs_arguments` was the same shape).
    // Retain the payload instead: the box leaves self-owned,
    // the array keeps its scope drop, and nothing takes a moved
    // mark (the index is read, never handed out). Gated on the
    // receiver's static Arr<Any> type — other Any-producing
    // index lanes may answer owned boxes where a retain would
    // leak.
    if let Expr::Index { obj, .. } = ctx.ast.get_expr(src_eid)
        && let Expr::Ident(obj_name) = ctx.ast.get_expr(*obj)
        && ctx.operand_ty(&v) == Type::Any
        && ctx.locals.get(obj_name).is_some_and(|info| {
            matches!(info.ty, Type::Arr(id)
                if ctx.arr_layouts[id.0 as usize] == Type::Any)
        })
    {
        let retained = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_retain, vec![v.clone()]),
            Type::Any,
            None,
        );
        return Operand::Value(retained);
    }
    // Rotation 326 — the typed sibling of the retain above:
    // `return xs[i]` on a typed Arr<T> with refcounted T answers
    // the slot's value as a BORROW (load_dyn reads the element
    // in place; the slot keeps its stake), while the fn-return
    // contract hands the caller an owned value. Without the +1
    // the synthesized boxed entry boxed the borrow into `any`
    // (owned by NaN-box contract) and the caller's drop stole
    // the slot's stake (census: `() => bs[0]` on a `bigint[]`
    // global underflowed the live element). Local and global
    // receivers alike; Str/Substr receivers never reach here
    // (s[i] answers a fresh owned Substr), and the Any-elem
    // shape took the anyv_retain arm above.
    if let Expr::Index { obj, .. } = ctx.ast.get_expr(src_eid) {
        let v_ty = ctx.operand_ty(&v);
        let elem_matches = matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Array(_)))
            && v_ty.is_refcounted()
            && !matches!(v_ty, Type::Any | Type::Substr);
        if elem_matches {
            ctx.emit_rc_inc(v.clone());
            return v;
        }
    }
    // Chunk 752 — a Copy-typed result (scalar) cannot alias any
    // local's heap, so no binding needs the moved mark; the
    // blanket walk stranded every non-Copy local the expression
    // touched (`return v.length` skipped v's scope drop and
    // leaked one concat cell per call — probe vJ 15.97MB vs
    // 6.37MB flat; the L3b #6 any-alias framing was refuted by
    // the typed-string variant leaking identically).
    if !ctx.operand_ty(&v).is_copy() {
        ctx.consume_all_idents_in_return(src_eid);
    }
    v
}

pub(crate) fn lower(ctx: &mut LowerCtx, maybe: Option<crate::ast::ExprId>) {
    // A moved mark says "THIS return hands the local to the caller",
    // and it is a compile-time flag on the binding — left standing it
    // also silenced the local's drop at every LATER return, on paths
    // that never came through this one (`if (c) return s; return t;`
    // stranded `s` whenever `c` was false: 59 MB vs 1.6 MB flat over
    // 200k, strings alike). So the marks this return makes are undone
    // once its own drops are emitted; nothing runs after a `return`.
    let unmoved: Vec<String> = ctx
        .locals
        .iter()
        .filter(|(_, info)| !info.moved)
        .map(|(name, _)| name.clone())
        .collect();
    let ret_operand = maybe.map(|eid| lower_return_value(ctx, eid));
    // §7.4.9 on the way out of every enclosing for-of. `break` gets
    // this from the loop's exit block; a return never reaches that
    // block, so the iterator stayed open AND its slot stake stayed
    // held. Emitted after the return value is in hand — the value can
    // read the loop variable — and before the finally hand-off below.
    //
    // A `return` inside a try-with-finally closes only the for-ofs
    // INSIDE that try here; the ones enclosing the try close in the
    // finally tail's pending-return dispatch, after the finally body
    // (§14.7.5.7 step 6: IteratorClose runs on the completion the
    // loop body produced, and a try statement's completion is the
    // finally's). Closing them here put the close's throw inside the
    // try — its catch swallowed the iterator's `return()` throw and
    // the loop went on (rotation 652: `for (x of it) { try { return }
    // catch {} }` with a throwing `return()` never terminated). Same
    // split `break` / `continue` already make (`emit_pending_loop_flag`).
    match ctx.try_finally_stack.last().copied() {
        Some(target) => {
            crate::ssa_lower_for_of_teardown::emit_closes_from(ctx, target.teardown_depth, false)
        }
        None => crate::ssa_lower_for_of_teardown::emit_all_for_return(ctx),
    }
    let mut coerced = ret_operand.map(|op| coerce_to_ret(ctx, op, maybe));
    // A bare `return;` in a value-returning fn answers `undefined`
    // (§10.2.1.4 — [[Call]] of a completion with no value). The
    // mixed-return shape (`if (c) return; return v;`) infers an Any
    // ret, and emitting `Ret(None)` there handed the caller a
    // garbage register read as a NaN-box — the rc teardown on that
    // fake cell SIGSEGV'd (probe thj / JSON.parse reviver
    // `if (k === 'b') return;`).
    if coerced.is_none() && ctx.f.ret == Type::Any {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        coerced = Some(Operand::Value(v));
    }
    let coerced = if ctx.f.ret == Type::Void {
        None
    } else {
        coerced
    };
    // The finally hand-off is an exit like any other, so it stores the
    // COERCED value: the slot is `ctx.f.ret`-wide, and reading it back
    // in `emit_pending_return` loads at that width. Storing the raw
    // operand let a `return 1` inside a try land an I64 constant in an
    // F64 slot — codegen has no way to materialise that and aborted.
    if !ctx.try_finally_stack.is_empty() {
        let target = *ctx.try_finally_stack.last().unwrap();
        let ret_ty = ctx.f.ret;
        // A Void function parks no value — `Alloca(Void)` is not
        // allocable, and a bare `return;` inside a try whose finally
        // still has to run (a derived constructor's `catch { return }
        // finally { super() }`, test262 derived-class-return-override-
        // *) died on exactly that alloca. The flag alone carries the
        // pending return; `emit_pending_return` answers `Ret(None)`
        // when there is no slot.
        let slot = match (ctx.pending_return_slot, ret_ty) {
            (Some(s), _) => Some(s),
            (None, Type::Void) => None,
            (None, _) => {
                // Entry-allocated for the same reason as the flag
                // below: the lazy alloca lands in whichever block held
                // the `return`, and that block does not dominate the
                // finally tail once a path exists that reaches the
                // tail WITHOUT returning. No initialiser needed — the
                // flag gates every read of this slot.
                let s = ctx.alloca_in_entry(ret_ty, Some("__pending_ret"));
                ctx.pending_return_slot = Some(s);
                Some(s)
            }
        };
        let flag = match ctx.pending_return_flag {
            Some(f) => f,
            None => {
                // Entry-allocated and zeroed, like its `__pending_break`
                // / `__pending_continue` siblings: the finally tail
                // reads this flag on the fall-through path too, which
                // the lazy alloca in whichever block held the `return`
                // neither dominates nor initialises. Latent until the
                // fall-through path became reachable — it used to be
                // terminated `unreachable`.
                let f = ctx.alloca_bool_flag_in_entry(Some("__pending_flag"));
                ctx.pending_return_flag = Some(f);
                f
            }
        };
        if let (Some(v), Some(slot)) = (coerced, slot) {
            ctx.f
                .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(slot), 0));
        }
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstBool(true), Operand::Value(flag), 0),
        );
        // RFC 20260901-scope-exit-drops — the value is parked, so the
        // frames inside the try body (or catch) die now; the frames
        // between the finally and the fn boundary are the finally
        // tail's to release when it completes the return.
        ctx.emit_drops_for_scopes_from(target.scope_depth);
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(target.blk));
        return;
    }
    ctx.emit_drops_for_owned_locals();
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Ret(coerced));
    // Not on the finally hand-off above: the frames outside the try
    // are released by the finally tail, one site shared by every
    // return routed through it, which still needs the mark.
    for name in unmoved {
        if let Some(info) = ctx.locals.get_mut(&name) {
            info.moved = false;
        }
    }
}

/// Step 4's boundary coercions, lifted out of `lower` to keep it
/// under the 200-line limit (the S8.5 arm was what pushed it over).
/// `maybe` is the returned expression's id, needed by the two arms
/// that consult the source expression rather than only its type.
fn coerce_to_ret(ctx: &mut LowerCtx, op: Operand, maybe: Option<crate::ast::ExprId>) -> Operand {
    let actual = ctx.operand_ty(&op);
    if ctx.f.ret == Type::Any && actual != Type::Any {
        // Chunk 806 — the expr-aware variant tags an Undefined-
        // typed source ANY_UNDEF; the plain box encoded its
        // ConstPtrNull payload as ANY_NULL, so `return undefined`
        // (and void-call returns) printed `null` at the caller.
        if let Some(eid) = maybe {
            return ctx.box_to_any_from_expr(eid, op);
        }
        return ctx.box_to_any(op);
    }
    if actual == Type::Any && matches!(ctx.f.ret, Type::I64 | Type::F64) {
        return ctx.coerce_any_to_number(op, ctx.f.ret);
    }
    if actual == Type::Any && matches!(ctx.f.ret, Type::Closure(_)) {
        // 403-03 — an Any box crossing a fn-typed return boundary
        // (`effective_ret_ty`'s any-binding upgrade). The kernel
        // passes a Closure cell through with the box's stake riding
        // the pointer, and answers the undefined sentinel for
        // anything else — the call site's undefable-heap guard turns
        // calling THAT into a catchable TypeError (right phase: the
        // return itself never throws). Pre-fix the raw box bits were
        // handed over as a pointer: a cell box happened to work, a
        // number box was a SIGSEGV.
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_to_callable_cell, vec![op]),
            ctx.f.ret,
            None,
        );
        return Operand::Value(v);
    }
    if actual == Type::Any && ctx.f.ret == Type::Bool {
        // Cluster-`values` follow-up (rotation 253) — a promoted bool
        // read (`return flags[1]` off a boolean[] global: the index
        // lane boxes Bool elements so OOB can spell undefined) used
        // to flow its Any box RAW into the declared Bool ret slot.
        // The caller then printed the box as if it were 0/1 — true
        // under one build's non-zero test, coincidentally right
        // under another's low-bit test. ToBoolean at the boundary,
        // like the number/Str arms above (a bool box carries no heap
        // payload, so no drop dance).
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_to_bool, vec![op]),
            Type::Bool,
            None,
        );
        return Operand::Value(v);
    }
    if actual == Type::Any && ctx.f.ret == Type::Str {
        // RC-4 — Any-typed value returned where the declared
        // return is Str: `anyv_to_str` materializes (fresh
        // owned; a short-str immediate box would otherwise be
        // deref'd as a Str pointer by the caller — heap results
        // only survived because a cell box IS the raw pointer).
        // The Any box's own stake settles via the box drop.
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_to_str_box, vec![op]),
            Type::Str,
            None,
        );
        ctx.emit_drop_value(op, Type::Any);
        // Pending-throw propagation — a user toString can throw
        // (0-check audit, rotation 130 L3b).
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    if ctx.f.ret == Type::F64 && actual == Type::I64 {
        ctx.coerce_to_f64(op)
    } else if ctx.f.ret == Type::I64 && actual == Type::F64 {
        ctx.coerce_to_i64(op)
    } else if ctx.f.ret == Type::Str && actual == Type::Substr {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![op]),
            Type::Str,
            None,
        );
        ctx.emit_drop_value(op, Type::Substr);
        Operand::Value(v)
    } else if let (Type::Arr(want_id), Type::Arr(got_id)) = (ctx.f.ret, actual)
        && ctx.arr_layouts[want_id.0 as usize] == Type::Str
        && ctx.arr_layouts[got_id.0 as usize] == Type::Substr
    {
        ctx.materialize_arr_substr_to_str(op, ctx.f.ret)
    } else if let Some(eid) = maybe
        && let (Type::Arr(_), Type::Arr(_)) = (ctx.f.ret, actual)
    {
        // P-SURF S8.5 — an `Arr<Any>` handed back through a typed
        // `T[]` return. The let-decl lane has converted at this
        // boundary since chunk 698; the return lane did not, so
        // `function read(): number[] { return [...gen()] }` gave
        // the caller NaN-box payloads to read as `f64` — silently,
        // since the same spread printed in place is correct. The
        // checker admits the pair through the assignability
        // lattice (`Array` is covariant and `Any` is assignable to
        // anything), which is only sound with the decode the
        // let-decl lane was already paying for.
        crate::ssa_lower_stmt_let_decl::maybe_arr_any_to_typed(ctx, ctx.f.ret, eid, op).0
    } else {
        op
    }
}
