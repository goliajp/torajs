//! `lower_inner` — top-level lowering driver extracted from `ssa_lower.rs`
//! chunk 369.
//!
//! Invoked once per compile from `ssa_lower::lower_with_arity`. Runs the
//! whole SSA lowering pipeline in order:
//!   0. M3 generic monomorphization (see [`crate::ssa_lower_generics_monomorph`]).
//!   0. W1 num_width analysis (see [`crate::num_width`]).
//!   0. Intrinsic-declare batches A/B/C/D (see the four
//!      [`crate::ssa_lower_intrinsics_init_a`]-style siblings).
//!   0.5 Type-alias + TypeDecl + class-tag + may-throw (see
//!       [`crate::ssa_lower_pass_0_5`]).
//!   1. Pre-allocate user-fn FuncIds + record return types (see
//!      [`crate::ssa_lower_pass_1`]).
//!   1.5 Top-level data-global promotion (see
//!       [`crate::ssa_lower_toplevel_globals`]).
//!   2. User FnDecl body lowering (see [`crate::ssa_lower_fn::lower_fn`]).
//!   3. Synthesize main from top-level non-FnDecl stmts (see
//!      [`crate::ssa_lower_pass_3`]).
//!   2B. Lifted-closure body lowering — reversed (see
//!       [`crate::ssa_lower_pass_2b`]).
//!   2.5 Populate pre-registered env-drop bodies (see
//!       [`crate::ssa_lower_pass_2_5`]).
//!   T-24 / T-26.C vtable + ClassLayoutMeta emit (see
//!   [`crate::ssa_lower_module_metadata`]).
//!
//! Called through `crate::ssa_lower::lower_with_arity` — the sibling only
//! exposes `pub(crate) fn lower_inner`; downstream ordering knobs stay
//! private to this module.

use std::collections::HashMap;

use crate::ast::{Ast, Stmt};
use crate::check::GenericCallSites;
use crate::ssa::{self, FnNameEntry, FuncId, Module, Type};
use crate::ssa_lower::monomorphize_generics;

