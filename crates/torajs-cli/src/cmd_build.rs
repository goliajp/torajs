//! `tr build` — AOT pipeline. Routes the source through
//! `torajs-codegen` (aarch64 emit) + `torajs-obj` (relocs) +
//! `torajs-link` (Mach-O exec with `link_to_exec_with_archives`).
//! All seven user-code data sections (`strings` / `data_globals` /
//! `vtable_globals` / `__DATA_CONST` rebase chain) are wired into
//! the link layer; this module materializes `ssa::Module` →
//! `LinkConfig` and shells out to the staticlib-aware emit path.

use std::process::ExitCode;

use crate::ast_pipeline;
use crate::cmd_build_extern_relocs::rewrite_extern_relocs;
use crate::cmd_build_ssa_string_registries::{
    build_class_names, build_fn_name_globals, build_user_strings,
};
use crate::cmd_build_synthesize::{
    ENTRY_SYM, USER_MAIN_SYM, synthesize_main_argv_wrapper, synthesize_obj_alloc,
    synthesize_obj_drop_sized,
};
use torajs_codegen::CompiledFunction;
use torajs_codegen::compile_function_with_sigs;
use torajs_codegen::frame::FrameLayout;
use torajs_core::ssa::{FuncId, Module, Type};
use torajs_core::{TORAJS_STATICLIBS, check, lexer, modules, parser, ssa_lower};
use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::{LinkConfig, UserClassLayoutEntry, UserDataGlobalEntry, UserVtableEntry};
use torajs_link::resolve::SymTable;

use crate::util::{base_dir_for, read_source, sloppy_goal_for};

pub(crate) fn run(args: &[String]) -> ExitCode {
    if matches!(
        args.first().map(String::as_str),
        Some("--help") | Some("-h")
    ) {
        println!("tr build — AOT via codegen + obj + link");
        println!();
        println!("USAGE: tr build <input.ts> -o <output>");
        return ExitCode::SUCCESS;
    }

    let mut input: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                let Some(path) = args.get(i) else {
                    eprintln!("error: `-o` requires a path");
                    return ExitCode::from(2);
                };
                output = Some(path.as_str());
                i += 1;
            }
            other if !other.starts_with('-') && input.is_none() => {
                input = Some(other);
                i += 1;
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }

    let Some(input) = input else {
        eprintln!("error: missing input file");
        return ExitCode::from(2);
    };
    let Some(output) = output else {
        eprintln!("error: missing `-o <output>`");
        return ExitCode::from(2);
    };

    let ssa_module = match lower_to_ssa(input) {
        Ok(m) => m,
        Err(code) => return code,
    };

    // Phase 0 step 8b — egraph mid-end pass. Honors TORAJS_EGRAPH_OFF=1.
    let ssa_module = torajs_egraph::transform_module(ssa_module);

    let cfg = build_link_config(&ssa_module, true);

    let bytes = match link_to_exec_with_archives(&cfg) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("link error: {e:?}");
            return ExitCode::from(1);
        }
    };

    if let Err(e) = std::fs::write(output, &bytes) {
        eprintln!("write error: {e}");
        return ExitCode::from(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = match std::fs::metadata(output) {
            Ok(m) => m.permissions(),
            Err(e) => {
                eprintln!("stat error: {e}");
                return ExitCode::from(1);
            }
        };
        perms.set_mode(0o755);
        if let Err(e) = std::fs::set_permissions(output, perms) {
            eprintln!("chmod error: {e}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}

pub(crate) fn lower_to_ssa(input: &str) -> Result<Module, ExitCode> {
    let src = read_source(input).map_err(|e| {
        eprintln!("error: {e}");
        ExitCode::from(2)
    })?;
    let tokens = lexer::tokenize(&src).map_err(|e| {
        eprintln!("lex error: {e}");
        ExitCode::from(1)
    })?;
    let mut ast = parser::parse(&src, &tokens).map_err(|e| {
        eprintln!("parse error: {e}");
        ExitCode::from(1)
    })?;
    ast.source = src.to_string();
    ast.warm_newline_cache();
    // RFC 20260810-sloppy-goal-arguments S1 — goal bit from the input
    // extension (bun mapping: `.cts` = CommonJS sloppy).
    ast.sloppy_script_goal = sloppy_goal_for(input);
    // Goal-triage gates that must beat the resolver's diagnostics —
    // see ast_pipeline.rs.
    ast_pipeline::run_pre_resolve_gates(&mut ast).map_err(|()| ExitCode::from(1))?;
    let base_dir = base_dir_for(input);
    modules::resolve_imports(&mut ast, &base_dir).map_err(|e| {
        eprintln!("import error: {e}");
        ExitCode::from(1)
    })?;

    // Raw-AST early-error gates + the pre-chain passes — see
    // ast_pipeline.rs.
    ast_pipeline::run_ast_prelude(&mut ast).map_err(|()| ExitCode::from(1))?;
    // Shared 31-pass desugar chain — see ast_pipeline.rs for the
    // per-pass ordering notes.
    ast_pipeline::run_ast_desugar_pipeline(&mut ast).map_err(|()| ExitCode::from(1))?;

    let (artifacts, warnings) = check::check_with_arity_warn(&ast).map_err(|e| {
        eprintln!("type error: {e}");
        ExitCode::from(1)
    })?;
    // RFC 20260730-undeclared-ident — non-fatal diagnostics: printed,
    // program still compiles and runs (spec semantics take over at
    // runtime). The `warning:` prefix stays outside the test262
    // verdict classifier's compile-reject set.
    for w in &warnings {
        eprintln!("warning: {w}");
    }

    // ssa_lower panics on unsupported AST shapes; surface them as the
    // exit-3 "not yet supported" contract the bench harness keys on.
    // The workspace release profile is `panic = "abort"`, so the
    // catch_unwind below never sees the payload there — the hook is
    // the only place the message can surface, and exiting from it
    // (instead of falling through to abort) preserves both the
    // message and the exit code. Pre-fix the hook was empty: every
    // lower reject was a silent SIGABRT with zero output, and the
    // harness misclassified skips as real failures.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "ssa_lower panicked".to_string()
        };
        eprintln!("not yet supported: {msg}");
        std::process::exit(3);
    }));
    let lower_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ssa_lower::lower_with_arity(&ast, &artifacts)
    }));
    std::panic::set_hook(prev_hook);
    // Unreachable under panic=abort (the hook exits first) and a pure
    // backstop under panic=unwind — the hook already printed.
    lower_result.map_err(|_| ExitCode::from(3))
}

