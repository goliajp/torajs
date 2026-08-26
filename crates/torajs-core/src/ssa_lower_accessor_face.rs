//! Accessor DEFINE-face lowering — carved out of
//! `ssa_lower_accessor.rs` (file-size hard limit) when RFC
//! 20260717-fnexpr-this-channel knife 1 grew the face lane past 500
//! prod lines. One face of a literal `{ get, set }` descriptor lowers
//! here: the value-ABI kind derivation (`ACC_KIND_*` mirror), the
//! named-fn zero-capture env mint, and the fn-expr receiver-first
//! marking. The pair assembly / define emit stays in
//! [`crate::ssa_lower_accessor`].

use crate::ast::ExprId;
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Map an SSA value type to the accessor value-ABI kind the runtime
/// invoke path keys on (mirrors `torajs-dynobj::accessor`'s
/// `ACC_KIND_*`: 0=Any 1=I64 2=F64 3=Bool 4=Ptr). Drives which
/// fn-pointer transmute + box the getter/setter dispatch uses across
/// the register-class boundary.
fn accessor_kind_of(ty: &Type) -> i64 {
    match ty {
        Type::I64 | Type::I32 => 1,
        Type::F64 => 2,
        Type::Bool => 3,
        Type::Any => 0,
        // A throw-only / no-`return` getter is a native void fn —
        // never read its return register (ACC_KIND_VOID answers
        // undefined; RFC 20260713-accessor-void-kind).
        Type::Void => 6,
        t if t.is_refcounted() => 4,
        _ => 0,
    }
}

/// Getter ret kind — the closure's SSA return type → [`accessor_kind_of`].
fn accessor_ret_kind(ctx: &LowerCtx, op: &Operand) -> i64 {
    match ctx.operand_ty(op) {
        Type::Closure(sid) | Type::FnSig(sid) => accessor_kind_of(&ctx.fn_sigs[sid.0 as usize].1),
        _ => 0,
    }
}

/// Setter param kind — the closure's user value parameter →
/// [`accessor_kind_of`]. A `Closure`/`FnSig` signature is the USER
/// face: the lifted `__env` is a codegen detail and never enters
/// `fn_sigs` params, so the value parameter is `.first()` (S2.27
/// probe: `set(v: number)` interned as `([I64], Void)`; the prior
/// `.get(1)` env-first read answered None → ACC_KIND_ANY, and the
/// invoke handed the NaN-box verbatim to an i64-ABI body — every
/// typed-param setter behind the define kernel read box bits).
fn accessor_param_kind(ctx: &LowerCtx, op: &Operand) -> i64 {
    match ctx.operand_ty(op) {
        Type::Closure(sid) | Type::FnSig(sid) => ctx.fn_sigs[sid.0 as usize]
            .0
            .first()
            .map_or(0, accessor_kind_of),
        _ => 0,
    }
}

