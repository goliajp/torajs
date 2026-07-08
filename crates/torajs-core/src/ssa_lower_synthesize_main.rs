//! `declare_intrinsic` + `synthesize_main` extracted from `ssa_lower.rs`
//! chunk 367.
//!
//! - `declare_intrinsic` — helper used at intrinsic-table registration
//!   sites (and by `ssa_lower_substr_trim_into::declare_all`).
//! - `synthesize_main` — top-level driver that constructs a `LowerCtx`
//!   over the program's top-level stmts, primes escape-capture / deque /
//!   escape-obj sets, walks each top-level stmt, and drains microtasks +
//!   cycle-collector before returning.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::ssa::{self, BakedRegexEntry, FuncId, InstKind, Module, Type};
use crate::ssa_lower::{CallRetargets, Intrinsics, LowerCtx};
use crate::ssa_lower_closure_captures::collect_closure_captures_in_stmt;
use crate::ssa_lower_deque_escape::collect_deque_arr_names_in_stmt;
use crate::ssa_lower_obj_escape::collect_escape_obj_let_names_in_stmt;

pub(crate) fn declare_intrinsic(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    name: &str,
    param_tys: &[Type],
    ret_ty: Type,
) -> FuncId {
    let mut f = ssa::Function::new(name, ret_ty);
    for (i, &t) in param_tys.iter().enumerate() {
        f.add_param(t, &format!("a{i}"));
    }
    // No blocks → declaration only; backend supplies the body.
    let id = FuncId(module.funcs.len() as u32);
    fn_table.insert(name.to_string(), id);
    module.funcs.push(f);
    id
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_main(
    stmts: &[&Stmt],
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
    let mut f = ssa::Function::new("main", Type::I32);
    let entry = f.add_block();
    let mut new_strings: Vec<ssa::StringLiteral> = Vec::new();
    {
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
            pending_return_slot: None,
            pending_return_flag: None,
            pending_break_flag: None,
            pending_continue_flag: None,
            try_finally_loop_depth: Vec::new(),
            locals: HashMap::new(),
            argc_locals: std::collections::HashSet::new(),
            variadic_locals: std::collections::HashSet::new(),
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
            is_main_fn: true,
            drop_inline_stack: std::collections::HashSet::new(),
            deque_arrs: std::collections::HashSet::new(),
            escape_obj_lets: std::collections::HashSet::new(),
            dynobj_degraded: crate::define_receivers::collect_defineproperty_receivers(ast),
            nullable_arr_lets: std::collections::HashSet::new(),
            nullable_str_lets: std::collections::HashSet::new(),
            undefable_substr_lets: std::collections::HashSet::new(),
            stack_alloced_locals: std::collections::HashSet::new(),
            let_stack_alloc_hint: None,
            redispatch_lowered: None,
            owned_member_reads: std::collections::HashSet::new(),
        };
        // T-15.g.5 fix: prime escape_captured_lets BEFORE lowering any
        // top-level let-decl. Without this, top-level `let x = 10` in
        // a program that later does `let cb = function() { return x }`
        // alloca's x on stack; the closure construction stores that
        // stack pointer into env+CAP_OFFSET; env_drop then calls
        // obj_drop(stack_ptr) → "pointer being freed was not allocated"
        // SIGABRT during shutdown. lower_fn does the same prime walk
        // for user fn bodies; synthesize_main was missing it.
        for s in stmts {
            collect_closure_captures_in_stmt(ctx.ast, s, &mut ctx.escape_captured_lets);
        }
        // 11-A1 — prime deque-unsafe Array binding set.
        for s in stmts {
            collect_deque_arr_names_in_stmt(ctx.ast, s, &mut ctx.deque_arrs);
        }
        // 11-A2-a — prime escape-bound Obj-typed binding set.
        for s in stmts {
            collect_escape_obj_let_names_in_stmt(ctx.ast, s, &mut ctx.escape_obj_lets);
        }
        let mut prev: Option<&Stmt> = None;
        for s in stmts {
            if !ctx.try_lower_while_fast(prev, s) {
                ctx.lower_top_stmt(s);
            }
            prev = Some(*s);
        }
        if ctx.cur_open() {
            ctx.emit_drops_for_owned_locals();
            ctx.emit_drops_for_globals();
            // v0.5 T-15.e — drain pending Promise callbacks before
            // process exit. Cheap no-op when the queue is empty (one
            // fn call + one mt_len_ load + branch-not-taken). Emitted
            // unconditionally so async-unaware programs still get
            // correct semantics if they import a module that schedules
            // microtasks at top level.
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
            );
            // V3-10.b — drain the cycle collector buffer one last
            // time before returning from main. Cheap when the
            // buffer is empty; sweeps any orphaned cycles
            // accumulated during program lifetime so they don't
            // leak past process exit.
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.cycle_at_exit_drain, vec![]),
            );
            crate::ssa_lower_main_exit::emit_ret(&mut ctx);
        }
    }
    (f, new_strings)
}