/// Compile each SSA function to aarch64 bytes + per-fn reloc table.
///
/// The synthesized top-level wrapper ssa_lower emits as "main" needs
/// to surface as the Apple Silicon `_main` entry — renamed after
/// compile (to `_main_user`; the `_main` entry sym goes to the
/// argv-init wrapper synthesized at the tail so `process.argv` /
/// `Bun.argv` see the kernel-supplied argc/argv before the user body
/// runs).
///
/// SSA's `funcs` includes ~370 extern declarations (runtime
/// intrinsics with `is_declaration() == true`). Those have no body
/// and `compile_function` would emit a stub prologue at a zero-byte
/// vaddr, collapsing every extern call onto the same address. The
/// orthodox fix is to leave them in the slot space (so caller FuncIds
/// stay stable) but emit empty bytes AND rewrite any reloc targeting
/// them from `CallTarget::Func(fid)` to `CallTarget::Extern("_name")`
/// so the link layer resolves through the archive symbol table
/// (`___torajs_*` with Apple's `_` prefix). Mirrors how LLVM/clang
/// distinguishes external declarations from internal definitions.
fn compile_module_funcs(ssa_module: &Module) -> Vec<CompiledFunction> {
    // Per-FuncId param-type table. Indexes line up with the
    // `funcs` vec below (and with the FuncId space ssa_lower hands
    // out), so emit_call can read `fn_sigs[target_func.0]` to
    // discover the declared param types and coerce f64↔i64 args at
    // the call boundary. Mirrors `ssa_inkwell`'s
    // `callee.get_type().get_param_types()` lookup on the OLD
    // pipeline. Declarations (extern intrinsics) carry their sig
    // through `params + values[vid].ty` the same way as real fns.
    let fn_sigs: Vec<Vec<torajs_core::ssa::Type>> = ssa_module
        .funcs
        .iter()
        .map(|f| {
            f.params
                .iter()
                .map(|vid| f.values[vid.0 as usize].ty)
                .collect()
        })
        .collect();
    let mut funcs: Vec<_> = ssa_module
        .funcs
        .iter()
        .map(|f| {
            if f.is_declaration() {
                // Reserve a fn_vaddrs slot with empty bytes so call-site
                // reloc rewrites below preserve FuncId indexing.
                CompiledFunction {
                    name: f.name.clone(),
                    bytes: Vec::new(),
                    relocs: Vec::new(),
                    frame: FrameLayout::leaf_no_spill(),
                }
            } else {
                compile_function_with_sigs(f, &fn_sigs)
            }
        })
        .collect::<Vec<_>>();
    rewrite_extern_relocs(&mut funcs, &ssa_module.funcs);
    funcs.push(synthesize_obj_drop_sized());
    funcs.push(synthesize_obj_alloc());
    for cf in funcs.iter_mut() {
        if cf.name == "main" {
            cf.name = USER_MAIN_SYM.to_string();
        }
    }
    funcs.push(synthesize_main_argv_wrapper());
    funcs
}

