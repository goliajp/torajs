//! `Checker::new` and the full-pipeline driver — the constructor's
//! field initializer list is the one thing in `check.rs` that grows
//! with every table the checker gains. Split out at the 500-line
//! limit (rotation 635); a child module reaches the parent's private
//! fields directly, so nothing changed visibility.

use std::collections::HashMap;

use crate::ast::Ast;

use super::Checker;

impl Checker {
    pub(crate) fn new() -> Self {
        Self {
            globals: HashMap::new(),
            global_spellings: HashMap::new(),
            hoisted_closure_lets: HashMap::new(),
            scopes: vec![HashMap::new()],
            aliases: HashMap::new(),
            class_structs: HashMap::new(),
            class_names: std::collections::HashSet::new(),
            source_dunder_idents: std::collections::HashSet::new(),
            errors: Vec::new(),
            expected_return: None,
            current_class: None,
            closure_captures: HashMap::new(),
            closure_fn_names: std::collections::HashSet::new(),
            dynobj_degraded: std::collections::HashSet::new(),
            tdz_reads: std::collections::HashSet::new(),
            proto_shadow: Default::default(),
            declared_privates: Default::default(),
            cross_type_widened: std::collections::HashSet::new(),
            shadowed_reads: Default::default(),
            held_any: Default::default(),
            any_promoted_inits: std::collections::HashSet::new(),
            generic_type_params: HashMap::new(),
            generic_call_sites: HashMap::new(),
            any_widen_mono_sites: HashMap::new(),
            widen_ret_any: std::collections::HashSet::new(),
            widen_ret_declared: false,
            hidden_any_args: std::collections::HashSet::new(),
            fn_slot_ret_any: std::collections::HashSet::new(),
            fn_slot_kept: HashMap::new(),
            let_face_anns: HashMap::new(),
            face_mismatch_lets: std::collections::HashSet::new(),
            face_mismatch_params: std::collections::HashSet::new(),
            face_slots: Default::default(),
            face_slot_reads: Default::default(),
            ret_owner: None,
            any_struct_arg_callees: HashMap::new(),
            fn_escape_widen_sites: HashMap::new(),
            arity_pad_count: HashMap::new(),
            checking_spec_body: false,
            closure_walk_memo: HashMap::new(),
            speculative_depth: 0,
            defer_closure_bodies: 0,
            generic_alias_decls: HashMap::new(),
            fn_defaults: HashMap::new(),
            expr_types: HashMap::new(),
            unions: Default::default(),
            demoted_cm_rewrites: HashMap::new(),
            contextual_any_literals: std::collections::HashSet::new(),
            iter_destr_srcs: HashMap::new(),
            undeclared_reads: std::collections::HashMap::new(),
            self_name_active: None,
            self_name_writes: std::collections::HashSet::new(),
            const_writes: std::collections::HashSet::new(),
            spec_throws: std::collections::HashMap::new(),
            unresolved_captures: HashMap::new(),
            widen_clone_origin: HashMap::new(),
            toplevel_captures: Vec::new(),
            narrows: Default::default(),
        }
    }

    pub(crate) fn run_full_pipeline(&mut self, ast: &Ast) {
        // RC-4 F1c — defineProperty receivers must type as `any` so
        // the dynobj write-back can rebind them (see
        // `crate::dynobj_degrade`). Collected before the passes so
        // the LetDecl arm sees the set.
        self.dynobj_degraded = crate::dynobj_degrade::collect_dynobj_degraded_inits(ast);
        // §8.1.1.1 — the pipeline already settled which top-level
        // reads sit in a dead zone (`ast::mark_tdz_reads`).
        // Carried, not recomputed: the answer is about STATEMENT
        // POSITION in the tree the pipeline finished, and this side
        // has no other way to ask.
        self.tdz_reads = ast.tdz_reads.clone();
        // RFC 20260806 — what this module might patch onto a builtin
        // prototype, so a member read of such a method answers Any and
        // the call takes the dispatcher lane instead of a kernel.
        let seed = std::mem::take(&mut self.proto_shadow);
        self.proto_shadow = crate::builtin_proto_shadow::collect_shadowed_builtin_methods(ast);
        self.proto_shadow.carry_exec_written(&seed);
        self.declared_privates = crate::check_private_name::declared_private_names(ast);
        crate::iface_witness::install(ast);
        // RFC 20260804-mutable-let-widen — cross-family reassigned
        // mutable lets type `any` from declaration (same shared-set
        // architecture; the lowerer recomputes from its own snapshot).
        // Joined onto what the caller seeded: the type half a previous
        // run found (`check_entry::check_to_fixpoint`).
        let syntactic = crate::let_widen::collect_cross_type_widen_inits(ast, None);
        self.cross_type_widened.extend(syntactic);
        // Three native passes split out into `check_pipeline` sibling
        // (chunk 136). Order matters: Pass 1 reads aliases populated
        // by Pass 0; Pass 2 reads globals populated by Pass 1 and
        // the pre-pass.
        crate::check_pipeline::pass_0_register_type_aliases(self, ast);
        crate::check_type_ann::check_dict_sites(self, ast);
        crate::check_pipeline::pass_1_hoist_fn_signatures(self, ast);
        crate::check_pipeline::pass_2_register_globals_and_check_stmts(self, ast);
    }
}
