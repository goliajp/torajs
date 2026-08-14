//! `lower_inner` — top-level lowering driver extracted from `ssa_lower.rs`
//! chunk 369.
//!
//! Invoked once per compile from `ssa_lower::lower_with_arity`. Runs the
//! whole SSA lowering pipeline in order:
//!   0. W1 num_width analysis (see [`crate::num_width`]) over the
//!      check-side monomorphization output (`check_monomorph.rs` —
//!      the mono AST + call retargets arrive via `CheckArtifacts`).
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
//!
//! 2026-07-03 fn-debt decomp: Passes 2 / 3 / 2B / 2.5 →
//! [`body_passes`] (dir submodule); mono+num-width / intrinsic
//! batches / env-drop+promise+signatures / top-level globals /
//! module finalize become file-local sub-fns below.

mod body_passes;
mod setup;

pub(crate) use body_passes::{intern_fn_source, strip_static_method_name};
use setup::{build_anon_stamp_pool_with_snapshot, build_intrinsics_and_boxed_entries};

use std::collections::HashMap;

use crate::ast::{Ast, Stmt};
use crate::check_monomorph::MonoOutput;
use crate::ssa::{self, FuncId, Module, Type};

pub(crate) fn lower_inner(
    expr_types: &HashMap<crate::ast::ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<crate::ast::ExprId, usize>,
    demoted_cm_rewrites: &HashMap<crate::ast::ExprId, crate::ast::ExprId>,
    contextual_any: &std::collections::HashSet<crate::ast::ExprId>,
    mono: &MonoOutput,
) -> Module {
    // Monomorphization ran at the end of the check pipeline
    // (check_monomorph.rs) — specializations are already appended and
    // type-checked, demoted member-call shapes already restored. W1
    // num-width analysis is all that remains of the old pass 0.
    let ast: &Ast = &mono.mono_ast;
    let call_retargets = &mono.call_retargets;
    let generic_fn_names = &mono.generic_fn_names;
    let num_f64_slots =
        crate::num_width::analyze(ast, call_retargets, demoted_cm_rewrites, expr_types);

    let mut module = Module::default();
    let mut fn_table: HashMap<String, FuncId> = HashMap::new();

    let (init_a, init_b, init_c, init_d) = declare_intrinsic_batches(&mut module, &mut fn_table);

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
        generic_fn_names,
    );
    let mut fn_sig_ids = pass1.fn_sig_ids;
    let (decl_indices, closure_decls) = partition_closure_decls(ast, pass1.decl_indices);

    let (env_drop_fids, env_drop_trivial_fid, env_trace_fids, promise_thunks, signatures) =
        setup_callable_infra(
            ast,
            &mut module,
            &mut fn_table,
            &mut fn_sigs,
            &mut fn_sig_ids,
            &init_a,
            &num_f64_slots,
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
    let mut closure_captures: HashMap<String, Vec<(String, Type, bool)>> = HashMap::new();

    let globals = register_toplevel_globals(
        ast,
        expr_types,
        &aliases,
        &mut arr_layouts,
        &mut fn_sigs,
        &generic_struct_decls,
        &mut struct_layouts,
        &mut inst_memo,
        &num_f64_slots,
        &mut module,
    );

    let (anon_stamp_pool, struct_layouts_pass15_len) =
        build_anon_stamp_pool_with_snapshot(&class_name_to_tag, &aliases, &struct_layouts);

    // S2.36 — intrinsics + boxed-adapter synthesis moved AFTER the
    // stamp-pool build: the adapters' struct-param coercion call
    // needs each Obj param sid's class_tag, resolved through the
    // same named/anon split the ObjectLit alloc stamping uses.
    // Nothing between the old position and here consumed either
    // output (globals registration is data-slot work).
    let (intrinsics, boxed_entries) = build_intrinsics_and_boxed_entries(
        ast,
        &mut module,
        &mut fn_table,
        &mut fn_sigs,
        &mut fn_sig_ids,
        &anon_stamp_pool,
        &class_name_to_tag,
        &aliases,
        env_drop_trivial_fid,
        &init_a,
        &init_b,
        &init_c,
        &init_d,
    );

    body_passes::run(
        decl_indices,
        closure_decls,
        &env_drop_fids,
        &env_trace_fids,
        ast,
        &mut module,
        &fn_table,
        &signatures,
        &fn_sig_ids,
        &pass1.fn_dflt_lits,
        &intrinsics,
        &aliases,
        &mut arr_layouts,
        &mut baked_regex_buf,
        &mut fn_sigs,
        &mut struct_layouts,
        &mut inst_memo,
        &generic_struct_decls,
        &mut closure_captures,
        call_retargets,
        &may_throw,
        &class_name_to_tag,
        &anon_stamp_pool,
        &globals,
        expr_types,
        arity_pad_count,
        contextual_any,
        &num_f64_slots,
        &promise_thunks,
        &boxed_entries,
    );

    finalize_module(
        &mut module,
        ast,
        &fn_table,
        &boxed_entries,
        arr_layouts,
        fn_sigs,
        struct_layouts,
        baked_regex_buf,
        &class_name_to_tag,
        &aliases,
        &anon_stamp_pool,
        struct_layouts_pass15_len,
    );

    module
}

