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

mod fresh_this;
mod undef_params;

use crate::ast::{GenericStructDecls, PropKey};
use std::collections::HashMap;

use crate::ast::{self, Ast, ExprId, Stmt};
use crate::ssa::{self, BakedRegexEntry, FuncId, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{CallRetargets, Intrinsics, LowerCtx, effective_ret_ty};
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
    fn_dflt_lits: &crate::ssa_lower_boxed_entry::FnDfltLits,
    intrinsics: &Intrinsics,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    generic_struct_decls: &GenericStructDecls,
    string_id_base: usize,
    closure_captures: &mut HashMap<String, Vec<(String, Type, bool)>>,
    closure_capture_facts: &mut HashMap<String, crate::ssa_lower_closure_order::CaptureFacts>,
    call_retargets: &CallRetargets,
    may_throw_fns: &std::collections::HashSet<String>,
    facts: &crate::ssa_lower::proofs::ProgramFactsRc,
    class_name_to_tag: &HashMap<String, u32>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    globals: &HashMap<String, Type>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<ExprId, usize>,
    contextual_any: &std::collections::HashSet<ExprId>,
    num_f64_slots: &crate::num_width::WidthTable,
    promise_thunks: &crate::ssa_lower_promise_thunk::PromiseThunks,
    class_drop_fids: &std::collections::HashMap<u32, crate::ssa::FuncId>,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
    scans: &crate::ssa_lower_ctx_state::ModuleScans,
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
            params,
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

    let (param_setup, variadic_locals) = setup_fn_params(
        &mut f,
        name,
        params,
        ast.is_replace_view_fn(name),
        crate::ssa_lower_pass_1::hidden_argc_pos(ast, name, params),
        ast.boxed_params_of(name).as_ref(),
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
        num_f64_slots,
    );

    let entry = f.add_block();
    let mut new_strings: Vec<ssa::StringLiteral> = Vec::new();
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
        class_drop_fids,
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
        split_views: Vec::new(),
        try_finally_loop_depth: Vec::new(),
        pending: Default::default(),
        discarded_stmt_call: None,
        discarded_stmt_expr: None,
        self_name_slot: None,
        locals: HashMap::new(),
        variadic_locals,
        ns_static_locals: HashMap::new(),
        builtin_mv_locals: HashMap::new(),
        scope_stack: vec![Vec::new()],
        shadow_stack: vec![Vec::new()],
        loop_stack: Vec::new(),
        case_block_depth: 0,
        label_stack: Vec::new(),
        cur_block: entry,
        new_strings: &mut new_strings,
        string_id_base,
        closure_captures,
        closure_capture_facts,
        call_retargets,
        may_throw_fns,
        escape_captured_lets: std::collections::HashSet::new(),
        mutated_captured_lets: std::collections::HashSet::new(),
        boxed_noncopy_lets: std::collections::HashSet::new(),
        hoisted_closure_lets: std::collections::HashSet::new(),
        forward_capture_boxes: std::collections::HashMap::new(),
        prereserve: crate::ssa_lower_arr_prereserve::PreReserve::new(params),
        regex_lit_cache: std::collections::HashMap::new(),
        binop: Default::default(),
        scans,
        bigint_op_may_throw: false,
        globals,
        is_main_fn: false,
        drop_inline_stack: std::collections::HashSet::new(),
        deque_arrs: std::collections::HashSet::new(),
        escape_obj_lets: std::collections::HashSet::new(),
        nullable_arr_lets: std::collections::HashSet::new(),
        exec_records: HashMap::new(),
        ret_ann: return_type.map(str::to_string),
        let_anns: HashMap::new(),
        nullable_str_lets: std::collections::HashSet::new(),
        undefable_f64_lets: std::collections::HashSet::new(),
        undefable_f64_fields: std::collections::HashSet::new(),
        nullable_str_fields: std::collections::HashSet::new(),
        undefable_substr_lets: std::collections::HashSet::new(),
        undefable_heap_lets: std::collections::HashSet::new(),
        num_map_slots: std::collections::HashSet::new(),
        stack_alloced_locals: std::collections::HashSet::new(),
        let_stack_alloc_hint: None,
        let_declared_obj_layout: None,
        redispatch_lowered: None,
        temps: Default::default(),
        owned_member_reads: std::collections::HashSet::new(),
        pins: Default::default(),
        proofs: crate::ssa_lower::proofs::Proofs::new(facts),
    };

    let assigned_in_body = ctx.prime_body_binding_sets(body.iter());
    crate::undef_f64_fields::prime(&mut ctx);

    ctx.materialize_fn_params(name, param_setup, &assigned_in_body, params);
    crate::ssa_lower::alias_hazard::prime(&mut ctx, body.iter());
    ctx.emit_closure_env_preamble(name, params);
    ctx.emit_gen_env_preamble(name);

    seed_body_facts(&mut ctx, name, params, body);

    let mut prev: Option<&Stmt> = None;
    for s in body {
        if !ctx.try_lower_while_fast(prev, s) {
            ctx.lower_stmt(s);
        }
        // Terminating statement (throw / return / break / continue)
        // closed the block — stop lowering siblings, same as the
        // Block / Multi / try-body / switch-case walks. Without this
        // guard dead siblings append into the terminated block and
        // execute before its terminator.
        if !ctx.cur_open() {
            break;
        }
        prev = Some(s);
    }
    if ctx.cur_open() {
        if name == crate::ast::CLASS_PROLOGUE_FN {
            hand_class_cells_to_registry(&mut ctx);
        }
        ctx.emit_drops_for_owned_locals();
        let cb = ctx.cur_block;
        close_fallthrough_path(&mut ctx, cb);
    }

    (f, new_strings)
}

