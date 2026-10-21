//! K.3/K.4 top-level-global and M2 fn-addr let-decl lanes of
//! [`crate::ssa_lower_stmt_let_decl`] (chunk 734 file-size split —
//! chunk 732's mutable re-repr pushed the host past the 500-line
//! hard limit unrecorded; bodies verbatim).
//!
//! - `try_lower_global_let` — main-fn binding whose name registered
//!   as a module global: GlobalRef + Store with the K.6 empty-array
//!   fast path, chunk-698 any-to-typed decode-copy, and the chunk-627
//!   ident-alias borrow-inc admission rules.
//! - `try_lower_fn_addr_let` — `let f = global_fn`: FnSig slot with
//!   FnAddr store (direct-dispatch home for immutable named-fn
//!   references).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

use crate::ssa_lower_stmt_let_decl::maybe_arr_any_to_typed;

mod settle;

/// K.3 / K.4 — top-level data global: main fn + name in `globals` emits
/// GlobalRef + Store (K.6 empty-array fast path; refcount slots require
/// fresh-heap init). Returns false when `name` is not a module global.
pub(crate) fn try_lower_global_let(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
) -> bool {
    if !ctx.is_main_fn {
        return false;
    }
    // Only a TOP-LEVEL declaration is the global's own (`scope_stack`
    // holds the function frame alone there); a `let` of the same name
    // inside a block / catch / loop body is a distinct binding
    // (§14.3.1), and taking it here wrote the block's value into the
    // global slot — `var e: any = 1; { let e = "blk" } e` answered
    // "blk" (rotation 636; the `.cts` gate met it through the
    // global-var cell, which binds every top-level `var`).
    if ctx.scope_stack.len() > 1 {
        return false;
    }
    let Some(slot_ty) = ctx.globals.get(name).copied() else {
        return false;
    };
    let init_val = lower_global_init_val(ctx, init, slot_ty, type_ann);
    // Chunk 698 — general-path mirror: an `Array<Any>` init bound
    // to a typed `Array<T>` global decode-copies into a fresh typed
    // block (the global slot takes the fresh cell's only stake, so
    // the borrow-inc below is skipped for converted inits).
    let (init_val, converted) = maybe_arr_any_to_typed(ctx, slot_ty, init, init_val);
    // RFC 20260710 C1, at the GLOBAL position — `undefined` into a
    // sentinel-capable slot binds that width's immortal cell. Without
    // it the init is an `Expr::Ident`, which the borrow gate below
    // reads as an alias of another binding and refuses for a type
    // mismatch (`const a: string | undefined = undefined` was a K.4
    // "init shape is not yet supported"); the same declaration
    // spelled `let` has worked since this rotation's second blade,
    // and the two should not disagree about one line of source.
    //
    // `converted` is the existing channel for "ownership is already
    // settled, skip the borrow gate", and a sentinel qualifies for a
    // reason the other two users do not share: the cell is immortal
    // (FLAG_STATIC_LITERAL — rc and drop are no-ops), so there is no
    // stake to settle. Answers None for Any, which boxes to
    // ANY_UNDEF through the arm below instead.
    let (init_val, converted) = if !converted
        && matches!(
            ctx.expr_types.get(&init),
            Some(crate::check::Type::Undefined)
        )
        && let Some(sentinel) = ctx.str_undef_sentinel_for(slot_ty)
    {
        (sentinel, true)
    } else {
        (init_val, converted)
    };
    // The scalar mirror of the chunk-698 crossing above, and the same
    // one the fn-scope lane has always made: an `any` init under a
    // `number` / `string` / `boolean` annotation decodes at the binding
    // boundary. Missing here, a Call-shaped init (not a borrow shape,
    // so the gate below waved it through) stored the BOX BITS into a
    // `str` global and the next read deref'd them as a Str pointer —
    // `function g(): any { return "lit" }` + `const s: string = g()`
    // was a silent SIGSEGV, while the same line spelled `let` (fn-scope
    // lane) printed `lit`. Runs after the sentinel arm: `undefined`
    // must bind the immortal cell, not ToString into "undefined".
    let (init_val, converted) = if converted {
        (init_val, converted)
    } else {
        settle::any_into_scalar(ctx, type_ann, slot_ty, init, init_val)
    };
    // Chunk 809 — an Any slot boxes a concrete init value: a
    // borrowed source rc_incs first (`box_to_any` TRANSFERS the
    // stake into the box), so the fresh box is the slot's own ref
    // and K.4's fresh-heap-init holds. Skips the borrow gate below —
    // the box already settled ownership.
    let (init_val, converted) =
        if slot_ty == Type::Any && ctx.operand_ty(&init_val) != Type::Any && !converted {
            let is_borrow = matches!(
                ctx.ast.get_expr(init),
                Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
            );
            (ctx.box_owned_from_expr(init, init_val, is_borrow), true)
        } else {
            (init_val, converted)
        };
    let converted = converted || settle::settle_any_read_init(ctx, init, slot_ty, &init_val);
    let init_val = if converted {
        init_val
    } else {
        settle::settle_typed_init(ctx, name, init, slot_ty, init_val)
    };
    let coerced = if slot_ty == Type::F64 && ctx.operand_ty(&init_val) == Type::I64 {
        ctx.coerce_to_f64(init_val)
    } else {
        init_val
    };
    // RFC 20260707 chunk 627 — a typed array stored into an Arr<Any>
    // global slot (T-11 widen) marks the block's elem kind for the
    // kind-aware Arr<Any> readers (621 let-decl general-path mirror;
    // self-gating no-op for non-array / Arr<Any> values).
    ctx.emit_arr_mark_kind(&coerced);
    emit_module_ns_finalize(ctx, name, slot_ty, &coerced);
    crate::ssa_lower_map_num_lane::mark_global(ctx, name, coerced);
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
    emit_script_global_var_bind(ctx, name, slot_ty, ptr, BindSite::Declaration);
    true
}

