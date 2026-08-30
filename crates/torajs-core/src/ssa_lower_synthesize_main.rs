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
    fn_dflt_lits: &crate::ssa_lower_boxed_entry::FnDfltLits,
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
    contextual_any: &std::collections::HashSet<ExprId>,
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
            fn_dflt_lits,
            intrinsics: *intrinsics,
            aliases,
            expr_types,
            arity_pad_count,
            contextual_any,
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
            for_of_teardown_stack: Vec::new(),
            pending_return_slot: None,
            pending_return_flag: None,
            self_name_slot: None,
            pending_break_flag: None,
            pending_continue_flag: None,
            try_finally_loop_depth: Vec::new(),
            locals: HashMap::new(),
            variadic_locals: std::collections::HashSet::new(),
            ns_static_locals: HashMap::new(),
            builtin_mv_locals: HashMap::new(),
            scope_stack: vec![Vec::new()],
            shadow_stack: vec![Vec::new()],
            loop_stack: Vec::new(),
            label_stack: Vec::new(),
            cur_block: entry,
            new_strings: &mut new_strings,
            string_id_base,
            closure_captures,
            call_retargets,
            may_throw_fns,
            escape_captured_lets: std::collections::HashSet::new(),
            mutated_captured_lets: std::collections::HashSet::new(),
            boxed_noncopy_lets: std::collections::HashSet::new(),
            hoisted_closure_lets: std::collections::HashSet::new(),
            forward_capture_boxes: std::collections::HashMap::new(),
            prereserve: crate::ssa_lower_arr_prereserve::PreReserve::new(),
            regex_lit_cache: std::collections::HashMap::new(),
            binop: Default::default(),
            proto_shadow: crate::builtin_proto_shadow::collect_shadowed_builtin_methods(ast),
            bigint_op_may_throw: false,
            globals,
            is_main_fn: true,
            drop_inline_stack: std::collections::HashSet::new(),
            deque_arrs: std::collections::HashSet::new(),
            escape_obj_lets: std::collections::HashSet::new(),
            dynobj_degraded: crate::dynobj_degrade::collect_dynobj_degraded_inits(ast),
            cross_type_widened: crate::let_widen::collect_cross_type_widen_inits(ast),
            nullable_arr_lets: std::collections::HashSet::new(),
            nullable_str_lets: std::collections::HashSet::new(),
            undefable_f64_lets: std::collections::HashSet::new(),
            undefable_f64_fields: std::collections::HashSet::new(),
            undefable_substr_lets: std::collections::HashSet::new(),
            undefable_heap_lets: std::collections::HashSet::new(),
            stack_alloced_locals: std::collections::HashSet::new(),
            let_stack_alloc_hint: None,
            let_declared_obj_layout: None,
            redispatch_lowered: None,
            argv_owned_temps: Vec::new(),
            owned_member_reads: std::collections::HashSet::new(),
            compound_key_memo: None,
        };
        ctx.undefable_f64_fields = crate::undef_f64_fields::collect(&ctx);
        // T-15.g.5 — prime the binding sets BEFORE lowering any
        // top-level let-decl (an unprimed escape-captured `let`
        // stack-allocs and SIGABRTs at env_drop; see the helper doc).
        let _ = ctx.prime_body_binding_sets(stmts.iter().copied());
        crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(
            &mut ctx,
            stmts.iter().copied(),
        );
        let mut prev: Option<&Stmt> = None;
        for s in stmts {
            if !ctx.try_lower_while_fast(prev, s) {
                ctx.lower_top_stmt(s);
            }
            // Terminating statement (top-level throw in particular)
            // closed the block — stop lowering siblings, same as the
            // Block / Multi / try-body / switch-case walks. Without
            // this guard dead siblings append into the terminated
            // block and execute before its terminator.
            if !ctx.cur_open() {
                break;
            }
            prev = Some(*s);
        }
        if ctx.cur_open() {
            // v0.5 T-15.e — drain pending Promise callbacks FIRST,
            // before any top-level teardown: a deferred settle / user
            // `.then` callback runs user code, which must observe the
            // top-level locals, globals and class registry alive.
            // (Draining after the drops ran callbacks against a
            // partially-torn-down world — rotation 348.)
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
            );
            // P10.5-A3-b — sweep unhandled rejections + capture the
            // exit code while the heap is still fully alive. The
            // reporter resolves a rejected reason's `name` through
            // its class prototype chain, and instance→proto edges
            // are borrowed from the class registry, so the sweep
            // must precede the registry-stake drops below (the
            // rotation-348 promise-pool UAF / rc-underflow family).
            let exit_code = crate::ssa_lower_main_exit::emit_exit_code(&mut ctx);
            ctx.emit_drops_for_owned_locals();
            ctx.emit_drops_for_globals();
            // r505 (A12) — the class cells the prologue fn handed to
            // the registry.
            crate::ssa_lower_main_exit::emit_class_cell_registry_release(&mut ctx);
            // V3-10.b — drain the cycle collector buffer one last
            // time before returning from main. Cheap when the
            // buffer is empty; sweeps any orphaned cycles
            // accumulated during program lifetime so they don't
            // leak past process exit.
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.cycle_at_exit_drain, vec![]),
            );
            crate::ssa_lower_main_exit::emit_ret(&mut ctx, exit_code);
        }
    }
    (f, new_strings)
}
