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
//!      1.5 Top-level data-global promotion (see
//!      [`crate::ssa_lower_toplevel_globals`]).
//!   2. User FnDecl body lowering (see [`crate::ssa_lower_fn::lower_fn`]).
//!   3. Synthesize main from top-level non-FnDecl stmts (see
//!      [`crate::ssa_lower_pass_3`]).
//!      2B. Lifted-closure body lowering — reversed (see
//!      [`crate::ssa_lower_pass_2b`]).
//!      2.5 Populate pre-registered env-drop bodies (see
//!      [`crate::ssa_lower_pass_2_5`]).
//!      T-24 / T-26.C vtable + ClassLayoutMeta emit (see
//!      [`crate::ssa_lower_module_metadata`]).
//!
//! Called through `crate::ssa_lower::lower_with_arity` — the sibling only
//! exposes `pub(crate) fn lower_inner`; downstream ordering knobs stay
//! private to this module.
//!
//! 2026-07-03 fn-debt decomp: Passes 2 / 3 / 2B / 2.5 →
//! [`body_passes`] (dir submodule); mono+num-width / intrinsic
//! batches / top-level globals / module finalize become file-local
//! sub-fns below; env-drop+class-drop+promise+signatures live in
//! [`callable_infra`].

pub(crate) mod body_passes;
pub(crate) mod body_passes_fn_name;
mod callable_infra;
mod declare;
mod setup;

use crate::ast::{GenericStructDecls, PropKey};
pub(crate) use body_passes_fn_name::{
    es_arity, intern_fn_source, register_fn_name, strip_static_method_name,
};
use callable_infra::setup_callable_infra;
use declare::declare_intrinsic_batches;
use setup::{build_anon_stamp_pool_with_snapshot, build_intrinsics_and_boxed_entries};

use std::collections::{HashMap, HashSet};

use crate::ast::Ast;
use crate::check_monomorph::MonoOutput;
use crate::ssa::{self, FuncId, Module, Type};
use crate::ssa_lower_closure_order::CaptureFacts;
use crate::ssa_lower_fn::async_setup;
use crate::ssa_lower_pass_0_5;
use crate::ssa_lower_pass_2_5::cell_size;

pub(crate) fn lower_inner(
    expr_types: &HashMap<crate::ast::ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<crate::ast::ExprId, usize>,
    contextual_any: &std::collections::HashSet<crate::ast::ExprId>,
    mono: &MonoOutput,
    global_spellings: &HashMap<String, String>,
) -> Module {
    // A value class the finished SSA contradicts leaves the set, and
    // the whole module lowers again with its arrays holding cells: the
    // set only shrinks, so this ends. Classes are kept out by name:
    // nothing promises a second lowering numbers its structs alike.
    let mut held_as_cells: HashSet<String> = HashSet::new();
    loop {
        let (module, disagree) = lower_once(
            expr_types,
            arity_pad_count,
            contextual_any,
            mono,
            global_spellings,
            &held_as_cells,
        );
        if disagree.is_empty() {
            return module;
        }
        let before = held_as_cells.len();
        held_as_cells.extend(disagree);
        assert!(
            held_as_cells.len() > before,
            "internal error: a value class contradicted again after it was held as cells"
        );
    }
}