/// Where a global-var cell bind is being emitted from.
/// A module namespace binding's object gets its §10.4.6 exotic
/// attributes from `__torajs_module_ns_finalize`, whichever slot the
/// binding lands in: the main-local lane and this global lane both
/// call here. A namespace read from a named function promotes to a
/// global, and without this the promoted object took writes
/// (`ns.count = 99` ran instead of throwing).
pub(crate) fn emit_module_ns_finalize(ctx: &mut LowerCtx, name: &str, ty: Type, val: &Operand) {
    if ty == Type::Any && ctx.ast.namespace_bindings.contains_key(name) {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.module_ns_finalize, vec![*val]),
            Type::Void,
            None,
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindSite {
    /// The top-level declaration's store — a written `var` binds here,
    /// so the property exists from instantiation (§16.1.7).
    Declaration,
    /// An assignment to the global slot — a synthesized implicit
    /// global binds here, so the property exists from the first write
    /// (§9.1.1.4.6 creates it then, and a `delete this.x` between two
    /// writes sees it created again). The slot holds nothing the
    /// program meant: this is the `x = x` self-assignment the `var`
    /// hoist mints for an init-less eval / Script `var`, so over an
    /// existing property the PROPERTY's value is the binding's.
    Write,
    /// An assignment whose value the slot already holds — a real
    /// `x = e`. Over an existing data property the binding takes the
    /// property (attributes kept) but NOT its value: the kernel used
    /// to copy the property's value back into the slot after the
    /// store, so `defineProperty(this, "q", {value: 0, writable:
    /// true, …}); q = 3` read back 0 (rotation 639).
    WriteAssigned,
}

/// The init value of a top-level data global, by shape: the K.6
/// empty-array fast path, an `Array<Any>` literal, an ObjectLit into
/// an `any` slot (dynobj lane), or the general expression — the head
/// of [`try_lower_global_let`], split at the function cap (rotation
/// 636); body verbatim.
fn lower_global_init_val(
    ctx: &mut LowerCtx,
    init: ExprId,
    slot_ty: Type,
    type_ann: Option<&String>,
) -> Operand {
    if let Expr::Array(els) = ctx.ast.get_expr(init)
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
    } else if slot_ty == Type::Any && matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. }) {
        // Rotation 204 — mirror of the fn-scope P3.2 arm
        // (`lower_let_init_val`): an ObjectLit init in an Any slot
        // lowers through the dynobj lane, NOT the anon-struct lane —
        // the define/expando write-back contract requires a dynobj
        // cell. The raw Ptr result rides the chunk-809 box station
        // below (Ptr is not refcounted at the SSA level, so no
        // borrow-inc fires; the fresh dynobj's +1 transfers into the
        // tag-4 box and K.4's fresh-heap-init holds).
        ctx.lower_dynobj_init(init)
    } else {
        // Chunk 780 — pin the declared struct layout for a direct
        // ObjectLit init (global-lane mirror of the fn-scope hint;
        // see `let_declared_obj_layout` field doc). Consumed by
        // `resolve_objlit_layout` via take-once.
        ctx.pin_declared(slot_ty, type_ann.map(String::as_str), init);
        ctx.lower_expr(init)
    }
}

