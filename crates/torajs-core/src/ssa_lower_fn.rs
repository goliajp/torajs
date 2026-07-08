//! `lower_fn` extracted from [`crate::ssa_lower`] (chunk 144).
//!
//! Pre-extract this private free fn was 388 LOC inline in ssa_lower.rs
//! (over the 200-line god-fn hard limit per `torajs-file-size-debt`).
//! Body verbatim moved here; the 2 in-ssa_lower.rs callers route
//! through `crate::ssa_lower_fn::lower_fn`. 28-arg signature
//! preserved as-is; a follow-up chunk could ctx-struct'ify it,
//! but verbatim moves preserve byte-equal codegen output and
//! that's the safety bar for this rotation.
//!
//! Chunk 450 decomposed the body under the 200-line fn limit:
//! param materialization (step 4) and the M2 closure-env preamble
//! (step 5) moved verbatim into `LowerCtx` methods below; the twice-
//! repeated W1 f64-slot-promote + container-widen tail on ret/param
//! type resolution deduped into `promote_and_widen`.
//!
//! Builds a `ssa::Function` for a single user FnDecl body:
//! 1. Compute effective ret type (W1 num_width + container widen).
//! 2. Build param SSA values, allocaing + storing each one into a
//!    local slot so body lowering reads via Load uniformly with
//!    let-locals.
//! 3. Construct a `LowerCtx` rooted at the freshly minted entry
//!    block; prime the escape-closure-captures / deque-arr /
//!    escape-obj let-name analyses against the body.
//! 4. Materialize each param as an alloca-backed local (with
//!    refcounted-capture-box for escape-captured Copy params, and
//!    moved+borrowed bookkeeping for `__env` / `__this`).
//! 5. M2 closure-body env preamble — for first-param `__env`, decode
//!    the `__env(c1|c2|...)` annotation, env-load each capture at
//!    offset 8/16/... per the construction-site `closure_captures`
//!    side channel, bind under capture's name as moved + borrowed
//!    (env owns the canonical pointer).
//! 6. Walk body statements; emit drops + implicit ret on fall-
//!    through.

use std::collections::HashMap;