/// One `get` / `set` face of a literal accessor descriptor (chunk D,
/// RFC 20260713-defprop-tpd-cluster):
///
/// - explicit `undefined` → NULL face (present-and-clearing; the
///   flags byte's per-face present bit still lands).
/// - a named top-level fn (`Type::FnSig` — a raw code address, NOT a
///   closure cell) → mint a zero-capture env cell so the pair's
///   env-first invoke / drop contract holds (pre-fix the raw address
///   was stored verbatim: invoke read code memory as a cell header —
///   SIGBUS). Invoke rides the boxed dual entry (`ACC_KIND_BOXED`).
/// - everything else keeps the prior shape (closure cells verbatim
///   with their signature-derived kind).
pub(crate) fn lower_accessor_face(ctx: &mut LowerCtx, eid: ExprId, is_get: bool) -> (Operand, i64) {
    if matches!(ctx.ast.get_expr(eid), crate::ast::Expr::Ident(n) if n == "undefined")
        && !ctx.locals.contains_key("undefined")
    {
        return (Operand::ConstPtrNull, 0);
    }
    let op = ctx.lower_expr(eid);
    if matches!(ctx.operand_ty(&op), Type::FnSig(_))
        && let crate::ast::Expr::Ident(name) = ctx.ast.get_expr(eid)
        && let Some(&fid) = ctx.fn_table.get(name)
    {
        // Signature-derived kind | ACC_KIND_NAKED (0x80, torajs-dynobj
        // accessor.rs mirror) — the named fn's native signature has
        // no leading env param, so the setter's user value is param 0
        // (accessor_param_kind reads the env-first param 1).
        let sig = *ctx.fn_sig_ids.get(&fid).expect("named fn has a signature");
        let k = if is_get {
            accessor_ret_kind(ctx, &op)
        } else {
            ctx.fn_sigs[sig.0 as usize]
                .0
                .first()
                .map_or(0, accessor_kind_of)
        };
        let fn_name = name.clone();
        return (canonical_named_fn_cell(ctx, fid, &fn_name), k | 0x80);
    }
    // RFC 20260717-fnexpr-this-channel knife 1 — a fn-expr face whose
    // body says `this` was given a `__this` first param by
    // `desugar_fnexpr_this`; ride the boxed dual entry with the
    // receiver in argv[0] (ACC_KIND_BOXED=5 | ACC_KIND_RECV=0x40,
    // `torajs_dynobj::accessor` mirror).
    if let crate::ast::Expr::Closure { fn_name, .. } = ctx.ast.get_expr(eid)
        && ctx.ast.fnexpr_recv_fns.contains(fn_name)
    {
        return (op, 5 | 0x40);
    }
    // Knife 2 — a variable-routed fn-expr face: the AST pass recorded
    // the exact face ExprId (single-use const binding, so no other
    // call path exists). The Ident borrow still takes its own stake
    // (same rationale as the plain-Ident arm below — the pair owns
    // its faces).
    if ctx.ast.fnexpr_recv_faces.contains(&eid) {
        if matches!(ctx.operand_ty(&op), Type::Closure(_)) {
            ctx.emit_rc_inc(op.clone());
        }
        return (op, 5 | 0x40);
    }
    // An Any-typed face (`const g: any = function(){..}`) is a
    // NaN-box — storing its bits in the pair verbatim transmuted
    // them as a cell on invoke (SIGSEGV). Unbox through the runtime
    // §6.2.6.5 IsCallable check: a closure cell answers an owned
    // pointer (pair takes ownership), undefined/null clear the
    // face, everything else leaves a catchable TypeError.
    if matches!(ctx.operand_ty(&op), Type::Any) {
        let p = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.accessor_face_from_any, vec![op]),
            Type::Ptr,
            None,
        );
        ctx.emit_throw_check(None);
        // ACC_KIND_BOXED — any-world closures invoke through the
        // boxed dual entry.
        return (Operand::Value(p), 5);
    }
    // An Ident face is a BORROW of the binding's closure ref, but
    // `accessor_pair_new` takes its faces by ownership transfer —
    // without a stake of its own the pair aliases the binding, and a
    // later reassignment / scope close frees the closure under the
    // live pair (rotation 128 diag: `let s = function(v){..};
    // defineProperty(o, k, {set: s}); s = ...;` left the pair's face
    // at rc 0 — the property write invoked freed memory). A fresh
    // `Expr::Closure` face already arrives owned (mint / canonical
    // cell +1), so only the Ident shape takes the extra stake.
    if matches!(ctx.ast.get_expr(eid), crate::ast::Expr::Ident(_))
        && matches!(ctx.operand_ty(&op), Type::Closure(_))
    {
        ctx.emit_rc_inc(op.clone());
    }
    let k = if is_get {
        accessor_ret_kind(ctx, &op)
    } else {
        accessor_param_kind(ctx, &op)
    };
    (op, k)
}

