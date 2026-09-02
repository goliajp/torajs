//! Passes 2 / 3 / 2B / 2.5 of `lower_inner` — user-fn body lowering
//! (+ fn-name registry rows), `main` synthesis, lifted-closure body
//! lowering (reverse order), and env-drop body population, split
//! from `ssa_lower_inner.rs` (2026-07-03, fn-debt decomp). Bodies
//! verbatim; sibling of `ssa_lower_pass_3.rs` / `ssa_lower_pass_2b.rs`
//! — this module owns the Pass 2 loop and sequences the four passes.

use crate::ast::PropKey;
use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, FnNameEntry, FuncId, Module, Type};
use crate::ssa_lower::Intrinsics;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    decl_indices: Vec<(usize, FuncId)>,
    closure_decls: Vec<(usize, FuncId)>,
    env_drop_fids: &[(String, FuncId, ssa::SigId)],
    env_trace_fids: &[(String, FuncId)],
    ast: &Ast,
    module: &mut Module,
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
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    closure_captures: &mut HashMap<String, Vec<(String, Type, bool)>>,
    closure_variadic_captures: &mut HashMap<String, Vec<String>>,
    call_retargets: &HashMap<ExprId, String>,
    may_throw: &std::collections::HashSet<String>,
    class_name_to_tag: &HashMap<String, u32>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    globals: &HashMap<String, Type>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<ExprId, usize>,
    contextual_any: &std::collections::HashSet<ExprId>,
    num_f64_slots: &WidthTable,
    promise_thunks: &crate::ssa_lower_promise_thunk::PromiseThunks,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
) {
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
            span,
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
                fn_table,
                signatures,
                fn_sig_ids,
                fn_dflt_lits,
                intrinsics,
                aliases,
                arr_layouts,
                baked_regex_buf,
                fn_sigs,
                struct_layouts,
                inst_memo,
                generic_struct_decls,
                string_id_base,
                closure_captures,
                closure_variadic_captures,
                call_retargets,
                may_throw,
                class_name_to_tag,
                anon_stamp_pool,
                globals,
                expr_types,
                arity_pad_count,
                contextual_any,
                num_f64_slots,
                promise_thunks,
                boxed_entries,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
            register_fn_name(module, name, params, fid, ast, *span, boxed_entries);
        }
    }

    // Pass 3: synthesize `main` from top-level non-FnDecl statements.
    // Delegated to [`crate::ssa_lower_pass_3::run`].
    crate::ssa_lower_pass_3::run(
        ast,
        module,
        fn_table,
        signatures,
        fn_sig_ids,
        fn_dflt_lits,
        intrinsics,
        aliases,
        arr_layouts,
        baked_regex_buf,
        fn_sigs,
        struct_layouts,
        inst_memo,
        generic_struct_decls,
        closure_captures,
        closure_variadic_captures,
        call_retargets,
        may_throw,
        class_name_to_tag,
        anon_stamp_pool,
        globals,
        expr_types,
        arity_pad_count,
        contextual_any,
        num_f64_slots,
        promise_thunks,
        boxed_entries,
    );

    // Pass 2B (T-15.g.5): lower lifted-closure bodies. Delegated to
    // [`crate::ssa_lower_pass_2b::run`].
    crate::ssa_lower_pass_2b::run(
        closure_decls,
        ast,
        module,
        fn_table,
        signatures,
        fn_sig_ids,
        fn_dflt_lits,
        intrinsics,
        aliases,
        arr_layouts,
        baked_regex_buf,
        fn_sigs,
        struct_layouts,
        inst_memo,
        generic_struct_decls,
        closure_captures,
        closure_variadic_captures,
        call_retargets,
        may_throw,
        class_name_to_tag,
        anon_stamp_pool,
        globals,
        expr_types,
        arity_pad_count,
        contextual_any,
        num_f64_slots,
        promise_thunks,
        boxed_entries,
    );

    // Pass 2.5: synthesize each pre-registered env-drop fn body now
    // that closure_captures is populated. Delegated to
    // [`crate::ssa_lower_pass_2_5::populate_env_drop_bodies`].
    crate::ssa_lower_pass_2_5::populate_env_drop_bodies(
        env_drop_fids,
        closure_captures,
        intrinsics,
        module,
    );

    // Pass 2.5b: the paired env-trace fn bodies (RFC 20260717
    // closure-env-cycle knife 2) from the same closure_captures
    // truth — [`crate::ssa_lower_pass_2_5::populate_env_trace_bodies`].
    crate::ssa_lower_pass_2_5::populate_env_trace_bodies(
        env_trace_fids,
        closure_captures,
        fn_sigs,
        module,
    );
}

