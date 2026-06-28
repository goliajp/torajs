//! `Stmt::LetDecl` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 156).
//!
//! Pre-extract this arm was 789 LOC inline inside `lower_stmt` —
//! the final big arm left after chunks 145-155 cleaned the rest.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! `let v[: T] = init` lowering with multiple fast-paths:
//!
//! 1. **T-19.d** — `let X: T = await Bun.file(p).json()`: typed
//!    JSON parse inlined with `fs_read_file_sync` + `lower_json_parse`.
//! 2. **T-02** — `let v: T = JSON.parse(text)`: caller-driven typed
//!    parse (number→F64 widening for "number" annotations, container
//!    widening via `widen_container_ty`); fresh-owned text Str drop.
//! 3. **T-09.c** — `let o: Pair = Object.fromEntries(es)`: per-field
//!    unfold from `Array<Array<Any>>` entries into struct slots
//!    (untag per field type, rc_inc heap-typed values).
//! 4. **K.3 / K.4** — top-level module global (main fn + name in
//!    `globals`): emit GlobalRef + Store, with K.6 empty-array fast
//!    path and refcount fresh-init requirement.
//! 5. **M2 Phase B Stage 4** — `let f = global_fn`: FnSig slot with
//!    FnAddr store.
//! 6. **General path** — `let v[: T] = init`:
//!    - Parse annotation (number→F64 if num_f64_slots says so;
//!      container widening).
//!    - Empty `[]` literal w/o annotation → Array<Any> default
//!      (P0.10 — interned ArrId for sibling pushes).
//!    - **Alias-init detection** (Member / Index / cross-scope
//!      Ident) — moved=true so end-of-scope drop skips; underlying
//!      owner is the canonical dropper.
//!    - **11-A2-a stack-alloc hint** for ObjectLit binding inits
//!      not in `escape_obj_lets` (ObjectLit arm consumes via
//!      `.take()` + escape-safety decided at runtime layout).
//!    - **T-10.c / T-11** Arr<Any> alloc dispatch.
//!    - **P3.2** `let x: any = { ... }` → `lower_dynobj_init`
//!      directly (bypass struct alloc); `let x: any = [...]` →
//!      `lower_array_any_literal` (mirrors P3.2 ObjectLit dynobj path).
//!    - **CPython/Swift-ARC** share-vs-consume — same-scope `let t = s`
//!      of a refcounted local emits `rc_inc` instead of consume.
//!    - Boundary coercions: i64→f64; concrete→Any via
//!      `box_to_any_from_expr`; type-ann inference (Type::Void
//!      placeholder); Substr widening (Str/Substr per init).
//!    - **T-15.g.5** escape-captured Copy let: heap capture-box
//!      (16B = rc + value), F64/Bool bit-cast to i64.
//!    - Shadowing: capture prior LocalInfo into shadow_stack when
//!      outer-scope name is reinstated on this scope's close.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx, intern_arr_layout};
use crate::ssa_lower_parse_type::parse_type;