/// M2.A fix — lifted closures (`__closure_N`) must lower in REVERSE
/// append order so each closure's CONSTRUCTION site (in its enclosing
/// fn / outer closure) runs before its BODY (which reads
/// `closure_captures` populated by the construction). Without this
/// reorder, nested capturing closures crashed: __closure_0 (innermost)
/// is appended first by lift_arrow_fns and would lower first, but its
/// captures are populated by __closure_1 (outer)'s body lowering.
///
/// T-15.g.5 extension: closure construction can also live at module
/// top-level (`let cb = function(v) { return v + cap }` directly in
/// implicit main). Top-level construction only runs when synthesize_
/// main lowers, so closure bodies that depend on top-level captures
/// must lower AFTER main, not just after user fns. Pipeline now:
/// Pass 2A user fns → Pass 3 main → Pass 2B closure bodies (reverse).
fn partition_closure_decls(
    ast: &Ast,
    decl_indices: Vec<(usize, FuncId)>,
) -> (Vec<(usize, FuncId)>, Vec<(usize, FuncId)>) {
    let (user_decls, mut closure_decls): (Vec<_>, Vec<_>) =
        decl_indices
            .into_iter()
            .partition(|(stmt_idx, _)| match &ast.stmts[*stmt_idx] {
                Stmt::FnDecl { name, .. } => !name.starts_with("__closure_"),
                _ => true,
            });
    closure_decls.reverse();
    // Reverse append equals parent-before-child only for parser-shaped
    // nesting; a closure minted by the capturing-nested-fn route sits
    // at the arena tail and needs the topological pass to lower after
    // the closure that constructs it.
    let closure_decls = crate::ssa_lower_closure_order::order_closure_bodies(ast, closure_decls);
    (user_decls, closure_decls)
}