/// RFC 20260717-namedfn-canonical-cell chunk 2 — the naked-face
/// mint rides a per-fn hidden `__fncell_naked_*` slot (the chunk-1
/// `__forward_*` lazy-once mirror): an ES fn object is a SINGLETON,
/// so two accessor faces over the same named fn must compare equal
/// (`gOPD(o).get === gOPD(p).get`). This lane only sees fns the
/// forwarder collector didn't rewrite (nested `__nested___top_*`
/// lifts — a rewritten face arrives as a Closure and never reaches
/// the FnSig arm). The slot keeps a permanent stake; each use
/// answers the cell +1, so the caller's owned-face contract
/// (`bf094d85`) is unchanged.
fn canonical_named_fn_cell(ctx: &mut LowerCtx, fid: crate::ssa::FuncId, name: &str) -> Operand {
    let sig = *ctx.fn_sig_ids.get(&fid).expect("named fn has a signature");
    let closure_ty = Type::Closure(sig);
    let gref = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(format!("__fncell_naked_{name}")),
        Type::Ptr,
        None,
    );
    let cached = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(closure_ty, Operand::Value(gref), 0),
        closure_ty,
        None,
    );
    let res_slot = ctx.alloca(closure_ty, Some("__fncell_naked_res"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(cached), Operand::Value(res_slot), 0),
    );
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(cached), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let mint_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: mint_blk,
            else_blk: join_blk,
        },
    );
    ctx.cur_block = mint_blk;
    let minted = mint_named_fn_env(ctx, fid);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(minted.clone(), Operand::Value(gref), 0),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(minted, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    ctx.cur_block = join_blk;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(closure_ty, Operand::Value(res_slot), 0),
        closure_ty,
        None,
    );
    ctx.emit_rc_inc(Operand::Value(v));
    Operand::Value(v)
}

/// Zero-capture env cell for a named top-level fn used as an
/// accessor face — header + fn_addr + trivial drop + NULL props +
/// boxed dual entry (the `ACC_KIND_BOXED` invoke channel; 0 when no
/// adapter was synthesized, which the invoke answers as undefined /
/// no-op). Same hand-mint shape as the promise-chain thunk wrap.
fn mint_named_fn_env(ctx: &mut LowerCtx, fid: crate::ssa::FuncId) -> Operand {
    let sig = *ctx.fn_sig_ids.get(&fid).expect("named fn has a signature");
    let env_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_alloc,
            vec![Operand::ConstI64(
                crate::ssa_lower::CLOSURE_CAP_BASE_OFF as i64,
            )],
        ),
        Type::Closure(sig),
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI32(1), Operand::Value(env_v), 0),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI32(3), Operand::Value(env_v), 4),
    );
    let fn_addr = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::FnAddr(fid), Type::FnSig(sig), None);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::Value(fn_addr),
            Operand::Value(env_v),
            crate::ssa_lower::CLOSURE_FN_ADDR_OFF,
        ),
    );
    let (drop_fid, drop_sig) = ctx.intrinsics.env_drop_trivial;
    let drop_addr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::FnAddr(drop_fid),
        Type::FnSig(drop_sig),
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::Value(drop_addr),
            Operand::Value(env_v),
            crate::ssa_lower::CLOSURE_DROP_FN_OFF,
        ),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::ConstI64(0),
            Operand::Value(env_v),
            crate::ssa_lower::CLOSURE_PROPS_OFF,
        ),
    );
    // trace_fn stub — zero-capture env has nothing to trace, and
    // obj_alloc is plain malloc (slot would be garbage otherwise).
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::ConstI64(0),
            Operand::Value(env_v),
            crate::ssa_lower::CLOSURE_TRACE_FN_OFF,
        ),
    );
    let boxed_op = match ctx.boxed_entries.get(&fid) {
        Some(&(bfid, bsig)) => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BoxedEntryAddr(bfid),
                Type::FnSig(bsig),
                None,
            );
            Operand::Value(v)
        }
        None => Operand::ConstI64(0),
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            boxed_op,
            Operand::Value(env_v),
            crate::ssa_lower::CLOSURE_BOXED_ENTRY_OFF,
        ),
    );
    Operand::Value(env_v)
}