/// RFC 20260912-global-var-cell — §16.1.7 step 17: a script's
/// top-level `var` becomes an own property of the global object over
/// the slot just stored. The hoisted `: any` prelude runs before the
/// first user statement, so the property exists from instantiation;
/// an escape-hatch typed `var` binds where its declaration runs (a
/// typed slot has no reflectable value before its init).
pub(crate) fn emit_script_global_var_bind(
    ctx: &mut LowerCtx,
    name: &str,
    slot_ty: Type,
    ptr: crate::ssa::ValueId,
    site: BindSite,
) {
    let Some(flavor) = ctx.ast.script_global_var_names.get(name).copied() else {
        return;
    };
    // An eval's / Script's `var` binds where its text ran: the hoist
    // left a write at that site (`var_hoist_eval_sites`).
    let binds_here = match flavor {
        crate::ast::ScriptGlobalVar::Declared | crate::ast::ScriptGlobalVar::Function => {
            site == BindSite::Declaration
        }
        crate::ast::ScriptGlobalVar::Implicit
        | crate::ast::ScriptGlobalVar::EvalVar
        | crate::ast::ScriptGlobalVar::ScriptVar
        | crate::ast::ScriptGlobalVar::EvalFn
        | crate::ast::ScriptGlobalVar::ScriptFn => site != BindSite::Declaration,
    };
    if !binds_here {
        return;
    }
    // A function declaration's hidden slot binds under the FUNCTION's
    // name (knife 3 of the RFC; `script_global_fns`).
    let is_function = matches!(
        flavor,
        crate::ast::ScriptGlobalVar::Function
            | crate::ast::ScriptGlobalVar::EvalFn
            | crate::ast::ScriptGlobalVar::ScriptFn
    );
    let name = if is_function {
        name.strip_prefix(crate::ast::script_global_fns::SCRIPT_GLOBAL_FN_SLOT_PREFIX)
            .expect("a function-flavoured slot carries the prefix")
    } else {
        name
    };
    // `torajs_dynobj::global_cell::KIND_*`; bit 8 asks for a
    // configurable property, bit 9 marks a binding with no bare
    // compiled writer, bit 10 a function binding, bit 11 a store
    // whose value the slot already holds, bit 12 an implicit global
    // (the kernel's contract).
    let configurable: i64 = match flavor {
        crate::ast::ScriptGlobalVar::Declared
        | crate::ast::ScriptGlobalVar::Function
        | crate::ast::ScriptGlobalVar::ScriptVar
        | crate::ast::ScriptGlobalVar::ScriptFn => 0,
        crate::ast::ScriptGlobalVar::Implicit
        | crate::ast::ScriptGlobalVar::EvalVar
        | crate::ast::ScriptGlobalVar::EvalFn => 1 << 8,
    };
    let writerless: i64 = if ctx.ast.script_global_writerless.contains(name) {
        1 << 9
    } else {
        0
    };
    // Bit 10: §9.1.1.4.18 CreateGlobalFunctionBinding rather than
    // CreateGlobalVarBinding — over an existing property the function
    // VALUE wins (the slot already holds it), attributes per the
    // property's configurability.
    let function: i64 = if is_function { 1 << 10 } else { 0 };
    let assigned: i64 = if site == BindSite::WriteAssigned {
        1 << 11
    } else {
        0
    };
    // Bit 12: the binding a [[Set]] creates (§9.1.1.4.6), which a
    // non-extensible object refuses SILENTLY in sloppy code — unlike a
    // `var` declaration's CanDeclareGlobalVar (TypeError).
    let implicit: i64 = if flavor == crate::ast::ScriptGlobalVar::Implicit {
        1 << 12
    } else {
        0
    };
    let Some(repr) = global_cell_kind(&slot_ty) else {
        return;
    };
    let kind: i64 = configurable | writerless | function | assigned | implicit | repr;
    let name_str = ctx.intern_string_literal(name);
    let f = ctx.intrinsics.global_var_bind;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            f,
            vec![
                Operand::Value(name_str),
                Operand::Value(ptr),
                Operand::ConstI64(kind),
            ],
        ),
    );
    ctx.emit_throw_check(None);
}

/// The representation word a cell over a global slot of `ty` carries
/// (torajs-dynobj `global_cell::KIND_*`): what the cell reads the
/// slot as. `None` for a slot no cell can read.
pub(crate) fn global_cell_kind(ty: &Type) -> Option<i64> {
    Some(match ty {
        Type::Any => 0,
        Type::I64 | Type::I32 => 1,
        Type::F64 => 2,
        Type::Bool => 3,
        Type::Str => 4,
        t if t.is_refcounted() => 5,
        _ => return None,
    })
}

/// M2 Phase B Stage 4 — `let f = global_fn`: FnSig slot with FnAddr
/// store. Returns false when init is not a bare global-fn ident.
pub(crate) fn try_lower_fn_addr_let(ctx: &mut LowerCtx, name: &str, init: ExprId) -> bool {
    // A closure capturing this name will inc and drop the slot as a
    // capture box (`ssa_lower_closure::write_captures` — a `FnSig` is
    // Copy, so it takes that arm unconditionally), and a capture box
    // keeps its refcount at `slot - 8`. The slot below is a bare
    // alloca, so that word is the callee frame's saved `lr`: the
    // closure's env drop decremented a return address and the program
    // died of `EXC_ARM_DA_ALIGN` with `pc == lr == <return addr> - 1`.
    // Let the general path claim the declaration instead — it is the
    // one that knows to heap-promote an escape-captured binding, the
    // same reason `try_sub_siblings` yields on `forward_capture_boxes`.
    if ctx.escape_captured_lets.contains(name) {
        return false;
    }
    let Expr::Ident(src_name) = ctx.ast.get_expr(init) else {
        return false;
    };
    if ctx.locals.contains_key(src_name) {
        return false;
    }
    let Some(fid) = ctx.fn_table.get(src_name).copied() else {
        return false;
    };
    let Some(sig_id) = ctx.fn_sig_ids.get(&fid).copied() else {
        return false;
    };
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
            boxed: false,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
    true
}
