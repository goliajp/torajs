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
//!    `f.x = v` ECMAScript §10.2 Function-as-Object), zero the
//!    `trace_fn` stub at `CLOSURE_TRACE_FN_OFF` (RFC 20260717
//!    closure-env-cycle), store `drop_fn_addr` at
//!    `CLOSURE_DROP_FN_OFF` (pre-registered in Pass 1 as
//!    `__env_drop_<fn_name>`).
//! 3. **Capture writes** — for each capture in declaration order:
//!    - **Copy types** (`is_copy()`): by-reference. `info.slot`
//!      is a stable pointer (heap-alloc'd at let-decl when the
//!      let is escape-captured, otherwise stack alloca living at
//!      least as long as the closure). T-15.g.5: inc the capture
//!      box's refcount via `__torajs_capture_box_inc` BEFORE
//!      stashing in env so multi-closure capture doesn't
//!      double-free at env-drop. Store `info.slot` ptr at
//!      `CLOSURE_CAP_BASE_OFF + i*8`.
//!    - **Non-Copy types**: by-value of the heap pointer — the env
//!      SHARES the capture (RFC 20260705 Phase 1.5): every env takes
//!      its own +1 stake (`any_box_rc_inc` for Any, `emit_rc_inc`
//!      otherwise) and the outer binding keeps its own stake (scope
//!      close / reassign releases it). env-drop releases one
//!      reference per non-Copy slot. Known residue (RFC Phase 2):
//!      an Arr capture realloc'd by an in-closure push leaves the
//!      outer slot holding a stale ptr — capture-box indirection is
//!      the orthodox fix.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{
    CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF, CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, LowerCtx,
    intern_fn_sig,
};

/// Does the lifted closure `fn_name` carry an object-literal method's
/// `__this` receiver (second param, after `__env`)?
fn fn_has_this_param(ctx: &LowerCtx<'_>, fn_name: &str) -> bool {
    ctx.ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::FnDecl { name, params, .. }
            if name == fn_name && params.get(1).is_some_and(|p| p.name == "__this"))
    })
}