pub(crate) fn lower_inner(
    ast: &Ast,
    generic_call_sites: &GenericCallSites,
    expr_types: &HashMap<crate::ast::ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<crate::ast::ExprId, usize>,
    demoted_cm_rewrites: &HashMap<crate::ast::ExprId, crate::ast::ExprId>,
) -> Module {
    // M3 — produce monomorphized FnDecls from each generic call site,
    // and a per-call-site `ExprId → mono_name` retarget map. We clone
    // the AST so the appended mono FnDecls don't mutate the caller's
    // copy (cheap: the AST is a few thousand exprs at most). The
    // monomorphizer needs a `&mut Ast` so it can fabricate new Ident
    // expressions when transitively-rewriting inner generic-call
    // callees in cloned bodies (class methods calling each other with
    // shared type params).
    let mut owned_ast: Ast = ast.clone();
    // Restore the member-call shape at demoted speculative rewrites
    // BEFORE monomorphization, so cloned generic bodies and num_width
    // see the builtin dispatch shape (mechanism: cm_demote.rs).
    for (&call_eid, &alt_eid) in demoted_cm_rewrites {
        owned_ast.exprs[call_eid.0 as usize] = owned_ast.exprs[alt_eid.0 as usize].clone();
    }
    let (mono_decls, call_retargets, generic_fn_names) =
        monomorphize_generics(&mut owned_ast, generic_call_sites);
    owned_ast.stmts.extend(mono_decls);
    let ast: &Ast = &owned_ast;

    // W1 (ann-width RFC) — module-wide number-slot width inference.
    // Single ground truth for every `: number` (or un-annotated
    // number) slot's I64-vs-F64 representation; consumers below gate
    // on the annotation and the `__` synthetic-fn exclusion.
    let num_f64_slots = crate::num_width::analyze(ast, &call_retargets, demoted_cm_rewrites);

    let mut module = Module::default();
    let mut fn_table: HashMap<String, FuncId> = HashMap::new();

    // Pass 0 batch A — print / obj_capture / arr / str_a / num / str_b
    // (6 sub-systems, 76 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_a`] (chunk-323 of the
    // ssa_lower.rs god-file + lower_inner god-fn decomp). The
    // `init_a.<group>.<field>` references appear directly in the
    // `Intrinsics { ... }` literal below; no local *_id vars are
    // needed because nothing between here and the literal reads them.
    let init_a = crate::ssa_lower_intrinsics_init_a::declare(&mut module, &mut fn_table);
    // Pass 0 batch B — regex / date / fs / process / arr_any / object
    // (6 sub-systems, 121 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_b`] (chunk-324 RFC continuation).
    let init_b = crate::ssa_lower_intrinsics_init_b::declare(&mut module, &mut fn_table);
    // Pass 0 batch C — any_substrate / print_freeze / bigint / weak /
    // map_set / runtime_misc (6 sub-systems, 123 FuncIds) declare via
    // aggregator in [`crate::ssa_lower_intrinsics_init_c`] (chunk-325
    // RFC continuation). Sibling also folds in the `gc` alias insert
    // and the `main_exit` / `process_on` declares that immediately
    // followed runtime_misc in the original Pass 0.
    let init_c = crate::ssa_lower_intrinsics_init_c::declare(&mut module, &mut fn_table);
    // Pass 0 batch D — promise / substr / substr_trim_into /
    // arr_str_etc / str_extra / math / json_misc / throw (8
    // sub-systems, 170 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_d`] (chunk-326 RFC final
    // batch). After this Pass 0 is fully drained from `lower_inner`.
    let init_d = crate::ssa_lower_intrinsics_init_d::declare(&mut module, &mut fn_table);

    // Pass 0.5: type-alias registration + V3-05 two-phase TypeDecl
    // resolution + Phase H.1.b class tag table + may-throw fixed-point
    // (chunk-328 RFC continuation). All the mutable interner state
    // threaded through Pass 1 / 2 — aliases, arr_layouts, fn_sigs,
    // baked_regex_buf, inst_memo, generic_struct_decls, struct_layouts,
    // class_sids, class_name_to_tag, plus the may_throw set — is built
    // by [`crate::ssa_lower_pass_0_5::run`] and handed back through the
    // `Pass05` holder. struct_layouts is `mem::take`'d off `module`
    // inside the sibling; everything else is fresh.
    let pass05 = crate::ssa_lower_pass_0_5::run(ast, expr_types, &mut module, &num_f64_slots);
    let aliases = pass05.aliases;
    let mut arr_layouts = pass05.arr_layouts;
    let mut baked_regex_buf = pass05.baked_regex_buf;
    let mut fn_sigs = pass05.fn_sigs;
    let may_throw = pass05.may_throw;
    let generic_struct_decls = pass05.generic_struct_decls;
    let mut struct_layouts = pass05.struct_layouts;
    let mut inst_memo = pass05.inst_memo;
    let class_name_to_tag = pass05.class_name_to_tag;

    // Pass 1: pre-allocate FuncIds + record correct return types for
    // every user FnDecl, with Pass 0.4 (Pass-0 intrinsic signatures
    // → fn_sig_ids) folded in. Delegated to
    // [`crate::ssa_lower_pass_1::run`] (chunk-329 RFC continuation
    // after Pass 0.5 sibling in chunk-328); see that module's doc for
    // the full pipeline rationale and W1 / W4 widening semantics.
    let pass1 = crate::ssa_lower_pass_1::run(
        ast,
        &mut module,
        &mut fn_table,
        &aliases,
        &mut arr_layouts,
        &mut fn_sigs,
        &generic_struct_decls,
        &mut struct_layouts,
        &mut inst_memo,
        &num_f64_slots,
        &generic_fn_names,
    );
    let decl_indices = pass1.decl_indices;
    let mut fn_sig_ids = pass1.fn_sig_ids;
    // M2.A fix — lifted closures (`__closure_N`) must lower in REVERSE
    // append order so each closure's CONSTRUCTION site (in its enclosing
    // fn / outer closure) runs before its BODY (which reads
    // `closure_captures` populated by the construction). Without this
    // reorder, nested capturing closures crashed: __closure_0 (innermost)
    // is appended first by lift_arrow_fns and would lower first, but its
    // captures are populated by __closure_1 (outer)'s body lowering.
    //
    // T-15.g.5 extension: closure construction can also live at module
    // top-level (`let cb = function(v) { return v + cap }` directly in
    // implicit main). Top-level construction only runs when synthesize_
    // main lowers, so closure bodies that depend on top-level captures
    // must lower AFTER main, not just after user fns. Pipeline now:
    // Pass 2A user fns → Pass 3 main → Pass 2B closure bodies (reverse).
    let (user_decls, mut closure_decls): (Vec<_>, Vec<_>) =
        decl_indices
            .into_iter()
            .partition(|(stmt_idx, _)| match &ast.stmts[*stmt_idx] {
                Stmt::FnDecl { name, .. } => !name.starts_with("__closure_"),
                _ => true,
            });
    closure_decls.reverse();
    let decl_indices: Vec<_> = user_decls;

    // Env-drop fn infrastructure (per-closure pre-allocate + trivial
    // wrapper) — see [`crate::ssa_lower_env_drop_setup`].
    let env_drop_setup = crate::ssa_lower_env_drop_setup::run(
        ast,
        &mut module,
        &mut fn_table,
        &mut fn_sigs,
        &mut fn_sig_ids,
        &init_a,
    );
    let env_drop_fids = env_drop_setup.env_drop_fids;
    let env_drop_trivial_fid = env_drop_setup.env_drop_trivial_fid;

    // ②.6b — promise callback ABI thunks (bits-adapters for f64-faced
    // `.then` / `.catch` handlers). Synthesized here because the fn
    // list freezes at the signatures snapshot below; modules without
    // a promise chain synthesize nothing.
    let promise_thunks = crate::ssa_lower_promise_thunk::synthesize_promise_thunks(
        ast,
        &num_f64_slots,
        &mut module,
        &mut fn_table,
        &mut fn_sigs,
        &mut fn_sig_ids,
        init_a.obj_capture.obj_drop_sized,
        init_a.obj_capture.value_drop_heap,
    );

    // Snapshot every callable's return type — used inside lower_fn to type
    // call-site results correctly.
    let signatures: HashMap<FuncId, Type> = module
        .funcs
        .iter()
        .enumerate()
        .map(|(i, f)| (FuncId(i as u32), f.ret))
        .collect();

    let intrinsics = crate::ssa_lower_intrinsics_table::build(
        env_drop_trivial_fid,
        &init_a,
        &init_b,
        &init_c,
        &init_d,
    );

    // (struct_layouts already detached from module at top of lower(),
    // see M3.4 block above; write-back happens at the end.)

    // M2 — capture-types side channel. The construction site of
    // `Expr::Closure` populates this map (lifted-fn-name → ordered
    // capture types) using the outer scope's local types; the lifted
    // FnDecl's body lowering reads the map to emit env-load preamble
    // instructions for each capture. Construction site always runs
    // before its lifted body in ast.stmts ordering: user FnDecls come
    // first, lifted `__closure_N` decls are appended to the end.
    let mut closure_captures: HashMap<String, Vec<(Type, bool)>> = HashMap::new();

    // Pass 1.5 (K.3) — register top-level data globals. Promotion
    // policy (annotation parsing, the K.3b ast_refs gate, and the
    // localize gate that keeps main-only primitive bindings out of
    // the global space) lives in ssa_lower_toplevel_globals.
    let globals = crate::ssa_lower_toplevel_globals::collect_toplevel_globals(
        ast,
        &aliases,
        &mut arr_layouts,
        &mut fn_sigs,
        &generic_struct_decls,
        &mut struct_layouts,
        &mut inst_memo,
        &num_f64_slots,
    );
    let mut data_globals_out: Vec<ssa::DataGlobal> = globals
        .iter()
        .map(|(name, ty)| ssa::DataGlobal {
            name: name.clone(),
            ty: *ty,
        })
        .collect();
    data_globals_out.sort_by(|a, b| a.name.cmp(&b.name));
    module.data_globals = data_globals_out;

    /* W-J Phase A1 (RFC 20260614-w-j-struct-reflect §3) — anon_sid_to_tag
     * snapshot for ObjectLit alloc stamping. Build at Pass 1.5 boundary
     * (just after collect_toplevel_globals) so the Pass 2 lowerers can
     * stamp `class_tag@+8` on anonymous ObjectLit allocations via the map.
     *
     * MVP scope: sids visible at snapshot time get their stamp; sids
     * interned later inside Pass 2 (e.g. generic mono spawning a fresh
     * `{a:1}` shape per specialization) fall back to `unwrap_or(0)` and
     * stay anon-untagged. The downstream reflection consumers (Phase B+)
     * see those as "no struct layout" — graceful degradation, not a crash.
     *
     * Tag space layout (must align with the class_layouts emit loop's
     * push order — A0 + this anon push both walk `struct_layouts.iter()
     * .enumerate().filter(|(idx,_)| !named_sids.contains(idx))`):
     *   - [1 .. n_named]                                = named classes
     *   - [n_named+1 .. n_named+n_anon] (per anon_idx)  = anonymous structs
     * Cycle visitor indexes class_layouts via `class_tag - 1`, so the
     * vec push order must mirror this enumeration. */
    let anon_stamp_pool = crate::ssa_lower_anon_stamp::build_snapshot_pool(
        &class_name_to_tag,
        &aliases,
        &struct_layouts,
    );

    // Pass 2: lower user FnDecl bodies. Each call returns the lowered
    // function plus any string literals interned during its body; we
    // append those into module.strings before the next call so the
    // StringId counter stays in lockstep with module.strings.len().
    for (stmt_idx, fid) in decl_indices {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = &ast.stmts[stmt_idx]
        {
            let string_id_base = module.strings.len();
            let (f, new_strings) = crate::ssa_lower_fn::lower_fn(
                name,
                params,
                return_type.as_deref(),
                body,
                ast,
                &fn_table,
                &signatures,
                &fn_sig_ids,
                &intrinsics,
                &aliases,
                &mut arr_layouts,
                &mut baked_regex_buf,
                &mut fn_sigs,
                &mut struct_layouts,
                &mut inst_memo,
                &generic_struct_decls,
                string_id_base,
                &mut closure_captures,
                &call_retargets,
                &may_throw,
                &class_name_to_tag,
                &anon_stamp_pool,
                &globals,
                expr_types,
                arity_pad_count,
                &num_f64_slots,
                &promise_thunks,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
            // Fn-name registry Step 2 — record the (FuncId, name,
            // name_sid) triple for the link-time __torajs_fn_name_table
            // emit (Step 3) + the runtime __torajs_fn_print_inline
            // binary search (Step 4). Skip the desugared class-method
            // mangled forms (`__cm_<C>__<m>`, `__dispatch_<m>`,
            // `__new_<C>`) — bun reports the user-visible method
            // name on those, not the mangled name, and we get
            // there in Step 5's wire by stripping the prefix when
            // emitting. Skip generic-mono specialized names too
            // (`<fn>__<typeargs>__<idx>`) — they share the source
            // fn's user-visible name; the entry already exists for
            // the generic form. Closure-lifted bodies
            // (`__closure_*`) are anonymous from the user's point
            // of view; runtime falls back to
            // `[Function (anonymous)]` if no entry is found.
            if !name.starts_with("__cm_")
                && !name.starts_with("__dispatch_")
                && !name.starts_with("__new_")
                && !name.starts_with("__closure_")
                && !name.contains("__mono_")
            {
                // Intern the name as a Module-level string literal so
                // the link layer can resolve `__user_string_<sid>` to
                // the rodata cstring entry. encode_from_str picks
                // Latin-1 / UTF-16 to match the upstream string-literal
                // encoding contract (TS allows non-ASCII fn names).
                let lit = ssa::StringLiteral::encode_from_str(name);
                let name_sid = ssa::StringId(module.strings.len() as u32);
                module.strings.push(lit);
                module.fn_name_globals.push(FnNameEntry {
                    fn_id: fid,
                    name: name.clone(),
                    name_sid,
                });
            }
        }
    }

    // Pass 3: synthesize `main` from top-level non-FnDecl statements.
    // Delegated to [`crate::ssa_lower_pass_3::run`].
    crate::ssa_lower_pass_3::run(
        ast,
        &mut module,
        &fn_table,
        &signatures,
        &fn_sig_ids,
        &intrinsics,
        &aliases,
        &mut arr_layouts,
        &mut baked_regex_buf,
        &mut fn_sigs,
        &mut struct_layouts,
        &mut inst_memo,
        &generic_struct_decls,
        &mut closure_captures,
        &call_retargets,
        &may_throw,
        &class_name_to_tag,
        &anon_stamp_pool,
        &globals,
        expr_types,
        arity_pad_count,
        &num_f64_slots,
        &promise_thunks,
    );

    // Pass 2B (T-15.g.5): lower lifted-closure bodies. Delegated to
    // [`crate::ssa_lower_pass_2b::run`].
    crate::ssa_lower_pass_2b::run(
        closure_decls,
        ast,
        &mut module,
        &fn_table,
        &signatures,
        &fn_sig_ids,
        &intrinsics,
        &aliases,
        &mut arr_layouts,
        &mut baked_regex_buf,
        &mut fn_sigs,
        &mut struct_layouts,
        &mut inst_memo,
        &generic_struct_decls,
        &mut closure_captures,
        &call_retargets,
        &may_throw,
        &class_name_to_tag,
        &anon_stamp_pool,
        &globals,
        expr_types,
        arity_pad_count,
        &num_f64_slots,
        &promise_thunks,
    );

    // Pass 2.5: synthesize each pre-registered env-drop fn body now
    // that closure_captures is populated. Delegated to
    // [`crate::ssa_lower_pass_2_5::populate_env_drop_bodies`].
    crate::ssa_lower_pass_2_5::populate_env_drop_bodies(
        &env_drop_fids,
        &closure_captures,
        &intrinsics,
        &arr_layouts,
        &struct_layouts,
        &mut module,
    );

    module.arr_layouts = arr_layouts;
    module.signatures = fn_sigs;
    module.struct_layouts = struct_layouts;
    module.baked_regex_entries = baked_regex_buf;

    // T-24 vtables (per-class slot dispatch) + T-26.C named-class
    // ClassLayoutMeta + W-J Phase A0 anonymous-struct ClassLayoutMeta
    // — see [`crate::ssa_lower_module_metadata`] for the consolidated
    // builder docs.
    crate::ssa_lower_module_metadata::populate_vtables(ast, &fn_table, &mut module);
    crate::ssa_lower_module_metadata::populate_class_layouts(
        &class_name_to_tag,
        &aliases,
        &mut module,
    );

    // W-J Phase A1 follow-up — append `ClassLayoutMeta` rows for
    // each Pass 2 fresh sid recorded in `anon_stamp_pool`.
    crate::ssa_lower_anon_stamp::append_fresh_class_layouts(
        &anon_stamp_pool,
        &module.struct_layouts.clone(),
        &mut module.class_layouts,
    );

    module
}
