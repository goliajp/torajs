//! What `synthesize_main` lowers the top level against, and the
//! lowering context it builds over it: one for `main` itself, or two
//! when the module awaits at its top level (the module body, and the
//! wrapper `main` that calls it and then drains the job queue).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, ExprId, GenericStructDecls, PropKey};
use crate::ssa::{self, BakedRegexEntry, BlockId, FuncId, Type};
use crate::ssa_lower::{CallRetargets, Intrinsics, LowerCtx};

/// The program-wide tables the top level is lowered against.
pub(crate) struct MainEnv<'a> {
    pub(crate) ast: &'a Ast,
    pub(crate) fn_table: &'a HashMap<String, FuncId>,
    pub(crate) signatures: &'a HashMap<FuncId, Type>,
    pub(crate) fn_sig_ids: &'a HashMap<FuncId, ssa::SigId>,
    pub(crate) fn_dflt_lits: &'a crate::ssa_lower_boxed_entry::FnDfltLits,
    pub(crate) intrinsics: &'a Intrinsics,
    pub(crate) aliases: &'a HashMap<String, Type>,
    pub(crate) arr_layouts: &'a mut ssa::ArrLayouts,
    pub(crate) baked_regex_buf: &'a mut Vec<BakedRegexEntry>,
    pub(crate) fn_sigs: &'a mut Vec<(Vec<Type>, Type)>,
    pub(crate) struct_layouts: &'a mut Vec<Vec<(PropKey, Type)>>,
    pub(crate) inst_memo: &'a mut HashMap<String, ssa::StructId>,
    pub(crate) generic_struct_decls: &'a GenericStructDecls,
    pub(crate) closure_captures: &'a mut HashMap<String, Vec<(String, Type, bool)>>,
    pub(crate) closure_capture_facts:
        &'a mut HashMap<String, crate::ssa_lower_closure_order::CaptureFacts>,
    pub(crate) call_retargets: &'a CallRetargets,
    pub(crate) may_throw_fns: &'a HashSet<String>,
    pub(crate) facts: &'a crate::ssa_lower::proofs::ProgramFactsRc,
    pub(crate) class_name_to_tag: &'a HashMap<String, u32>,
    pub(crate) anon_stamp_pool: &'a crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    pub(crate) globals: &'a HashMap<String, Type>,
    pub(crate) expr_types: &'a HashMap<ExprId, crate::check::Type>,
    pub(crate) arity_pad_count: &'a HashMap<ExprId, usize>,
    pub(crate) contextual_any: &'a HashSet<ExprId>,
    pub(crate) num_f64_slots: &'a crate::num_width::WidthTable,
    pub(crate) promise_thunks: &'a crate::ssa_lower_promise_thunk::PromiseThunks,
    pub(crate) class_drop_fids: &'a HashMap<u32, FuncId>,
    pub(crate) boxed_entries: &'a HashMap<FuncId, (FuncId, ssa::SigId)>,
}

/// Where one context writes: its function, the block it starts in,
/// the string literals it interns (numbered from `string_id_base`).
pub(crate) struct MainTarget<'b> {
    pub(crate) f: &'b mut ssa::Function,
    pub(crate) entry: BlockId,
    pub(crate) new_strings: &'b mut Vec<ssa::StringLiteral>,
    pub(crate) string_id_base: usize,
    /// The body is a function of its own that awaits in place
    /// (`suspend_body`).
    pub(crate) suspend_body: bool,
}

impl<'a> MainEnv<'a> {
    /// A context over `t` for the module top level: `is_main_fn`, no
    /// locals yet, no return annotation.
    pub(crate) fn ctx<'b>(
        &'b mut self,
        t: MainTarget<'b>,
        scans: &'b crate::ssa_lower_ctx_state::ModuleScans,
    ) -> LowerCtx<'b> {
        LowerCtx {
            f: t.f,
            ast: self.ast,
            fn_table: self.fn_table,
            signatures: self.signatures,
            fn_sig_ids: self.fn_sig_ids,
            fn_dflt_lits: self.fn_dflt_lits,
            intrinsics: *self.intrinsics,
            aliases: self.aliases,
            expr_types: self.expr_types,
            arity_pad_count: self.arity_pad_count,
            contextual_any: self.contextual_any,
            num_f64_slots: self.num_f64_slots,
            promise_thunks: self.promise_thunks,
            class_drop_fids: self.class_drop_fids,
            boxed_entries: self.boxed_entries,
            arr_layouts: self.arr_layouts,
            baked_regex_buf: self.baked_regex_buf,
            fn_sigs: self.fn_sigs,
            struct_layouts: self.struct_layouts,
            inst_memo: self.inst_memo,
            generic_struct_decls: self.generic_struct_decls,
            class_name_to_tag: self.class_name_to_tag,
            anon_stamp_pool: self.anon_stamp_pool,
            try_stack: Vec::new(),
            try_finally_stack: Vec::new(),
            for_of_teardown_stack: Vec::new(),
            split_views: Vec::new(),
            pending: Default::default(),
            discarded_stmt_call: None,
            discarded_stmt_expr: None,
            self_name_slot: None,
            try_finally_loop_depth: Vec::new(),
            locals: HashMap::new(),
            variadic_locals: HashSet::new(),
            ns_static_locals: HashMap::new(),
            builtin_mv_locals: HashMap::new(),
            scope_stack: vec![Vec::new()],
            shadow_stack: vec![Vec::new()],
            loop_stack: Vec::new(),
            case_block_depth: 0,
            label_stack: Vec::new(),
            cur_block: t.entry,
            new_strings: t.new_strings,
            string_id_base: t.string_id_base,
            closure_captures: self.closure_captures,
            closure_capture_facts: self.closure_capture_facts,
            call_retargets: self.call_retargets,
            may_throw_fns: self.may_throw_fns,
            escape_captured_lets: HashSet::new(),
            mutated_captured_lets: HashSet::new(),
            boxed_noncopy_lets: HashSet::new(),
            hoisted_closure_lets: HashSet::new(),
            forward_capture_boxes: HashMap::new(),
            prereserve: crate::ssa_lower_arr_prereserve::PreReserve::new(&[]),
            regex_lit_cache: HashMap::new(),
            binop: Default::default(),
            scans,
            bigint_op_may_throw: false,
            globals: self.globals,
            is_main_fn: true,
            suspend_body: t.suspend_body,
            drop_inline_stack: HashSet::new(),
            deque_arrs: HashSet::new(),
            escape_obj_lets: HashSet::new(),
            nullable_arr_lets: HashSet::new(),
            exec_records: HashMap::new(),
            nullable_str_lets: HashSet::new(),
            // Main has no return annotation.
            ret_ann: None,
            let_anns: HashMap::new(),
            undefable_f64_lets: HashSet::new(),
            undefable_f64_fields: HashSet::new(),
            nullable_str_fields: HashSet::new(),
            undefable_substr_lets: HashSet::new(),
            undefable_heap_lets: HashSet::new(),
            num_map_slots: HashSet::new(),
            stack_alloced_locals: HashSet::new(),
            let_stack_alloc_hint: None,
            let_declared_obj_layout: None,
            redispatch_lowered: None,
            temps: Default::default(),
            owned_member_reads: HashSet::new(),
            pins: Default::default(),
            proofs: crate::ssa_lower::proofs::Proofs::new(self.facts),
            proto_keys: Default::default(),
        }
    }
}