/// The SSA type a `Expr::Closure` VALUE has: the lifted fn's interned
/// signature with the hidden leading params stripped.
///
/// Skip `__env`, and for an object-literal method (RFC
/// 20260714-objlit-accessor blade 1) also the `__this` receiver — the
/// closure VALUE's type is the user-facing signature. Keeping the
/// receiver out of every Type is what stops `__ObjLit_n`'s layout from
/// referring to itself: a method slot's sig would name the very struct
/// being interned, and `parse_struct` has no memo-before-fill to break
/// the cycle. The real `(__env, __this, ...user)` ABI is reassembled at
/// the one call arm that knows it (the field-call arm in
/// `ssa_lower_call_struct_method_dispatch`).
///
/// The mint below reads it, and so does the let-decl's self-reference
/// path — which has to size the binding's box BEFORE the closure that
/// fills it exists, and so cannot wait for the minted operand's type.
pub(crate) fn closure_value_ty(ctx: &mut LowerCtx<'_>, fn_name: &str) -> Type {
    let fid = ctx
        .fn_table
        .get(fn_name)
        .copied()
        .unwrap_or_else(|| panic!("ssa-lower: closure target `{fn_name}` not in fn table"));
    let own_sig = ctx
        .fn_sig_ids
        .get(&fid)
        .copied()
        .unwrap_or_else(|| panic!("ssa-lower: closure `{fn_name}` has no interned sig"));
    let (own_params, user_ret_ty) = ctx.fn_sigs[own_sig.0 as usize].clone();
    let hidden = 1 + usize::from(fn_has_this_param(ctx, fn_name));
    let user_param_tys: Vec<Type> = own_params.iter().skip(hidden).copied().collect();
    Type::Closure(intern_fn_sig(ctx.fn_sigs, user_param_tys, user_ret_ty))
}

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, fn_name: String, captures: Vec<String>) -> Operand {
    let fid = ctx
        .fn_table
        .get(&fn_name)
        .copied()
        .unwrap_or_else(|| panic!("ssa-lower: closure target `{fn_name}` not in fn table"));
    let closure_ty = closure_value_ty(ctx, &fn_name);

    // K.3/K.4/K.6 — a capture name that resolves to a promoted data
    // global is NOT a capture: the lifted body reads it through
    // GlobalRef + Load exactly like a named-fn body does, so it gets
    // no env slot. Locals win over a same-named global (shadowing —
    // the literal site's scope is the ground truth `lift_arrow_fns`
    // tracked). The filtered list drives the env layout everywhere:
    // this side channel feeds both the body preamble and the
    // env-drop synthesis, so all three stay in agreement.
    let eff_captures: Vec<(String, Type)> = captures
        .iter()
        .filter_map(|c| {
            if let Some(l) = ctx.locals.get(c) {
                Some((c.clone(), l.ty))
            } else if ctx.globals.contains_key(c) {
                None
            } else {
                panic!("ssa-lower: closure capture `{c}` not in scope")
            }
        })
        .collect();
    // The bool is the BYREF flag: the env slot holds a capture-box
    // pointer (shared live binding) instead of a value snapshot.
    // Copy escape-captures always box (T-15.g.5); a mutated non-Copy
    // binding promotes too (RFC 20260710 — the let-decl site boxed
    // it and recorded the name in `boxed_noncopy_lets`).
    let cap_meta: Vec<(String, Type, bool)> = eff_captures
        .iter()
        .map(|(n, t)| {
            (
                n.clone(),
                *t,
                t.is_copy() || ctx.boxed_noncopy_lets.contains(n),
            )
        })
        .collect();
    ctx.closure_captures.insert(fn_name.clone(), cap_meta);

    // RFC 20260717-namedfn-canonical-cell chunk 1 — a `__forward_*`
    // Closure expr denotes a top-level named fn used as a value, and
    // the ES fn object is a SINGLETON (declaration instantiation
    // creates it once): mint THE cell lazily into the hidden
    // `__fncell_*` slot and answer it from every site, +1 per use
    // (the slot keeps a permanent stake so the cell never hits
    // rc 0). Pre-fix each site minted a fresh env, so `t === u` on
    // two reads of the same fn name answered false and expando
    // writes landed on throwaway cells. Plain arrow / fn-expr
    // evaluations keep the fresh-mint path below (each evaluation
    // is a NEW object per spec).
    if fn_name.starts_with("__forward_") && eff_captures.is_empty() {
        let slot_name = format!("__fncell_{fn_name}");
        let gref = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::GlobalRef(slot_name),
            Type::Ptr,
            None,
        );
        let cached = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(closure_ty, Operand::Value(gref), 0),
            closure_ty,
            None,
        );
        let res_slot = ctx.alloca(closure_ty, Some("__fncell_res"));
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
        let env_v = alloc_env(ctx, closure_ty, 0, &fn_name);
        init_env_header(ctx, env_v, fid, &fn_name);
        // G2 (rotation 178) — a generator factory's `.prototype` IS
        // its `__Gen_<name>` class proto (what getPrototypeOf(g())
        // answers); define it into the fresh cell's props so every
        // member-get channel hits the ordinary props probe.
        if let Some(tag) = fn_name
            .strip_prefix("__forward_")
            .and_then(|n| ctx.ast.generator_factory_classes.get(n))
            .and_then(|cls| ctx.class_name_to_tag.get(cls))
            .copied()
        {
            let proto_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.proto_get,
                    vec![Operand::ConstI64(tag as i64)],
                ),
                Type::Any,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.closure_install_gen_proto,
                    vec![Operand::Value(env_v), Operand::Value(proto_v)],
                ),
            );
        }
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(env_v), Operand::Value(gref), 0),
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(env_v), Operand::Value(res_slot), 0),
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
        return Operand::Value(v);
    }

    let env_v = alloc_env(ctx, closure_ty, eff_captures.len(), &fn_name);
    init_env_header(ctx, env_v, fid, &fn_name);
    write_captures(ctx, env_v, &eff_captures);
    Operand::Value(env_v)
}