/// What the body's statements can rely on before the first is lowered.
fn seed_body_facts(ctx: &mut LowerCtx<'_>, name: &str, params: &[ast::Param], body: &[Stmt]) {
    // the captures' annotations, then the parameters', which shadow
    // them; an iterator method's captures are its wrapper's
    let site = match ctx.ast.gen_env_methods.get(name) {
        Some(info) => info.wrapper_fn_name(ctx.ast),
        None => name.to_string(),
    };
    let captured = ctx.closure_capture_facts.get(&site).map(|f| f.anns.clone());
    let params_anns = params
        .iter()
        .filter_map(|p| Some((p.name.clone(), p.type_ann.clone()?)));
    for (n, a) in captured.into_iter().flatten().chain(params_anns) {
        if let Some(l) = ctx.locals.get(&n) {
            ctx.let_anns.insert(l.slot, a);
        }
    }
    undef_params::seed_undef_sentinel_params(ctx, name, params);
    fresh_this::seed_fresh_this(ctx, name, params, body);
    // Mutually recursive closure bindings need each other's boxes open
    // before the first of them mints.
    crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, body.iter());
}

/// r505 (A12) — at the class prologue's fall-through exit, its class
/// object / prototype cells belong to the by-tag registry: the fn
/// hands its +1 over (the locals are marked moved) instead of
/// releasing it, and main's exit releases through the registry
/// (`emit_class_cell_registry_release`). Only the fall-through exit
/// — a throw out of the prologue still runs the ordinary scope drops.
fn hand_class_cells_to_registry(ctx: &mut LowerCtx<'_>) {
    for (lname, info) in ctx.locals.iter_mut() {
        if info.ty == Type::Any
            && crate::ssa_lower_closure::class_sentinel_name_tag(ctx.class_name_to_tag, lname)
        {
            info.moved = true;
        }
    }
}