/// Pass 0 — the four intrinsic-declare batches (A/B/C/D aggregator
/// siblings). Split 2026-07-03 (fn-debt decomp); bodies verbatim.
fn declare_intrinsic_batches(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> (
    crate::ssa_lower_intrinsics_init_a::InitA,
    crate::ssa_lower_intrinsics_init_b::InitB,
    crate::ssa_lower_intrinsics_init_c::InitC,
    crate::ssa_lower_intrinsics_init_d::InitD,
) {
    // Pass 0 batch A — print / obj_capture / arr / str_a / num / str_b
    // (6 sub-systems, 76 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_a`] (chunk-323 of the
    // ssa_lower.rs god-file + lower_inner god-fn decomp). The
    // `init_a.<group>.<field>` references appear directly in the
    // `Intrinsics { ... }` literal below; no local *_id vars are
    // needed because nothing between here and the literal reads them.
    let init_a = crate::ssa_lower_intrinsics_init_a::declare(module, fn_table);
    // Pass 0 batch B — regex / date / fs / process / arr_any / object
    // (6 sub-systems, 121 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_b`] (chunk-324 RFC continuation).
    let init_b = crate::ssa_lower_intrinsics_init_b::declare(module, fn_table);
    // Pass 0 batch C — any_substrate / print_freeze / bigint / weak /
    // map_set / runtime_misc (6 sub-systems, 123 FuncIds) declare via
    // aggregator in [`crate::ssa_lower_intrinsics_init_c`] (chunk-325
    // RFC continuation). Sibling also folds in the `gc` alias insert
    // and the `main_exit` / `process_on` declares that immediately
    // followed runtime_misc in the original Pass 0.
    let init_c = crate::ssa_lower_intrinsics_init_c::declare(module, fn_table);
    // Pass 0 batch D — promise / substr / substr_trim_into /
    // arr_str_etc / str_extra / math / json_misc / throw (8
    // sub-systems, 170 FuncIds) declare via aggregator in
    // [`crate::ssa_lower_intrinsics_init_d`] (chunk-326 RFC final
    // batch). After this Pass 0 is fully drained from `lower_inner`.
    let init_d = crate::ssa_lower_intrinsics_init_d::declare(module, fn_table);
    (init_a, init_b, init_c, init_d)
}

/// Env-drop fn infrastructure + promise-callback ABI thunks + the
/// callables return-type snapshot. Split 2026-07-03 (fn-debt
/// decomp); bodies verbatim.
#[allow(clippy::too_many_arguments)]
fn setup_callable_infra(
    ast: &Ast,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    init_a: &crate::ssa_lower_intrinsics_init_a::InitA,
    num_f64_slots: &crate::num_width::WidthTable,
) -> (
    Vec<(String, FuncId, ssa::SigId)>,
    (FuncId, ssa::SigId),
    Vec<(String, FuncId)>,
    crate::ssa_lower_promise_thunk::PromiseThunks,
    HashMap<FuncId, Type>,
) {
    // Env-drop fn infrastructure (per-closure pre-allocate + trivial
    // wrapper) — see [`crate::ssa_lower_env_drop_setup`].
    let env_drop_setup =
        crate::ssa_lower_env_drop_setup::run(ast, module, fn_table, fn_sigs, fn_sig_ids, init_a);
    let env_drop_fids = env_drop_setup.env_drop_fids;
    let env_drop_trivial_fid = env_drop_setup.env_drop_trivial_fid;
    let env_trace_fids = env_drop_setup.env_trace_fids;

    // ②.6b — promise callback ABI thunks (bits-adapters for f64-faced
    // `.then` / `.catch` handlers). Synthesized here because the fn
    // list freezes at the signatures snapshot below; modules without
    // a promise chain synthesize nothing.
    let promise_thunks = crate::ssa_lower_promise_thunk::synthesize_promise_thunks(
        ast,
        num_f64_slots,
        module,
        fn_table,
        fn_sigs,
        fn_sig_ids,
        init_a.obj_capture.obj_drop_sized,
        init_a.obj_capture.value_drop_heap,
        init_a.obj_capture.cycle_unbuffer,
    );

    // Snapshot every callable's return type — used inside lower_fn to type
    // call-site results correctly.
    let signatures: HashMap<FuncId, Type> = module
        .funcs
        .iter()
        .enumerate()
        .map(|(i, f)| (FuncId(i as u32), f.ret))
        .collect();
    (
        env_drop_fids,
        env_drop_trivial_fid,
        env_trace_fids,
        promise_thunks,
        signatures,
    )
}

/// Pass 1.5 (K.3) — collect + register top-level data globals.
/// Split 2026-07-03 (fn-debt decomp); body verbatim.
#[allow(clippy::too_many_arguments)]
fn register_toplevel_globals(
    ast: &Ast,
    expr_types: &HashMap<crate::ast::ExprId, crate::check::Type>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    num_f64_slots: &crate::num_width::WidthTable,
    module: &mut Module,
) -> HashMap<String, Type> {
    // Pass 1.5 (K.3) — register top-level data globals. Promotion
    // policy (annotation parsing, the K.3b ast_refs gate, and the
    // localize gate that keeps main-only primitive bindings out of
    // the global space) lives in ssa_lower_toplevel_globals.
    let globals = crate::ssa_lower_toplevel_globals::collect_toplevel_globals(
        ast,
        expr_types,
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
        num_f64_slots,
    );
    let mut data_globals_out: Vec<ssa::DataGlobal> = globals
        .iter()
        .map(|(name, ty)| ssa::DataGlobal {
            name: name.clone(),
            ty: *ty,
        })
        .collect();
    // RFC 20260717-namedfn-canonical-cell chunk 1 — one hidden
    // zero-init slot per forwarder fn; `ssa_lower_closure`'s
    // canonical arm lazily mints THE fn-object cell into it so every
    // named-fn value site answers the same singleton (ES identity).
    // Deliberately NOT in `globals`: no user-name resolution and no
    // exit drop — the fn object lives for the program, bun-equal.
    // Chunk 2 extends the same singleton scheme to the naked
    // accessor-face mint (fns the forwarder collector doesn't
    // rewrite — nested `__nested___top_*` lifts): one
    // `__fncell_naked_*` slot per FnDecl, 8 zero-init bytes each,
    // lazily minted only if a face ever evaluates.
    for stmt in &ast.stmts {
        if let crate::ast::Stmt::FnDecl { name, .. } = stmt {
            if name.starts_with("__forward_") {
                data_globals_out.push(ssa::DataGlobal {
                    name: format!("__fncell_{name}"),
                    ty: Type::Ptr,
                });
            } else {
                data_globals_out.push(ssa::DataGlobal {
                    name: format!("__fncell_naked_{name}"),
                    ty: Type::Ptr,
                });
            }
        }
    }
    data_globals_out.sort_by(|a, b| a.name.cmp(&b.name));
    module.data_globals = data_globals_out;
    globals
}

/// Interner write-backs + vtable / ClassLayoutMeta emit. Split
/// 2026-07-03 (fn-debt decomp); body verbatim.
#[allow(clippy::too_many_arguments)]
fn finalize_module(
    module: &mut Module,
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
    arr_layouts: Vec<Type>,
    fn_sigs: Vec<(Vec<Type>, Type)>,
    struct_layouts: Vec<Vec<(String, Type)>>,
    baked_regex_buf: Vec<ssa::BakedRegexEntry>,
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    struct_layouts_pass15_len: usize,
) {
    module.arr_layouts = arr_layouts;
    module.signatures = fn_sigs;
    module.struct_layouts = struct_layouts;
    module.baked_regex_entries = baked_regex_buf;

    // T-24 vtables (per-class slot dispatch) + T-26.C named-class
    // ClassLayoutMeta + W-J Phase A0 anonymous-struct ClassLayoutMeta
    // — see [`crate::ssa_lower_module_metadata`] for the consolidated
    // builder docs.
    crate::ssa_lower_module_metadata::populate_vtables(ast, fn_table, module);
    let generic_methods = crate::ssa_lower_module_metadata::populate_class_layouts(
        ast,
        fn_table,
        boxed_entries,
        class_name_to_tag,
        aliases,
        module,
        struct_layouts_pass15_len,
    );

    // W-J Phase A1 follow-up — append `ClassLayoutMeta` rows for
    // each Pass 2 fresh tag recorded in `anon_stamp_pool` (plain
    // anon sids + the 405-03 generic-factory rows, merged by tag).
    crate::ssa_lower_anon_stamp::append_fresh_class_layouts(
        anon_stamp_pool,
        &module.struct_layouts.clone(),
        &generic_methods,
        &mut module.class_layouts,
    );
}