/// One lowering of the module, with the value classes named in
/// `held` kept out; answers the names of the value classes the SSA
/// check contradicted.
fn lower_once(
    expr_types: &HashMap<crate::ast::ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<crate::ast::ExprId, usize>,
    contextual_any: &std::collections::HashSet<crate::ast::ExprId>,
    mono: &MonoOutput,
    global_spellings: &HashMap<String, String>,
    held: &HashSet<String>,
) -> (Module, Vec<String>) {
    // Monomorphization and the W1 num-width analysis ran at the end of
    // the check pipeline (check_monomorph.rs, check_monomorph/finish.rs)
    // — specializations are already appended and type-checked, demoted
    // member-call shapes already restored, and the width table was
    // built over this AST with these retargets.
    let ast: &Ast = &mono.mono_ast;
    let call_retargets = &mono.call_retargets;
    let generic_fn_names = &mono.generic_fn_names;
    let num_f64_slots = &mono.widths;

    let mut module = Module::default();
    let mut fn_table: HashMap<String, FuncId> = HashMap::new();

    let (init_a, init_b, init_c, init_d) = declare_intrinsic_batches(&mut module, &mut fn_table);

    // Pass 0.5: type-alias registration + V3-05 two-phase TypeDecl resolution + Phase H.1.b
    // class tag table + may-throw fixed-point (chunk-328 RFC continuation). All the mutable
    // interner state threaded through Pass 1 / 2 — aliases, arr_layouts, fn_sigs,
    // baked_regex_buf, inst_memo, generic_struct_decls, struct_layouts, class_sids,
    // class_name_to_tag, plus the may_throw set — is built by [`ssa_lower_pass_0_5::run`] and
    // handed back through the `Pass05` holder. struct_layouts is `mem::take`'d off `module`
    // inside the sibling; everything else is fresh.
    let pass05 = ssa_lower_pass_0_5::run(expr_types, &mut module, mono, held);
    let aliases = pass05.aliases;
    let mut arr_layouts = pass05.arr_layouts;
    let mut baked_regex_buf = pass05.baked_regex_buf;
    let mut fn_sigs = pass05.fn_sigs;
    let may_throw = pass05.may_throw;
    let facts = pass05.facts;
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
        num_f64_slots,
        generic_fn_names,
    );
    let mut fn_sig_ids = pass1.fn_sig_ids;
    let (decl_indices, closure_decls) =
        crate::ssa_lower_closure_order::partition_closure_decls(ast, pass1.decl_indices);

    let infra = setup_callable_infra(
        ast,
        &mut module,
        &mut fn_table,
        &mut fn_sigs,
        &mut fn_sig_ids,
        &init_a,
        num_f64_slots,
        &aliases,
        &struct_layouts,
    );

    // M2 — capture-types side channel. The construction site of
    // `Expr::Closure` populates this map (lifted-fn-name → ordered
    // capture types) using the outer scope's local types; the lifted
    // FnDecl's body lowering reads the map to emit env-load preamble
    // instructions for each capture. Construction site always runs
    // before its lifted body in ast.stmts ordering: user FnDecls come
    // first, lifted `__closure_N` decls are appended to the end.
    let mut closure_captures: HashMap<String, Vec<(String, Type, bool)>> = HashMap::new();
    // What each construction site knew of its captures (see `LowerCtx`).
    let mut closure_capture_facts: HashMap<String, CaptureFacts> = HashMap::new();

    let globals = register_toplevel_globals(
        ast,
        &fn_table,
        expr_types,
        global_spellings,
        &aliases,
        &mut arr_layouts,
        &mut fn_sigs,
        &generic_struct_decls,
        &mut struct_layouts,
        &mut inst_memo,
        num_f64_slots,
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
        call_retargets,
        &mut module,
        &mut fn_table,
        &mut fn_sigs,
        &mut fn_sig_ids,
        &anon_stamp_pool,
        &class_name_to_tag,
        &aliases,
        &arr_layouts,
        &struct_layouts,
        infra.env_drop_trivial_fid,
        &init_a,
        &init_b,
        &init_c,
        &init_d,
    );

    let class_drop_fids: HashMap<u32, FuncId> =
        infra.class_drops.iter().map(|d| (d.sid.0, d.fid)).collect();
    let suspend = async_setup::preregister(ast, &mut module, &mut fn_table, &mut fn_sigs);
    let drop_hooks = body_passes::run(
        &suspend,
        decl_indices,
        closure_decls,
        &infra.env_drop_fids,
        &infra.env_trace_fids,
        &infra.class_drops,
        ast,
        &mut module,
        &fn_table,
        &infra.signatures,
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
        &mut closure_capture_facts,
        call_retargets,
        &may_throw,
        &facts,
        &class_name_to_tag,
        &anon_stamp_pool,
        &globals,
        expr_types,
        arity_pad_count,
        contextual_any,
        num_f64_slots,
        &infra.promise_thunks,
        &class_drop_fids,
        &boxed_entries,
    );

    finalize_module(
        &mut module,
        ast,
        &fn_table,
        &fn_sig_ids,
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
    crate::ssa_lower_promise_thunk::twins::push_then_twins(
        &boxed_entries,
        &infra.promise_thunks,
        &mut module,
    );
    let disagree = cell_size::settle(&mut module, &closure_captures, ast, &facts, &drop_hooks);
    (module, struct_names(&aliases, &disagree))
}

/// The names that denote the structs in `sids`.
fn struct_names(aliases: &HashMap<String, Type>, sids: &[ssa::StructId]) -> Vec<String> {
    let names: Vec<String> = (aliases.iter())
        .filter(|(_, t)| matches!(t, Type::Obj(s) if sids.contains(s)))
        .map(|(n, _)| n.clone())
        .collect();
    // an empty answer reads as "no disagreement" and keeps the lowering
    debug_assert!(sids.is_empty() || !names.is_empty());
    names
}

/// Pass 1.5 (K.3) — collect + register top-level data globals.
/// Split 2026-07-03 (fn-debt decomp); body verbatim.
#[allow(clippy::too_many_arguments)]
fn register_toplevel_globals(
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    expr_types: &HashMap<crate::ast::ExprId, crate::check::Type>,
    global_spellings: &HashMap<String, String>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut crate::ssa::ArrLayouts,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &GenericStructDecls,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    num_f64_slots: &crate::num_width::WidthTable,
    module: &mut Module,
) -> HashMap<String, Type> {
    // Pass 1.5 (K.3) — register top-level data globals. Promotion
    // policy (annotation parsing, the K.3b ast_refs gate, and the
    // localize gate that keeps main-only primitive bindings out of
    // the global space) lives in ssa_lower_toplevel_globals.
    // the user functions whose return is an env cell: a call to one
    // initializes a fn-typed global the way a lifted arrow does
    let cell_fns: HashSet<&str> = fn_table
        .iter()
        .filter(|(_, f)| matches!(module.funcs[f.0 as usize].ret, Type::Closure(_)))
        .map(|(n, _)| n.as_str())
        .collect();
    let globals = crate::ssa_lower_toplevel_globals::collect_toplevel_globals(
        ast,
        &cell_fns,
        expr_types,
        global_spellings,
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
            // Rotation 599 — `__fwdrecv_` shells get the same per-name
            // slot the plain forwarders have; without it the widened
            // canonical arm would look up a global nobody emitted.
            if name.starts_with("__forward_") || name.starts_with("__fwdrecv_") {
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
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
    arr_layouts: crate::ssa::ArrLayouts,
    fn_sigs: Vec<(Vec<Type>, Type)>,
    struct_layouts: Vec<Vec<(PropKey, Type)>>,
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
    crate::ssa_lower_module_metadata_vtables::populate_vtables(ast, fn_table, module);
    // 507-02 — one slot, one ABI: the indirect call through a slot
    // takes ITS signature from a single owner, so the table is only
    // callable if every row agrees. Nothing else pins that; the
    // union-find that establishes it (`num_width::slot_abi`) is a
    // different pass with its own idea of which bodies share a slot.
    crate::ssa_lower_module_metadata_slot_abi::assert_vtable_slot_abi(ast, module, fn_sig_ids);
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
        &generic_methods,
        class_name_to_tag,
        module,
    );
}