/// One `UserDataGlobalEntry` per SSA data global (sym + slot size /
/// alignment from the SSA type).
fn build_data_globals(ssa_module: &Module) -> Vec<UserDataGlobalEntry> {
    ssa_module
        .data_globals
        .iter()
        .map(|dg| {
            let (size, align_log2) = type_slot_size_align(dg.ty);
            UserDataGlobalEntry {
                sym: dg.name.clone(),
                size,
                align_log2,
            }
        })
        .collect()
}

/// SD-4c-prereq+e8 — materialize ssa::Module.class_layouts (T-26.C
/// cycle collector metadata) into the proper in-house rodata path
/// (`__torajs_class_layouts` outer table + per-class inner
/// `.__class_offsets_<i>` globals + `__torajs_n_class_layouts`
/// count). Pre-e8 reserved two zerofill slots in `data_globals`
/// (now removed): the outer-ptr was NULL so the cycle collector
/// short-circuited on class-bearing programs. e8 lands real bytes
/// + dyld rebase via the e7b chained-fixups TextRebaseScope.
fn build_class_layout_entries(ssa_module: &Module) -> Vec<UserClassLayoutEntry> {
    ssa_module
        .class_layouts
        .iter()
        .map(|cl| UserClassLayoutEntry {
            child_offsets: cl.child_offsets.clone(),
            is_named: cl.is_named,
            is_generic: cl.is_generic,
            // W-J A3b — plumb FieldMetaSpec through to the link layer so
            // it can emit the per-class `.__class_fields_<i>` inner
            // global + per-field name strings + wire the outer entry's
            // field_metadata_ptr slot to the inner global's vaddr.
            fields: cl
                .field_metadata
                .iter()
                .map(|fm| torajs_link::exec::UserFieldMetaEntry {
                    name: fm.name.clone(),
                    offset: fm.offset,
                    type_tag: fm.type_tag,
                })
                .collect(),
            // 刀 4 (RFC 20260714-t262-top-clusters) — runtime class-
            // method dispatch rows; adapter fids resolve through
            // fn_vaddrs at rebase-assembly time.
            methods: cl
                .methods
                .iter()
                .map(|mm| torajs_link::exec::UserMethodMetaEntry {
                    name: mm.name.clone(),
                    adapter_fn_id: mm.adapter_fid.0,
                    // Bit 0 = this-free (S2.38); bit 1 = twin-primary
                    // (404-01 — the adapter is recv-first-shaped).
                    flags: u32::from(mm.this_free) | (u32::from(mm.twin_primary) << 1),
                    twin_fn_id: mm.twin_adapter_fid.map(|f| f.0),
                })
                .collect(),
        })
        .collect()
}

/// vtable slots resolve via `register_fn_addr_syms`'s
/// `__torajs_fn_<i>` override (codegen's `FnAddr` convention) — see
/// `archive_emit::link_to_exec_with_archives` and the
/// `probe_vtable_link` reference.
fn build_vtable_globals(ssa_module: &Module) -> Vec<UserVtableEntry> {
    ssa_module
        .vtable_globals
        .iter()
        .map(|vt| UserVtableEntry {
            sym: format!("__vtable_{}", vt.class_name),
            slot_syms: vt
                .fn_ids
                .iter()
                .map(|opt| opt.map(|fid: FuncId| format!("__torajs_fn_{}", fid.0)))
                .collect(),
        })
        .collect()
}