use crate::ast::{self, Ast, ExprId, Stmt};
use crate::ssa::{self, BakedRegexEntry, FuncId, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{
    CLOSURE_CAP_BASE_OFF, CallRetargets, Intrinsics, LocalInfo, LowerCtx, decode_env_ann,
    effective_ret_ty,
};
use crate::ssa_lower_closure_captures::collect_closure_captures_in_stmt;
use crate::ssa_lower_deque_escape::collect_deque_arr_names_in_stmt;
use crate::ssa_lower_obj_escape::collect_escape_obj_let_names_in_stmt;
use crate::ssa_lower_parse_type::parse_type;

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_fn(
    name: &str,
    params: &[ast::Param],
    return_type: Option<&str>,
    body: &[Stmt],
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    signatures: &HashMap<FuncId, Type>,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    intrinsics: &Intrinsics,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    string_id_base: usize,
    closure_captures: &mut HashMap<String, Vec<(String, Type, bool)>>,
    call_retargets: &CallRetargets,
    may_throw_fns: &std::collections::HashSet<String>,
    class_name_to_tag: &HashMap<String, u32>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    globals: &HashMap<String, Type>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<ExprId, usize>,
    num_f64_slots: &crate::num_width::WidthTable,
    promise_thunks: &crate::ssa_lower_promise_thunk::PromiseThunks,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
) -> (ssa::Function, Vec<ssa::StringLiteral>) {
    let ret_ty = promote_and_widen(
        effective_ret_ty(
            parse_type(
                return_type,
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            ),
            ast,
            body,
        ),
        return_type,
        &crate::num_width::SlotKey::Ret(name.to_string()),
        num_f64_slots,
        arr_layouts,
        struct_layouts,
        fn_sigs,
    );
    let mut f = ssa::Function::new(name, ret_ty);

    let mut param_setup: Vec<(String, ValueId, Type)> = Vec::with_capacity(params.len());
    // RFC 20260708-closure-argc-abi chunk 2 — `__clsargc(`-annotated
    // params (mono-instantiated real-argc closure slots) register for
    // the call arm's argc prepend.
    let mut argc_locals: std::collections::HashSet<String> = std::collections::HashSet::new();
    for p in params {
        if p.type_ann
            .as_deref()
            .is_some_and(|a| a.starts_with("__clsargc("))
        {
            argc_locals.insert(p.name.clone());
        }
        let pty = promote_and_widen(
            parse_type(
                p.type_ann.as_deref(),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            ),
            p.type_ann.as_deref(),
            &crate::num_width::SlotKey::Param(name.to_string(), p.name.clone()),
            num_f64_slots,
            arr_layouts,
            struct_layouts,
            fn_sigs,
        );
        let pid = f.add_param(pty, &p.name);
        param_setup.push((p.name.clone(), pid, pty));
    }

    let entry = f.add_block();
    let mut new_strings: Vec<ssa::StringLiteral> = Vec::new();
    let mut ctx = LowerCtx {
        f: &mut f,
        ast,
        fn_table,
        signatures,
        fn_sig_ids,
        intrinsics: *intrinsics,
        aliases,
        expr_types,
        arity_pad_count,
        num_f64_slots,
        promise_thunks,
        boxed_entries,
        arr_layouts,
        baked_regex_buf,
        fn_sigs,
        struct_layouts,
        inst_memo,
        generic_struct_decls,
        class_name_to_tag,
        anon_stamp_pool,
        try_stack: Vec::new(),
        try_finally_stack: Vec::new(),
        try_finally_loop_depth: Vec::new(),
        pending_return_slot: None,
        pending_return_flag: None,
        pending_break_flag: None,
        pending_continue_flag: None,
        locals: HashMap::new(),
        argc_locals,
        scope_stack: vec![Vec::new()],
        shadow_stack: vec![Vec::new()],
        loop_stack: Vec::new(),
        cur_block: entry,
        new_strings: &mut new_strings,
        string_id_base,
        closure_captures,
        call_retargets,
        may_throw_fns,
        escape_captured_lets: std::collections::HashSet::new(),
        push_unchecked_for: std::collections::HashMap::new(),
        regex_lit_cache: std::collections::HashMap::new(),
        binop_left_undef_id: None,
        binop_right_undef_id: None,
        binop_left_null_id: None,
        binop_right_null_id: None,
        binop_mul_square: false,
        bigint_op_may_throw: false,
        globals,
        is_main_fn: false,
        drop_inline_stack: std::collections::HashSet::new(),
        deque_arrs: std::collections::HashSet::new(),
        escape_obj_lets: std::collections::HashSet::new(),
        dynobj_degraded: std::collections::HashSet::new(),
        nullable_arr_lets: std::collections::HashSet::new(),
        nullable_str_lets: std::collections::HashSet::new(),
        undefable_substr_lets: std::collections::HashSet::new(),
        stack_alloced_locals: std::collections::HashSet::new(),
        let_stack_alloc_hint: None,
        redispatch_lowered: None,
        owned_member_reads: std::collections::HashSet::new(),
    };

    for s in body {
        collect_closure_captures_in_stmt(ctx.ast, s, &mut ctx.escape_captured_lets);
    }
    for s in body {
        collect_deque_arr_names_in_stmt(ctx.ast, s, &mut ctx.deque_arrs);
    }
    for s in body {
        collect_escape_obj_let_names_in_stmt(ctx.ast, s, &mut ctx.escape_obj_lets);
    }
    // RC-4 F1c — mirror of the checker's dynobj_degraded set (flat
    // arena scan; see `crate::define_receivers`).
    ctx.dynobj_degraded = crate::define_receivers::collect_defineproperty_receivers(ctx.ast);

    ctx.materialize_fn_params(name, param_setup);
    ctx.emit_closure_env_preamble(name, params);

    let mut prev: Option<&Stmt> = None;
    for s in body {
        if !ctx.try_lower_while_fast(prev, s) {
            ctx.lower_stmt(s);
        }
        prev = Some(s);
    }
    if ctx.cur_open() {
        ctx.emit_drops_for_owned_locals();
        let cb = ctx.cur_block;
        match ctx.f.ret {
            Type::Void => ctx.f.set_term(cb, Terminator::Ret(None)),
            _ => ctx.f.set_term(cb, Terminator::Unreachable),
        }
    }

    (f, new_strings)
}

/// shared tail of ret/param slot type resolution: W1 f64-slot promotion
/// (`number`-annotated I64 slots unified into the f64 width class) then
/// container widen against the same slot key
fn promote_and_widen(
    mut ty: Type,
    type_ann: Option<&str>,
    slot_key: &crate::num_width::SlotKey,
    num_f64_slots: &crate::num_width::WidthTable,
    arr_layouts: &mut Vec<Type>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
) -> Type {
    if ty == Type::I64 && type_ann == Some("number") && num_f64_slots.slot_is_f64(slot_key) {
        ty = Type::F64;
    }
    crate::ssa_lower_container_width::widen_container_ty(
        ty,
        type_ann,
        slot_key,
        num_f64_slots,
        arr_layouts,
        struct_layouts,
        fn_sigs,
    )
}

impl<'a> LowerCtx<'a> {
    /// T-15.g.5 escape-captured Copy binding: box the value on the heap
    /// (16B = rc + value) via `capture_box_alloc`, bit-casting F64 / zero-
    /// extending Bool into the i64 payload slot. Shared by fn-param
    /// materialization here and the let-decl general path.
    pub(crate) fn emit_capture_boxed(&mut self, ty: Type, v: Operand) -> ValueId {
        let init_i64 = if matches!(ty, Type::F64) {
            let b = self.f.append_inst(
                self.cur_block,
                InstKind::BitCastF64ToI64(v),
                Type::I64,
                None,
            );
            Operand::Value(b)
        } else if matches!(ty, Type::Bool) {
            let b = self
                .f
                .append_inst(self.cur_block, InstKind::ZExtBoolToI64(v), Type::I64, None);
            Operand::Value(b)
        } else {
            v
        };
        self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.capture_box_alloc, vec![init_i64]),
            Type::Ptr,
            None,
        )
    }

    /// step 4 of `lower_fn`: materialize each param as an alloca-backed
    /// local (refcounted capture box for escape-captured Copy params;
    /// `__env` / `__cm_*` `__this` marked moved+borrowed — caller owns)
    fn materialize_fn_params(&mut self, fn_name: &str, param_setup: Vec<(String, ValueId, Type)>) {
        for (pname, pid, ty) in param_setup {
            let escape_captured = ty.is_copy() && self.escape_captured_lets.contains(&pname);
            let slot = if escape_captured {
                self.emit_capture_boxed(ty, Operand::Value(pid))
            } else {
                let s = self.alloca(ty, Some(&pname));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(pid), Operand::Value(s), 0),
                );
                s
            };
            let is_env_param = pname == "__env";
            let is_class_self = fn_name.starts_with("__cm_") && pname == "__this";
            let borrows_caller = is_env_param || is_class_self || !ty.is_copy() || escape_captured;
            self.locals.insert(
                pname.clone(),
                LocalInfo {
                    slot,
                    ty,
                    moved: borrows_caller,
                    borrowed: borrows_caller,
                    scope_depth: 0,
                },
            );
            self.scope_stack[0].push(pname);
        }
    }

    /// step 5 of `lower_fn` — M2 closure-body env preamble: for a
    /// first-param `__env`, decode the `__env(c1|c2|...)` annotation and
    /// env-load each capture at offset 8/16/... per the construction-site
    /// `closure_captures` side channel, binding under the capture's name
    /// as moved+borrowed (the env owns the canonical pointer)
    fn emit_closure_env_preamble(&mut self, fn_name: &str, params: &[ast::Param]) {
        let Some(first) = params.first() else { return };
        if first.name != "__env" {
            return;
        }
        let Some(ann) = &first.type_ann else { return };
        let Some(cap_names) = decode_env_ann(ann) else {
            return;
        };
        if cap_names.is_empty() {
            return;
        }
        // The `__env(c1|c2|...)` ann is the PRE-filter capture list —
        // the construction site drops names that resolved to promoted
        // data globals (those read via GlobalRef like named-fn bodies,
        // no env slot), so the side-channel triples are the env-layout
        // ground truth. A body ident not bound here falls through to
        // the globals path in ident resolution.
        let cap_meta: Vec<(String, Type, bool)> = self
            .closure_captures
            .get(fn_name)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "ssa-lower: lifted closure `{fn_name}` has no capture types — \
                     construction site must run before body lowering"
                )
            });
        let env_slot = self
            .locals
            .get("__env")
            .copied()
            .expect("__env param materialized as local")
            .slot;
        for (i, (cap_name, cap_ty, is_byref)) in cap_meta.iter().enumerate() {
            let cap_ty = *cap_ty;
            let is_byref = *is_byref;
            let env_ptr = self.f.append_inst(
                self.cur_block,
                InstKind::Load(Type::Ptr, Operand::Value(env_slot), 0),
                Type::Ptr,
                None,
            );
            let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
            let cap_slot = if cap_ty.is_copy() && is_byref {
                self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(env_ptr), offset),
                    Type::Ptr,
                    None,
                )
            } else {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(cap_ty, Operand::Value(env_ptr), offset),
                    cap_ty,
                    None,
                );
                let local = self.alloca(cap_ty, Some(cap_name));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(v), Operand::Value(local), 0),
                );
                local
            };
            self.locals.insert(
                cap_name.clone(),
                LocalInfo {
                    slot: cap_slot,
                    ty: cap_ty,
                    moved: true,
                    borrowed: true,
                    scope_depth: 0,
                },
            );
            self.scope_stack[0].push(cap_name.clone());
        }
    }
}