fn alloc_env(
    ctx: &mut LowerCtx<'_>,
    closure_ty: Type,
    captures_len: usize,
    fn_name: &str,
) -> crate::ssa::ValueId {
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
    // Universal heap header: refcount=1 at +0, type_tag=CLOSURE=3 at
    // +4 — with FLAG_CLOSURE_RECV_FIRST (bit 12) packed into the
    // flags half-word for a promoted fn-expr face (RFC
    // 20260717-fnexpr-this-channel), so receiver-aware invokers put
    // the receiver in argv[0]. The flag MUST ride this tag-word
    // store: codegen's `emit_store` writes 64 bits regardless of
    // operand width, so every header field write clobbers the next 4
    // bytes and the init sequence stays correct only while offsets
    // ascend. Knife 1 stamped the flag with a second `+4` store
    // AFTER `+8` — silently zeroing `fn_addr`'s low half (latent
    // while faces only rode the boxed entry at +32; knife 2W's
    // direct calls read +8 and jumped to the image base).
    let mut flags: i32 = if ctx.ast.fnexpr_recv_fns.contains(fn_name) {
        1 << 12
    } else {
        0
    };
    // RFC 20260721-builtin-method-reflection 刀 4+9 — fn-flavor bits
    // (torajs-rc FLAG_FN_ASYNC = bit 7 / FLAG_FN_PROTO = bit 15,
    // Tag::Closure-private). Async forms reflect %AsyncFunction% and
    // own no `prototype`; a plain `function` form (expression via the
    // lift side-channel, decl via the `__forward_` singleton) owns
    // the lazily materialized §10.2.5 `.prototype`. Generator
    // factories and async-generator steps keep both bits clear
    // (their reflection surface is the G2 substrate).
    let decl_name = fn_name.strip_prefix("__forward_");
    if decl_name.is_some_and(|n| ctx.ast.generator_factory_classes.contains_key(n)) {
        // Generator factory — [[Prototype]] routes to the genfn trio
        // (RFC 20260721 刀 2); its `.prototype` rides the class-proto
        // channel, so FLAG_FN_PROTO stays off. Async generators keep
        // both flavor bits clear until their reflection lands (G2
        // async half).
        flags |= 1 << 3;
    } else if ctx.ast.fn_async_value_fns.contains(fn_name)
        || decl_name.is_some_and(|n| ctx.ast.async_fns.contains(n))
    {
        flags |= 1 << 7;
    } else if ctx.ast.fn_proto_fns.contains(fn_name)
        || decl_name.is_some_and(|n| !ctx.ast.async_generator_fns.contains(n))
    {
        flags |= 1 << 15;
    }
    let tag_word: i32 = 3 | flags << 16;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI32(1), Operand::Value(env_v), 0),
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI32(tag_word), Operand::Value(env_v), 4),
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
    // trace_fn — the `__env_trace_<fn>` twin the cycle collector
    // walks capture slots through (RFC 20260717 closure-env-cycle
    // knife 2). 0 when no capture can sit on a cycle: the collector
    // treats that as a leaf without the indirect call. obj_alloc is
    // plain malloc so the 0 must be stored explicitly.
    let has_traceable = ctx.closure_captures.get(fn_name).is_some_and(|caps| {
        caps.iter()
            .any(|(_, t, b)| crate::ssa_lower_env_trace::cap_is_traceable(t, *b))
    });
    let trace_op = if has_traceable {
        let trace_name = format!("__env_trace_{fn_name}");
        let trace_fid = *ctx.fn_table.get(&trace_name).unwrap_or_else(|| {
            panic!(
                "ssa-lower: missing pre-registered trace fn `{trace_name}` \
                 for closure `{fn_name}`"
            )
        });
        let trace_sig = *ctx
            .fn_sig_ids
            .get(&trace_fid)
            .expect("trace fn has interned signature");
        let cur_block = ctx.cur_block;
        Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::FnAddr(trace_fid),
            Type::FnSig(trace_sig),
            None,
        ))
    } else {
        Operand::ConstI64(0)
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            trace_op,
            Operand::Value(env_v),
            crate::ssa_lower::CLOSURE_TRACE_FN_OFF,
        ),
    );
    // boxed_entry — the C3a-2 dual `(env, argv, argc) -> AnyValue`
    // adapter's address; 0 when no adapter was synthesized (>8
    // params / non-boxable face) so the dynamic dispatcher answers
    // a catchable TypeError instead of a mis-ABI call.
    let boxed_op = match ctx.boxed_entries.get(&fid) {
        Some(&(bfid, bsig)) => {
            let cur_block = ctx.cur_block;
            Operand::Value(ctx.f.append_inst(
                cur_block,
                InstKind::FnAddr(bfid),
                Type::FnSig(bsig),
                None,
            ))
        }
        None => Operand::ConstI64(0),
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            boxed_op,
            Operand::Value(env_v),
            crate::ssa_lower::CLOSURE_BOXED_ENTRY_OFF,
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

fn write_captures(ctx: &mut LowerCtx<'_>, env_v: crate::ssa::ValueId, captures: &[(String, Type)]) {
    for (i, (cap_name, cap_ty)) in captures.iter().enumerate() {
        let info = *ctx.locals.get(cap_name).expect("capture in scope");
        let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
        if cap_ty.is_copy() || ctx.boxed_noncopy_lets.contains(cap_name) {
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
            // RFC 20260705 Phase 1.5 — shared-capture accounting: the
            // env ALWAYS takes its own +1 stake and the outer binding
            // keeps its own (scope close / reassign releases it; each
            // env-drop releases one reference per non-Copy slot). The
            // Phase 1 hand-the-stake protocol (unmoved outer moved
            // into the env, rc unchanged) freed the cell at env-drop
            // while the outer name stayed live — an inline closure
            // literal passed as a call argument drops its env right
            // after the call, so every later use of the captured
            // binding was a UAF (test262-bug-corpus RC-4 F6).
            if *cap_ty == Type::Any {
                ctx.f.append_void(
                    cur_block,
                    InstKind::Call(ctx.intrinsics.any_box_rc_inc, vec![Operand::Value(v)]),
                );
            } else {
                ctx.emit_rc_inc(Operand::Value(v));
            }
            ctx.f.append_void(
                cur_block,
                InstKind::Store(Operand::Value(v), Operand::Value(env_v), offset),
            );
        }
    }
}