/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-5a — per-literal baked
/// DFA entries. ssa_lower's Phase C-6 `Expr::Regex` arm pushes
/// entries into `ssa_module.baked_regex_entries` when the literal is
/// DFA-eligible; forwarded to the link layer's `UserBakedRegexEntry`
/// schema (empty Vec = link layer skips emit).
fn build_baked_regex_entries(ssa_module: &Module) -> Vec<torajs_link::exec::UserBakedRegexEntry> {
    ssa_module
        .baked_regex_entries
        .iter()
        .map(|e| torajs_link::exec::UserBakedRegexEntry {
            index: e.index,
            states_payload: e.states_payload.clone(),
            states_len: e.states_len,
            start: e.start,
            start_mid: e.start_mid,
            start_mid_word: e.start_mid_word,
            start_mid_nonword: e.start_mid_nonword,
            // Round 3 Phase B attack #R-E — propagate host-baked
            // flag through the SSA → link bridge.
            any_accept_before_byte: e.any_accept_before_byte,
        })
        .collect()
}

pub(crate) fn build_link_config(ssa_module: &Module, dead_strip: bool) -> LinkConfig {
    let funcs = compile_module_funcs(ssa_module);

    let mut strings = build_user_strings(ssa_module);
    let class_names = build_class_names(ssa_module, &mut strings);
    let data_globals = build_data_globals(ssa_module);
    let class_layouts = build_class_layout_entries(ssa_module);
    let vtable_globals = build_vtable_globals(ssa_module);

    // Borrowed straight off the baked `include_bytes` statics — the
    // per-case `to_vec()` deep copy here was ~36% of `tr run` compile
    // wall (~50MB memcpy; see rfcs/20260724 phase-timing.md A1).
    let archives: Vec<std::borrow::Cow<'static, [u8]>> = TORAJS_STATICLIBS
        .iter()
        .map(|(_, bytes)| std::borrow::Cow::Borrowed(*bytes))
        .collect();

    LinkConfig {
        funcs,
        entry: ENTRY_SYM.to_string(),
        sym_table: SymTable::new(),
        codesign_ident: "tora".into(),
        dead_strip,
        archives,
        strings,
        data_globals,
        vtable_globals,
        class_layouts,
        // tr build bundles libtorajs_cycle.a which references
        // ___torajs_n_class_layouts + ___torajs_class_layouts even on
        // class-free programs — force-emit a 4-byte count global (= 0)
        // and register both syms so the staticlib resolves.
        force_emit_class_layouts_globals: true,
        // Fn-name registry Phase 2 Step 5 — same rationale as
        // `force_emit_class_layouts_globals`. Every staticlib bundle
        // now pulls `libtorajs_fnname.a` transitively (inspect.rs's
        // Tag::Closure / Type::FnSig arms reference
        // `__torajs_fn_print_inline`), and that helper crate
        // unconditionally references the table extern statics —
        // force-emit a zero-count global so they always resolve.
        force_emit_fn_name_globals: true,
        // Fn-name registry Phase 2 Step 3b.4 — converted from
        // `ssa_module.fn_name_globals` (populated by Step 2 at
        // fn-decl lowering, with the name already interned into
        // `ssa_module.strings[entry.name_sid.0]` and the StringId
        // recorded alongside). Each link-layer entry pairs the fn's
        // `__torajs_fn_<fid>` alias (registered by
        // `register_fn_addr_syms`) with its name's
        // `__torajs_str_dyn_<sid>` alias — the RawBytes flavour
        // points at the raw char payload without the 16-byte Str
        // header so the runtime helper can putc each byte directly.
        // `apply_user_string_overrides` registers both flavours
        // downstream. Empty when no top-level fn declarations landed
        // an entry (every decl filtered as mangled / closure-lifted).
        fn_name_globals: build_fn_name_globals(ssa_module),
        // W-J Phase A3c — class-name registry built upstream from
        // `ssa_module.class_layouts`. Empty when no named class
        // entries land in the layout array (probes / anonymous-only
        // programs).
        class_names,
        // W-J Phase A3c — same rationale as
        // `force_emit_class_layouts_globals`:
        // `libtorajs_structmeta.a` references the table extern
        // statics unconditionally so `tr build` must emit the
        // zero-count global on class-name-free programs.
        force_emit_class_names_globals: true,
        baked_regex_entries: build_baked_regex_entries(ssa_module),
    }
}

/// SSA `Type` → `(slot size in bytes, log2 alignment)` for
/// `__DATA,__bss` placement. Heap-shaped reference types lower to a
/// single pointer at codegen, so they share the I64 8/3 slot. `Void`
/// is not allocable and panics at this layer (the SSA layer should
/// never declare a `let x: void`).
fn type_slot_size_align(ty: Type) -> (u32, u8) {
    match ty {
        Type::Void => panic!("DataGlobal of type Void is not allocable"),
        Type::I32 => (4, 2),
        Type::Bool => (1, 0),
        _ => (8, 3),
    }
}