/// Param materialize prelude of [`lower_fn`] (chunk 775 extraction):
/// parse + width-promote each declared param into an SSA param slot,
/// and register the annotation-keyed variadic lane — RFC
/// 20260708-variadic `variadic_locals`: `__rest(`-bearing anns route
/// the boxed-dual-entry call lane. (The `__clsargc(` argc lane
/// retired in RFC 20260810-indirect-argc-abi S3.4/S3.6.)
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn setup_fn_params(
    f: &mut ssa::Function,
    name: &str,
    params: &[ast::Param],
    view_twin: bool,
    hidden_argc_pos: Option<usize>,
    argv_boxed: Option<&std::collections::HashSet<String>>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &GenericStructDecls,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    num_f64_slots: &crate::num_width::WidthTable,
) -> (
    Vec<(String, ValueId, Type)>,
    std::collections::HashSet<String>,
) {
    let mut param_setup: Vec<(String, ValueId, Type)> = Vec::with_capacity(params.len());
    let mut variadic_locals: std::collections::HashSet<String> = std::collections::HashSet::new();
    // RFC 20260810-indirect-argc-abi H1 — a head-less T-31 body's
    // hidden argc sits at sig position 0 (no head to follow), so the
    // def-side twin binds before the param loop.
    if hidden_argc_pos == Some(0) {
        let apid = f.add_param(Type::I64, "__torajs_argc");
        param_setup.push(("__torajs_argc".to_string(), apid, Type::I64));
    }
    for p in params {
        // Rotation 365 fn-arg track — a param the argv tier marked
        // boxed-only (an argv-face fn-expr flows in directly) routes
        // its calls through the boxed dual entry exactly like a
        // rest-annotated one; the adapter is universal, so any other
        // inflowing closure behaves identically.
        if p.type_ann.as_deref().is_some_and(|a| a.contains("__rest("))
            || argv_boxed.is_some_and(|set| set.contains(&p.name))
        {
            variadic_locals.insert(p.name.clone());
        }
        let mut pty = promote_and_widen(
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
        // the replace lane's view twin reads its strings as views of
        // the subject; Pass 1 gives its signature the same slots
        if view_twin && pty == Type::Str {
            pty = Type::Substr;
        }
        let pid = f.add_param(pty, &p.name);
        param_setup.push((p.name.clone(), pid, pty));
        // RFC 20260810-indirect-argc-abi S1 — def-side twin of the
        // Pass-1 sig injection: an `__env`-first fn takes the hidden
        // I64 `__torajs_argc` right after the env, and the this-first
        // families theirs right after the head (one predicate,
        // `ssa_lower_pass_1::hidden_argc_pos`). Binding it through
        // param_setup gives S2's default guard / arguments.length a
        // named local; bodies that never read it leave a dead slot
        // egraph DCE clears.
        if hidden_argc_pos == Some(param_setup.len()) {
            let apid = f.add_param(Type::I64, "__torajs_argc");
            param_setup.push(("__torajs_argc".to_string(), apid, Type::I64));
        }
    }
    (param_setup, variadic_locals)
}

/// Terminate a block that the body walk left open — the path that
/// runs off the end of the function.
///
/// ES §10.2.1.4 [[Call]] step 11 says that path answers `undefined`,
/// so the question is only how each return width spells it:
///
/// - `void` — the slot's one value already is it.
/// - `number` — an I64 slot has no bit pattern to spare, F64 does.
///   num_width seeds exactly the `number` returns whose body can get
///   here (see [`crate::ast::body_always_terminates`]), so an F64
///   return slot with a still-open block is one of those.
/// - pointer-shaped (`Str` / `Substr` / fn / Obj / Arr / Closure) —
///   the per-type immortal sentinel cell, the same one an optional
///   field or a `find` miss hands out.
///
/// A width with no sentinel yet keeps asserting unreachable, which is
/// what every one of these used to do.
fn close_fallthrough_path(ctx: &mut LowerCtx<'_>, cb: ssa::BlockId) {
    let term = match ctx.f.ret {
        Type::Void => Terminator::Ret(None),
        Type::F64 => Terminator::Ret(Some(Operand::ConstF64(f64::from_bits(
            crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS,
        )))),
        // An `any` return spells its undefined as the immediate
        // ANY_UNDEF box (tag 5, payload 0), not a heap cell — the
        // implicit-generics pass routes every value-returning fn
        // whose body can fall through to an `any` ret precisely so
        // this path has a spelling (a Bool/I64 ret has none, and
        // the open block used to terminate `unreachable`: running
        // off the end of `if (c) return true;` trapped).
        Type::Any => {
            let b = ctx.f.append_inst(
                cb,
                ssa::InstKind::BoxAny(5, Operand::ConstI64(0)),
                Type::Any,
                None,
            );
            Terminator::Ret(Some(Operand::Value(b)))
        }
        ret_ty => match ctx.str_undef_sentinel_for(ret_ty) {
            Some(cell) => Terminator::Ret(Some(cell)),
            None => Terminator::Unreachable,
        },
    };
    ctx.f.set_term(cb, term);
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
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
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
