//! M2 — `Expr::Closure { fn_name, captures }` lowering pulled out
//! of [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-
//! 68 of the decomp (chunks 1-67 = ... + `Expr::Index` lowering).
//!
//! Allocates a heap env block of size `CLOSURE_CAP_BASE_OFF + 8 *
//! captures.len()`, stores `fn_addr` + `drop_fn_addr` + 0
//! `props_dynobj` slot in the header, then writes each capture.
//! Yields the env pointer typed as `Type::Closure(user_sig)`.
//!
//! Three phases:
//!
//! 1. **Signature derivation** — Look up the lifted FnDecl's
//!    Pass-1 interned `sig_id` from `fn_sig_ids` (re-parsing the
//!    AST annotations would re-introduce un-widened parse widths
//!    per §5.6 F2 drift). Strip the env-first param and intern
//!    the user-facing signature via `intern_fn_sig` to materialize
//!    `Type::Closure(user_sig)`.
//! 2. **Env construction** — `__torajs_obj_alloc(size)`, init the
//!    universal heap header (refcount=1 at +0, type_tag=CLOSURE=3
//!    at +4), store `fn_addr` at `CLOSURE_FN_ADDR_OFF`, init
//!    `props_dynobj` slot to 0 at `CLOSURE_PROPS_OFF` (T-27 lazy
//!    `f.x = v` ECMAScript §10.2 Function-as-Object), store
//!    `drop_fn_addr` at `CLOSURE_DROP_FN_OFF` (pre-registered in
//!    Pass 1 as `__env_drop_<fn_name>`).
//! 3. **Capture writes** — for each capture in declaration order:
//!    - **Copy types** (`is_copy()`): by-reference. `info.slot`
//!      is a stable pointer (heap-alloc'd at let-decl when the
//!      let is escape-captured, otherwise stack alloca living at
//!      least as long as the closure). T-15.g.5: inc the capture
//!      box's refcount via `__torajs_capture_box_inc` BEFORE
//!      stashing in env so multi-closure capture doesn't
//!      double-free at env-drop. Store `info.slot` ptr at
//!      `CLOSURE_CAP_BASE_OFF + i*8`.
//!    - **Non-Copy types**: by-value of the heap pointer. Load
//!      from `info.slot`, store value into env slot. Mark outer
//!      `local.moved = true` so the env's pointer isn't double-
//!      freed (closure body may realloc the array via push,
//!      updating env+offset; outer slot still holds stale pre-
//!      realloc ptr). env-drop skips non-Copy captures.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{
    CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF, CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, LowerCtx,
    intern_fn_sig,
};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, fn_name: String, captures: Vec<String>) -> Operand {
    let fid = ctx
        .fn_table
        .get(&fn_name)
        .copied()
        .unwrap_or_else(|| panic!("ssa-lower: closure target `{fn_name}` not in fn table"));
    let own_sig = ctx
        .fn_sig_ids
        .get(&fid)
        .copied()
        .unwrap_or_else(|| panic!("ssa-lower: closure `{fn_name}` has no interned sig"));
    let (own_params, user_ret_ty) = ctx.fn_sigs[own_sig.0 as usize].clone();
    let user_param_tys: Vec<Type> = own_params.iter().skip(1).copied().collect();
    let user_sig = intern_fn_sig(ctx.fn_sigs, user_param_tys, user_ret_ty);
    let closure_ty = Type::Closure(user_sig);

    let cap_tys: Vec<Type> = captures
        .iter()
        .map(|c| {
            ctx.locals
                .get(c)
                .map(|l| l.ty)
                .unwrap_or_else(|| panic!("ssa-lower: closure capture `{c}` not in scope"))
        })
        .collect();
    let cap_meta: Vec<(Type, bool)> = cap_tys.iter().map(|t| (*t, t.is_copy())).collect();
    ctx.closure_captures.insert(fn_name.clone(), cap_meta);

    let env_v = alloc_env(ctx, closure_ty, captures.len());
    init_env_header(ctx, env_v, fid, &fn_name);
    write_captures(ctx, env_v, &captures, &cap_tys);
    Operand::Value(env_v)
}

fn alloc_env(ctx: &mut LowerCtx<'_>, closure_ty: Type, captures_len: usize) -> crate::ssa::ValueId {
    let alloc_size = CLOSURE_CAP_BASE_OFF as i64 + 8 * (captures_len as i64);
    let cur_block = ctx.cur_block;
    let env_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_alloc,
            vec![Operand::ConstI64(alloc_size)],
        ),
        closure_ty,
        None,
    );
    let cur_block = ctx.cur_block;
    // Universal heap header: refcount=1 at +0, type_tag=CLOSURE=3 at +4.
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI32(1), Operand::Value(env_v), 0),
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI32(3), Operand::Value(env_v), 4),
    );
    env_v
}

fn init_env_header(
    ctx: &mut LowerCtx<'_>,
    env_v: crate::ssa::ValueId,
    fid: crate::ssa::FuncId,
    fn_name: &str,
) {
    let lifted_sig_id = *ctx
        .fn_sig_ids
        .get(&fid)
        .expect("lifted closure has interned signature");
    let cur_block = ctx.cur_block;
    let fn_addr_v = ctx.f.append_inst(
        cur_block,
        InstKind::FnAddr(fid),
        Type::FnSig(lifted_sig_id),
        None,
    );
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::Value(fn_addr_v),
            Operand::Value(env_v),
            CLOSURE_FN_ADDR_OFF,
        ),
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::ConstI64(0),
            Operand::Value(env_v),
            CLOSURE_PROPS_OFF,
        ),
    );
    let drop_fn_name = format!("__env_drop_{fn_name}");
    let drop_fid = *ctx.fn_table.get(&drop_fn_name).unwrap_or_else(|| {
        panic!(
            "ssa-lower: missing pre-registered drop fn `{drop_fn_name}` \
             for closure `{fn_name}`"
        )
    });
    let drop_sig = *ctx
        .fn_sig_ids
        .get(&drop_fid)
        .expect("drop fn has interned signature");
    let cur_block = ctx.cur_block;
    let drop_addr_v = ctx.f.append_inst(
        cur_block,
        InstKind::FnAddr(drop_fid),
        Type::FnSig(drop_sig),
        None,
    );
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::Value(drop_addr_v),
            Operand::Value(env_v),
            CLOSURE_DROP_FN_OFF,
        ),
    );
}

fn write_captures(
    ctx: &mut LowerCtx<'_>,
    env_v: crate::ssa::ValueId,
    captures: &[String],
    cap_tys: &[Type],
) {
    for (i, (cap_name, cap_ty)) in captures.iter().zip(cap_tys.iter()).enumerate() {
        let info = *ctx.locals.get(cap_name).expect("capture in scope");
        let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
        if cap_ty.is_copy() {
            let cur_block = ctx.cur_block;
            ctx.f.append_void(
                cur_block,
                InstKind::Call(
                    ctx.intrinsics.capture_box_inc,
                    vec![Operand::Value(info.slot)],
                ),
            );
            ctx.f.append_void(
                cur_block,
                InstKind::Store(Operand::Value(info.slot), Operand::Value(env_v), offset),
            );
        } else {
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Load(*cap_ty, Operand::Value(info.slot), 0),
                *cap_ty,
                None,
            );
            ctx.f.append_void(
                cur_block,
                InstKind::Store(Operand::Value(v), Operand::Value(env_v), offset),
            );
            if let Some(outer) = ctx.locals.get_mut(cap_name) {
                outer.moved = true;
            }
        }
    }
}