/// Fn-name registry Step 2 — record the (FuncId, name, name_sid)
/// triple for the link-time __torajs_fn_name_table emit (Step 3) +
/// the runtime __torajs_fn_print_inline binary search (Step 4).
/// Skip the desugared mangled forms (`__dispatch_<m>`, `__new_<C>`)
/// — bun reports the user-visible method name on those, not the
/// mangled name, and we get there in Step 5's wire by stripping the
/// prefix when emitting. Skip generic-mono specialized names too
/// (`<fn>__<typeargs>__<idx>`) — they share the source fn's
/// user-visible name; the entry already exists for the generic form.
/// Closure-lifted bodies (`__closure_*`) are anonymous from the
/// user's point of view; runtime falls back to
/// `[Function (anonymous)]` if no entry is found.
///
/// RFC 20260719-fn-tostring-source B6c — a dispatchable class-method
/// body (`__cm_<C>__<m>` with a synthesized boxed adapter) registers
/// against its ADAPTER's fn id: the reified `C.prototype.<m>` face
/// cell carries the adapter vaddr (its own fn_addr is the throwing
/// native entry), so toString/name/length resolve the user-visible
/// row through it. Ctor bodies, accessor bodies (their face carries
/// its own name/length meta), and adapter-less dropouts stay out.
fn register_fn_name(
    module: &mut Module,
    name: &str,
    params: &[crate::ast::Param],
    fid: FuncId,
    ast: &Ast,
    span: crate::lexer::Span,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
) {
    let class_parents = &ast.class_parents;
    if name.starts_with("__cm_") {
        let Some(mname) = strip_mangled_method_name(name, "__cm_", class_parents) else {
            return;
        };
        if mname == "ctor" || mname.is_empty() {
            return;
        }
        let Some(&(adapter_fid, _)) = boxed_entries.get(&fid) else {
            return;
        };
        if ast.accessor_getters.values().any(|f| f == name)
            || ast.accessor_setters.values().any(|f| f == name)
        {
            return;
        }
        let lit = ssa::StringLiteral::encode_from_str(mname);
        let name_sid = ssa::StringId(module.strings.len() as u32);
        module.strings.push(lit);
        let arity = params
            .iter()
            .filter(|p| p.name != "__env" && p.name != "__this")
            .take_while(|p| p.default.is_none() && !p.is_rest)
            .count() as u32;
        let (src_sid, src_len) = intern_fn_source(module, ast, span);
        module.fn_name_globals.push(FnNameEntry {
            fn_id: adapter_fid,
            name: mname.to_string(),
            name_sid,
            arity,
            src_sid,
            src_len,
        });
        return;
    }
    if name.starts_with("__dispatch_")
        || name.starts_with("__new_")
        || name.starts_with("__closure_")
        || name.starts_with("__bind_create_")
        || name.contains("__mono_")
    {
        return;
    }
    // `__forward_<target>` fn-value wrappers (ast_closure_param_tag)
    // carry the user-visible TARGET name — `const t: any = topfn;
    // t.name` must answer "topfn", and the synthetic leading `__env`
    // param stays out of the arity (chunk 716). `__bound_<fn>_<id>`
    // bind-desugar wrappers (ast_desugar_function_prototype_methods)
    // carry the ES SetFunctionName form `bound <fn>` (§20.2.3.2) —
    // chunk 798; the trailing `_<id>` disambiguator is stripped. The
    // `__bind_create_<fn>_<id>` factory twins are call-only synthetics
    // the user never holds a value of, so they skip the table above.
    let bound_form = name
        .strip_prefix("__bound_")
        .and_then(|rest| rest.rsplit_once('_'))
        .map(|(target, _id)| format!("bound {target}"));
    let visible = bound_form.as_deref().unwrap_or_else(|| {
        let base = name.strip_prefix("__forward_").unwrap_or(name);
        // 423-01 deconflict — a module-mangled decl (`__m<k>_<name>`)
        // answers the user spelling, mirroring the namespace object's
        // FIELD face (`B.tag.name` must say "tag").
        let base = strip_module_mangle(base);
        // RFC 20260729-fn-value-any V4 刀 2 — a hoisted generator
        // EXPRESSION answers the NamedEvaluation verdict the hoist
        // pass recorded (binding name, or its own self-name, or the
        // empty ES name), never the `__genexpr_N` mint. Generator
        // DECLARATIONS never land a row here and keep their own name.
        if let Some(n) = ast.genexpr_names.get(base) {
            return n.as_str();
        }
        // `__sm_<C>__<M>` static-method bodies carry the ES
        // SetFunctionName form — the property key `<M>` (`K.sf.name`
        // answered the mangled name). `<C>` is matched against the
        // known class set (longest first) since both a class name and
        // a method name may themselves contain `__`.
        strip_static_method_name(base, class_parents).unwrap_or(base)
    });
    // Intern the name as a Module-level string literal so the link
    // layer can resolve `__user_string_<sid>` to the rodata cstring
    // entry. encode_from_str picks Latin-1 / UTF-16 to match the
    // upstream string-literal encoding contract (TS allows non-ASCII
    // fn names).
    let lit = ssa::StringLiteral::encode_from_str(visible);
    let name_sid = ssa::StringId(module.strings.len() as u32);
    module.strings.push(lit);
    // ES-spec `Function.length` — leading params before the first
    // default / rest (§10.2.10 SetFunctionLength).
    let arity = params
        .iter()
        .filter(|p| p.name != "__env")
        .take_while(|p| p.default.is_none() && !p.is_rest)
        .count() as u32;
    let (src_sid, src_len) = intern_fn_source(module, ast, span);
    module.fn_name_globals.push(FnNameEntry {
        fn_id: fid,
        name: visible.to_string(),
        name_sid,
        arity,
        src_sid,
        src_len,
    });
    // 398-05 — a STATIC method body registers a second row against
    // its ADAPTER fid, the `__cm_` mirror: the class-object own entry
    // (`__torajs_class_static_method_define`) is a reified cell
    // carrying the `__sm_` adapter's vaddr, and the any-lane
    // `.length` / `.name` / toString reads resolve through
    // `registry_addr` = that adapter. The body-fid row above stays —
    // the compile-time-folded `S.s` read path answers off it.
    if name.starts_with("__sm_")
        && let Some(&(adapter_fid, _)) = boxed_entries.get(&fid)
    {
        module.fn_name_globals.push(FnNameEntry {
            fn_id: adapter_fid,
            name: visible.to_string(),
            name_sid,
            arity,
            src_sid,
            src_len,
        });
    }
}