pub(crate) fn lower(ctx: &mut LowerCtx, name: &str, type_ann: Option<&String>, init: ExprId) {
    // T-19.d — `let X: T = await Bun.file(p).json()`.
    if let Some(mut slot_ty_for_parse) = ctx.try_resolve_type_ann(type_ann.map(|s| s.as_str()))
        && let Some(path_eid) = ctx.is_bun_file_json_await(init)
    {
        if matches!(slot_ty_for_parse, Type::I64) && type_ann.map(|s| s.as_str()) == Some("number")
        {
            slot_ty_for_parse = Type::F64;
        }
        let path_op = ctx.lower_expr(path_eid);
        let str_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.fs_read_file_sync, vec![path_op]),
            Type::Str,
            None,
        );
        let cursor = ctx.alloca(Type::I64, Some("__json_pos"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(cursor), 0),
        );
        let result = ctx.lower_json_parse(
            Operand::Value(str_v),
            Operand::Value(cursor),
            slot_ty_for_parse,
        );
        ctx.emit_drop_value(Operand::Value(str_v), Type::Str);
        let slot = ctx.binding_slot_alloca(slot_ty_for_parse, name);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(result, Operand::Value(slot), 0),
        );
        let cur_depth = ctx.scope_stack.len() - 1;
        ctx.locals.insert(
            name.to_string(),
            LocalInfo {
                slot,
                ty: slot_ty_for_parse,
                moved: false,
                borrowed: false,
                scope_depth: cur_depth,
            },
        );
        ctx.scope_stack.last_mut().unwrap().push(name.to_string());
        return;
    }
    // T-02 — `let v: T = JSON.parse(text)`.
    if let Some(mut slot_ty_for_parse) = ctx.try_resolve_type_ann(type_ann.map(|s| s.as_str()))
        && ctx.is_json_parse_call(init)
    {
        if matches!(slot_ty_for_parse, Type::I64) && type_ann.map(|s| s.as_str()) == Some("number")
        {
            slot_ty_for_parse = Type::F64;
        }
        slot_ty_for_parse = crate::ssa_lower_container_width::widen_container_ty(
            slot_ty_for_parse,
            type_ann.map(|s| s.as_str()),
            &ctx.num_width_local_key(name),
            ctx.num_f64_slots,
            ctx.arr_layouts,
            ctx.struct_layouts,
            ctx.fn_sigs,
        );
        let text_eid = if let Expr::Call { args, .. } = ctx.ast.get_expr(init).clone() {
            args[0]
        } else {
            unreachable!()
        };
        let text_op = ctx.lower_expr(text_eid);
        let cursor = ctx.alloca(Type::I64, Some("__json_pos"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(cursor), 0),
        );
        let result = ctx.lower_json_parse(text_op, Operand::Value(cursor), slot_ty_for_parse);
        if ctx.expr_is_fresh_owned(text_eid) && ctx.operand_ty(&text_op).is_refcounted() {
            ctx.emit_drop_value(text_op, ctx.operand_ty(&text_op));
        }
        let slot = ctx.binding_slot_alloca(slot_ty_for_parse, name);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(result, Operand::Value(slot), 0),
        );
        let cur_depth = ctx.scope_stack.len() - 1;
        ctx.locals.insert(
            name.to_string(),
            LocalInfo {
                slot,
                ty: slot_ty_for_parse,
                moved: false,
                borrowed: false,
                scope_depth: cur_depth,
            },
        );
        let top = ctx.scope_stack.last_mut().expect("scope frame");
        top.push(name.to_string());
        return;
    }
    // T-09.c — `let o: Pair = Object.fromEntries(es)`. Sub-sibling.
    if crate::ssa_lower_stmt_let_decl_fromentries::try_lower(ctx, name, type_ann, init) {
        return;
    }
    // K.3 / K.4 — top-level data global.
    if ctx.is_main_fn
        && let Some(slot_ty) = ctx.globals.get(name).copied()
    {
        let init_val = if let Expr::Array(els) = ctx.ast.get_expr(init)
            && els.is_empty()
            && matches!(slot_ty, Type::Arr(_))
        {
            let alloc_fn = if let Type::Arr(arr_id) = slot_ty
                && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
            {
                ctx.intrinsics.arr_alloc_any
            } else {
                ctx.intrinsics.arr_alloc
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
                slot_ty,
                None,
            );
            Operand::Value(v)
        } else if let Expr::Array(els) = ctx.ast.get_expr(init)
            && let Type::Arr(arr_id) = slot_ty
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            let ids: Vec<ExprId> = els.clone();
            ctx.lower_array_any_literal(&ids)
        } else {
            ctx.lower_expr(init)
        };
        if slot_ty.is_refcounted() {
            let init_is_borrow = matches!(
                ctx.ast.get_expr(init),
                Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
            );
            if init_is_borrow {
                panic!(
                    "ssa-lower: K.4 refcount global `{name}` requires fresh-heap init (function-call / concat / new); borrow-shaped init not yet supported"
                );
            }
        }
        let coerced = if slot_ty == Type::F64 && ctx.operand_ty(&init_val) == Type::I64 {
            ctx.coerce_to_f64(init_val)
        } else {
            init_val
        };
        let ptr = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::GlobalRef(name.to_string()),
            Type::Ptr,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(coerced, Operand::Value(ptr), 0),
        );
        let _ = type_ann;
        let _ = slot_ty;
        return;
    }
    // M2 Phase B Stage 4 — `let f = global_fn`.
    if let Expr::Ident(src_name) = ctx.ast.get_expr(init)
        && ctx.locals.get(src_name).is_none()
        && let Some(fid) = ctx.fn_table.get(src_name).copied()
        && let Some(sig_id) = ctx.fn_sig_ids.get(&fid).copied()
    {
        let ty = Type::FnSig(sig_id);
        let slot = ctx.binding_slot_alloca(ty, name);
        let v = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::FnAddr(fid), ty, None);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
        );
        let cur_depth = ctx.scope_stack.len() - 1;
        ctx.locals.insert(
            name.to_string(),
            LocalInfo {
                slot,
                ty,
                moved: false,
                borrowed: false,
                scope_depth: cur_depth,
            },
        );
        let top = ctx.scope_stack.last_mut().expect("scope frame");
        top.push(name.to_string());
        return;
    }
    // General path.
    let mut ty = if type_ann.is_some() {
        let parsed = parse_type(
            type_ann.map(|s| s.as_str()),
            ctx.aliases,
            ctx.arr_layouts,
            ctx.fn_sigs,
            ctx.generic_struct_decls,
            ctx.struct_layouts,
            ctx.inst_memo,
        );
        if parsed == Type::I64
            && type_ann.map(|s| s.as_str()) == Some("number")
            && ctx
                .num_f64_slots
                .slot_is_f64(&ctx.num_width_local_key(name))
        {
            Type::F64
        } else {
            crate::ssa_lower_container_width::widen_container_ty(
                parsed,
                type_ann.map(|s| s.as_str()),
                &ctx.num_width_local_key(name),
                ctx.num_f64_slots,
                ctx.arr_layouts,
                ctx.struct_layouts,
                ctx.fn_sigs,
            )
        }
    } else if let Expr::Array(els) = ctx.ast.get_expr(init)
        && els.is_empty()
    {
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        Type::Arr(arr_id)
    } else {
        Type::Void
    };
    let cur_depth = ctx.scope_stack.len() - 1;
    let is_alias_init = match ctx.ast.get_expr(init) {
        Expr::Member { .. } | Expr::Index { .. } => true,
        Expr::Ident(src) => ctx
            .locals
            .get(src)
            .map(|info| info.scope_depth < cur_depth)
            .unwrap_or(false),
        _ => false,
    };
    let stack_alloc_hinted = matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. })
        && !ctx.escape_obj_lets.contains(name);
    if stack_alloc_hinted {
        ctx.let_stack_alloc_hint = Some(name.to_string());
    }
    let init_val = if let Expr::Array(els) = ctx.ast.get_expr(init)
        && els.is_empty()
    {
        if !matches!(ty, Type::Arr(_)) {
            panic!("ssa-lower: empty `[]` literal needs an array type annotation; got {ty:?}");
        }
        let alloc_fn = if let Type::Arr(arr_id) = ty
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            ctx.intrinsics.arr_alloc_any
        } else {
            ctx.intrinsics.arr_alloc
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            ty,
            None,
        );
        Operand::Value(v)
    } else if let Expr::Array(els) = ctx.ast.get_expr(init)
        && let Type::Arr(arr_id) = ty
        && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
    {
        let _ = els;
        let ids: Vec<ExprId> = if let Expr::Array(els2) = ctx.ast.get_expr(init) {
            els2.clone()
        } else {
            unreachable!()
        };
        ctx.lower_array_any_literal(&ids)
    } else if ty == Type::Any && matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. }) {
        ctx.lower_dynobj_init(init)
    } else if ty == Type::Any
        && let Expr::Array(els) = ctx.ast.get_expr(init)
    {
        let ids: Vec<ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else {
        ctx.lower_expr(init)
    };
    ctx.let_stack_alloc_hint = None;
    if !is_alias_init {
        let pre_ty = ctx.operand_ty(&init_val);
        let shares = if let Expr::Ident(src) = ctx.ast.get_expr(init) {
            ctx.locals.contains_key(src)
                && pre_ty.is_refcounted()
                && !(ty == Type::Any && pre_ty != Type::Any)
        } else {
            false
        };
        if shares {
            ctx.emit_rc_inc(init_val.clone());
        } else {
            ctx.consume_if_ident(init);
        }
    }
    let init_val = if ty == Type::F64 && ctx.operand_ty(&init_val) == Type::I64 {
        ctx.coerce_to_f64(init_val)
    } else {
        init_val
    };
    let init_val = if ty == Type::Any && ctx.operand_ty(&init_val) != Type::Any {
        ctx.box_to_any_from_expr(init, init_val)
    } else {
        init_val
    };
    let init_val = if type_ann.is_none() {
        ty = ctx.operand_ty(&init_val);
        if ty == Type::I64
            && ctx
                .num_f64_slots
                .slot_is_f64(&ctx.num_width_local_key(name))
        {
            ty = Type::F64;
            ctx.coerce_to_f64(init_val)
        } else {
            init_val
        }
    } else {
        init_val
    };
    let init_ty = ctx.operand_ty(&init_val);
    if ty == Type::Str && init_ty == Type::Substr {
        ty = Type::Substr;
    } else if let (Type::Arr(ann_id), Type::Arr(init_id)) = (ty, init_ty)
        && ctx.arr_layouts[ann_id.0 as usize] == Type::Str
        && ctx.arr_layouts[init_id.0 as usize] == Type::Substr
    {
        ty = init_ty;
    }
    let escape_captured = ty.is_copy() && ctx.escape_captured_lets.contains(name);
    let slot = if escape_captured {
        let init_i64 = if matches!(ty, Type::F64) {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BitCastF64ToI64(init_val.clone()),
                Type::I64,
                None,
            );
            Operand::Value(v)
        } else if matches!(ty, Type::Bool) {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ZExtBoolToI64(init_val.clone()),
                Type::I64,
                None,
            );
            Operand::Value(v)
        } else {
            init_val.clone()
        };
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.capture_box_alloc, vec![init_i64]),
            Type::Ptr,
            None,
        )
    } else {
        let slot = ctx.binding_slot_alloca(ty, name);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(init_val, Operand::Value(slot), 0),
        );
        slot
    };
    if let Some(prev) = ctx.locals.get(name).copied()
        && prev.scope_depth < cur_depth
    {
        let top_shadow = ctx.shadow_stack.last_mut().expect("shadow frame");
        top_shadow.push((name.to_string(), prev));
    }
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            moved: is_alias_init || escape_captured,
            borrowed: is_alias_init,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
}
