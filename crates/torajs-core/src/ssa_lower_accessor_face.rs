//! Accessor DEFINE-face lowering — carved out of
//! `ssa_lower_accessor.rs` (file-size hard limit) when RFC
//! 20260717-fnexpr-this-channel knife 1 grew the face lane past 500
//! prod lines. One face of a literal `{ get, set }` descriptor lowers
//! here: the value-ABI kind derivation (`ACC_KIND_*` mirror), the
//! named-fn zero-capture env mint, and the fn-expr receiver-first
//! marking. The pair assembly / define emit stays in
//! [`crate::ssa_lower_accessor`].

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
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

/// Setter param kind — the closure's user value parameter (param 1,
/// after the env-first lifted `__env`) → [`accessor_kind_of`].
fn accessor_param_kind(ctx: &LowerCtx, op: &Operand) -> i64 {
    match ctx.operand_ty(op) {
        Type::Closure(sid) | Type::FnSig(sid) => ctx.fn_sigs[sid.0 as usize]
            .0
            .get(1)
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
        return (mint_named_fn_env(ctx, fid), k | 0x80);
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
    let k = if is_get {
        accessor_ret_kind(ctx, &op)
    } else {
        accessor_param_kind(ctx, &op)
    };
    (op, k)
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
                InstKind::FnAddr(bfid),
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
