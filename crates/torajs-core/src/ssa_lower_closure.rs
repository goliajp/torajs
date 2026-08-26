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

use crate::ssa::{InstKind, Operand, Type};
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
    // RFC 20260810-indirect-argc-abi S1 — the hidden head is now two
    // slots (`__env` + the SSA-injected I64 `__torajs_argc`), plus
    // the object-literal method's `__this` when present.
    let hidden = 2 + usize::from(fn_has_this_param(ctx, fn_name));
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
    //
    // A class SENTINEL is filtered for the same reason and was missing:
    // `__class_<C>` / `__proto_<C>` are not bindings at all, they are
    // names `ssa_lower_ident` answers by calling `class_get(tag)`, so a
    // body reads one wherever it stands. An arrow in a static member
    // body is how they end up in a capture list — an arrow has no
    // `this` of its own, so the static-`this` mint reaches inside it
    // and `lift_arrow_fns` then sees a free `__class_<C>`:
    //
    //     class C { static u() { const f = () => this; return f() === C } }
    //
    // died on `closure capture __class_C not in scope`. (A function
    // EXPRESSION in the same position was the other half of this and is
    // fixed on the parser side: it binds its own `this`, so the mint
    // must not reach it at all — `parser/fn_expr.rs`.)
    let eff_captures: Vec<(String, Type)> = captures
        .iter()
        .filter_map(|c| {
            if let Some(l) = ctx.locals.get(c) {
                Some((c.clone(), l.ty))
            } else if ctx.globals.contains_key(c) || class_sentinel_name(ctx, c) {
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
    // A capture declared LATER in this list holds a provisional type
    // right now; note where it landed so the declaration can correct it.
    for (i, (cap_name, _)) in eff_captures.iter().enumerate() {
        if let Some(sites) = ctx.forward_capture_boxes.get_mut(cap_name) {
            sites.push((fn_name.clone(), i));
        }
    }
    ctx.closure_captures.insert(fn_name.clone(), cap_meta);

    // Canonical `__fncell_*` singleton arm — carved out to
    // `ssa_lower_closure_canonical.rs` (file-size cap); plain arrow /
    // fn-expr evaluations keep the fresh-mint path below (each
    // evaluation is a NEW object per spec).
    if let Some(v) = crate::ssa_lower_closure_canonical::try_lower_canonical_cell(
        ctx,
        &fn_name,
        fid,
        closure_ty,
        !eff_captures.is_empty(),
    ) {
        return v;
    }

    // §15.5.5 (RFC 20260810) — a named fn-expression gets one extra
    // trailing env slot holding the cell itself (the body preamble
    // binds the self-name from it). The store is a borrowed self-edge:
    // the slot can never outlive the cell, so no rc_inc, no env-drop
    // entry, no trace edge — `cap_meta` stays captures-only and the
    // drop/trace synthesizers follow it.
    let has_self_slot = ctx.ast.closure_self_names.contains_key(&fn_name);
    let env_v = alloc_env(
        ctx,
        closure_ty,
        eff_captures.len() + usize::from(has_self_slot),
        &fn_name,
    );
    init_env_header(ctx, env_v, fid, &fn_name);
    write_captures(ctx, env_v, &eff_captures);
    if has_self_slot {
        let off = CLOSURE_CAP_BASE_OFF + 8 * eff_captures.len() as u64;
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(Operand::Value(env_v), Operand::Value(env_v), off),
        );
    }
    Operand::Value(env_v)
}

pub(crate) fn alloc_env(
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
    // `__fwdrecv_<decl>` is the receiver-first flavor of the same
    // decl singleton (namedfn_recv_cb) — same fn-flavor story: a
    // plain-decl copy owns a `.prototype` (the value-shaped-parent
    // `Object.create(P.prototype)` reads it), an async decl's copy
    // reflects %AsyncFunction%.
    let decl_name = fn_name
        .strip_prefix("__forward_")
        .or_else(|| fn_name.strip_prefix("__fwdrecv_"));
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
        || decl_name.is_some_and(|n| {
            // A class method hoisted to a `__sm_` / `__cm_` decl is
            // still a METHOD: §10.2.5 MakeConstructor never runs for
            // it, so it owns no `.prototype` and no [[Construct]] —
            // the construct kernel keys on this bit (§7.2.4).
            !ctx.ast.async_generator_fns.contains(n)
                && !n.starts_with("__sm_")
                && !n.starts_with("__cm_")
        })
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

pub(crate) fn init_env_header(
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
                InstKind::BoxedEntryAddr(bfid),
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

/// Is `name` a class sentinel — `__class_<C>` or `__proto_<C>` for a
/// class the program actually declared?
///
/// The tag lookup is the load-bearing half: a user-written identifier
/// that merely starts with `__class_` names nothing, and answering
/// `true` for it would drop a genuinely-unresolvable capture silently
/// instead of failing loud.
pub(crate) fn class_sentinel_name(ctx: &LowerCtx<'_>, name: &str) -> bool {
    class_sentinel_name_tag(ctx.class_name_to_tag, name)
}

/// The table-only form of [`class_sentinel_name`], for callers that
/// hold a mutable borrow on another part of the ctx.
pub(crate) fn class_sentinel_name_tag(
    class_name_to_tag: &std::collections::HashMap<String, u32>,
    name: &str,
) -> bool {
    name.strip_prefix("__class_")
        .or_else(|| name.strip_prefix("__proto_"))
        .is_some_and(|c| class_name_to_tag.contains_key(c))
}