/// RFC 20260719-fn-tostring-source B3b — intern the type-erased
/// source slice for a registry row. Sentinel (0,0) spans (synthesized
/// decls with no user-written source) answer `(None, 0)`, which the
/// link layer bakes as a NULL `src_ptr`.
pub(crate) fn intern_fn_source(
    module: &mut Module,
    ast: &Ast,
    span: crate::lexer::Span,
) -> (Option<ssa::StringId>, u32) {
    if span.start == 0 && span.end == 0 {
        return (None, 0);
    }
    let erased = crate::fn_source_erase::erase_types(&ast.source, &ast.type_ann_spans, span);
    let lit = ssa::StringLiteral::encode_from_str(&erased);
    let src_len = lit.length;
    let src_sid = ssa::StringId(module.strings.len() as u32);
    module.strings.push(lit);
    (Some(src_sid), src_len)
}

/// `__sm_<C>__<M>` → `<M>` when `<C>` is a declared class name.
/// Longest class-name match wins — `class A__B { static f() {} }`
/// desugars to `__sm_A__B__f`, where a shortest-match would answer
/// `B__f`. Shared with the static `.name` member fold
/// (`ssa_lower_member_fn_intro`), which sees the same mangled ident
/// after the checker rewrites `K.sf` to `Ident("__sm_K__sf")`.
/// `__m<k>_<name>` → `<name>` — the 423-01 module-deconflict mangle
/// shape (`__m` + decimal sequence + `_`). Anything else — including
/// other `__m…` synthetics whose next byte is not a digit — passes
/// through untouched.
pub(crate) fn strip_module_mangle(n: &str) -> &str {
    let Some(rest) = n.strip_prefix("__m") else {
        return n;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return n;
    }
    match rest[digits..].strip_prefix('_') {
        Some(user) if !user.is_empty() => user,
        _ => n,
    }
}

pub(crate) fn strip_static_method_name<'a>(
    name: &'a str,
    class_parents: &HashMap<String, Option<String>>,
) -> Option<&'a str> {
    strip_mangled_method_name(name, "__sm_", class_parents)
}

/// `<prefix><C>__<M>` → `<M>` when `<C>` is a declared class name,
/// longest class-name match winning (see
/// [`strip_static_method_name`]). B6c generalization — the `__cm_`
/// instance-method registry rows strip the same mangled shape.
pub(crate) fn strip_mangled_method_name<'a>(
    name: &'a str,
    prefix: &str,
    class_parents: &HashMap<String, Option<String>>,
) -> Option<&'a str> {
    let rest = name.strip_prefix(prefix)?;
    class_parents
        .keys()
        .filter_map(|c| {
            rest.strip_prefix(c.as_str())
                .and_then(|r| r.strip_prefix("__"))
                .map(|m| (c.len(), m))
        })
        .max_by_key(|(clen, _)| *clen)
        .map(|(_, m)| m)
}
