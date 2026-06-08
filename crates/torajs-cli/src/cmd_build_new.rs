//! `tr build` env-gated **new** pipeline — `TORAJS_NEW_PIPELINE=1`
//! routes the same source through `torajs-codegen` (aarch64 emit)
//! + `torajs-obj` (relocs) + `torajs-link` (Mach-O exec with
//! `link_to_exec_with_archives`) instead of `ssa_inkwell` /
//! LLVM. SD-4c-prereq+ wired all seven user-code data sections
//! (`strings` / `data_globals` / `vtable_globals` / `__DATA_CONST`
//! rebase chain) into the link layer; this is the cmd-side
//! dispatcher that materializes `ssa::Module` → `LinkConfig`
//! and shells out to the staticlib-aware emit path. Default off
//! until swap-N closes; legacy LLVM path stays the default.

use std::process::ExitCode;

use torajs_codegen::CompiledFunction;
use torajs_codegen::compile_function;
use torajs_codegen::enc::b_imm26;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
use torajs_core::ssa::{FuncId, Module, Type};
use torajs_core::{
    TORAJS_NEW_PIPELINE_EXTRA_STATICLIBS, TORAJS_STATICLIBS, ast, check, lexer, modules, parser,
    ssa_lower,
};
use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::{
    LinkConfig, UserClassLayoutEntry, UserDataGlobalEntry, UserStringEntry, UserStringKind,
    UserVtableEntry,
};
use torajs_link::resolve::SymTable;

use crate::util::{base_dir_for, read_source};

/// Mach-O `MH_EXECUTE` entry-point symbol. ld64 / dyld both look up
/// `_main` (with the Apple Silicon underscore prefix); ssa_lower
/// emits the synthesized top-level wrapper as `"main"` so we rename
/// after compile to land at the right name in `LinkConfig.entry`.
const ENTRY_SYM: &str = "_main";

pub(crate) fn run(args: &[String]) -> ExitCode {
    if matches!(
        args.first().map(String::as_str),
        Some("--help") | Some("-h")
    ) {
        println!("tr build (TORAJS_NEW_PIPELINE=1) — AOT via codegen + obj + link");
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

    let cfg = build_link_config(&ssa_module);

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
    let mut ast = parser::parse(&tokens).map_err(|e| {
        eprintln!("parse error: {e}");
        ExitCode::from(1)
    })?;
    ast.source = src.to_string();
    ast.warm_newline_cache();
    let base_dir = base_dir_for(input);
    modules::resolve_imports(&mut ast, &base_dir).map_err(|e| {
        eprintln!("import error: {e}");
        ExitCode::from(1)
    })?;

    ast::unwrap_exports(&mut ast);
    ast::rename_user_main(&mut ast);
    ast::desugar_generators(&mut ast);
    ast::desugar_async(&mut ast);
    ast::desugar_builtin_imports(&mut ast);
    ast::desugar_builtin_new(&mut ast);
    ast::desugar_prototype_call(&mut ast);
    ast::inject_builtin_classes(&mut ast);
    ast::desugar_classes(&mut ast);
    ast::synthesize_class_globals(&mut ast);
    ast::tag_struct_field_closure_types(&mut ast);
    ast::lift_arrow_fns(&mut ast);
    ast::infer_anonymous_closure_params(&mut ast);
    ast::synthesize_forwarders(&mut ast);
    ast::synthesize_fn_to_closure_forwarders(&mut ast);
    ast::desugar_function_prototype_methods(&mut ast);
    ast::desugar_uninit_let(&mut ast);
    ast::desugar_var_hoist(&mut ast);
    ast::desugar_nested_fns(&mut ast);
    ast::desugar_variadic_push(&mut ast);
    ast::desugar_array_isarray_value(&mut ast);
    ast::desugar_arguments_object(&mut ast);
    ast::rewrite_split_for_i_to_iter(&mut ast);
    ast::escape_analyze_array_literals(&mut ast);
    ast::desugar_implicit_generics(&mut ast);
    ast::apply_default_args(&mut ast);
    ast::apply_rest_args(&mut ast);
    ast::compute_consuming_params(&mut ast);

    let (generic_call_sites, expr_types, arity_pad_count) =
        check::check_with_arity(&ast).map_err(|e| {
            eprintln!("type error: {e}");
            ExitCode::from(1)
        })?;

    // ssa_lower panics on unsupported AST shapes; mirror cmd_build.rs's
    // panic-catch so the bench harness sees a clean exit-3 "skip" rather
    // than a backtrace.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let lower_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ssa_lower::lower_with_arity(&ast, &generic_call_sites, &expr_types, &arity_pad_count)
    }));
    std::panic::set_hook(prev_hook);
    lower_result.map_err(|payload| {
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "ssa_lower panicked".to_string()
        };
        eprintln!("not yet supported: {msg}");
        ExitCode::from(3)
    })
}

pub(crate) fn build_link_config(ssa_module: &Module) -> LinkConfig {
    // Compile each SSA function to aarch64 bytes + per-fn reloc table.
    // The synthesized top-level wrapper ssa_lower emits as "main" needs
    // to surface as the Apple Silicon `_main` entry — rename after
    // compile so `LinkConfig.entry = "_main"` resolves.
    //
    // SSA's `funcs` includes ~370 extern declarations (runtime
    // intrinsics with `is_declaration() == true`). Those have no body
    // and `compile_function` would emit a stub prologue at a zero-byte
    // vaddr, collapsing every extern call onto the same address. The
    // orthodox fix is to leave them in the slot space (so caller FuncIds
    // stay stable) but emit empty bytes AND rewrite any reloc targeting
    // them from `CallTarget::Func(fid)` to `CallTarget::Extern("_name")`
    // so the link layer resolves through the archive symbol table
    // (`___torajs_*` with Apple's `_` prefix). Mirrors how LLVM/clang
    // distinguishes external declarations from internal definitions.
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
                compile_function(f)
            }
        })
        .collect::<Vec<_>>();
    rewrite_extern_relocs(&mut funcs, &ssa_module.funcs);
    funcs.push(synthesize_obj_drop_sized());
    funcs.push(synthesize_obj_alloc());
    for cf in funcs.iter_mut() {
        if cf.name == "main" {
            cf.name = ENTRY_SYM.to_string();
        }
    }

    // ssa::Module.strings → both UserStringKind flavours per literal.
    // Codegen emits StaticStrRef (`__torajs_str_lit_<i>`) for `"x"`
    // expressions and StringRef (`__torajs_str_dyn_<i>`) for raw-byte
    // consumers (e.g. obj field-name matching). Emit both so either
    // resolves at link time without a per-call usage scan.
    let mut strings: Vec<UserStringEntry> = Vec::with_capacity(ssa_module.strings.len() * 2);
    for (i, lit) in ssa_module.strings.iter().enumerate() {
        strings.push(UserStringEntry {
            sym: format!("__torajs_str_lit_{i}"),
            bytes: lit.bytes.clone(),
            is_latin1: lit.is_latin1,
            length: lit.length,
            kind: UserStringKind::StaticStr,
        });
        strings.push(UserStringEntry {
            sym: format!("__torajs_str_dyn_{i}"),
            bytes: lit.bytes.clone(),
            is_latin1: lit.is_latin1,
            length: lit.length,
            kind: UserStringKind::RawBytes,
        });
    }

    let data_globals: Vec<UserDataGlobalEntry> = ssa_module
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
        .collect();
    // SD-4c-prereq+e8 — materialize ssa::Module.class_layouts (T-26.C
    // cycle collector metadata) into the proper in-house rodata path
    // (`__torajs_class_layouts` outer table + per-class inner
    // `.__class_offsets_<i>` globals + `__torajs_n_class_layouts`
    // count). Pre-e8 reserved two zerofill slots in `data_globals`
    // (now removed): the outer-ptr was NULL so the cycle collector
    // short-circuited on class-bearing programs. e8 lands real bytes
    // + dyld rebase via the e7b chained-fixups TextRebaseScope.
    let class_layouts: Vec<UserClassLayoutEntry> = ssa_module
        .class_layouts
        .iter()
        .map(|cl| UserClassLayoutEntry {
            child_offsets: cl.child_offsets.clone(),
        })
        .collect();

    // vtable slots resolve via `register_fn_addr_syms`'s `__torajs_fn_<i>`
    // override (codegen's `FnAddr` convention) — see
    // `archive_emit::link_to_exec_with_archives` and the
    // `probe_vtable_link` reference.
    let vtable_globals: Vec<UserVtableEntry> = ssa_module
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
        .collect();

    let archives: Vec<Vec<u8>> = TORAJS_STATICLIBS
        .iter()
        .chain(TORAJS_NEW_PIPELINE_EXTRA_STATICLIBS.iter())
        .map(|(_, bytes)| bytes.to_vec())
        .collect();

    LinkConfig {
        funcs,
        entry: ENTRY_SYM.to_string(),
        sym_table: SymTable::new(),
        codesign_ident: "tora".into(),
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
    }
}

/// SD-4c-prereq swap-2d — `__torajs_obj_drop_sized(user_ptr, size) -> void`.
/// ssa_inkwell's `obj_builders::define_obj_drop_sized` inlines a TLAB
/// fast path mirroring `define_obj_alloc`'s TLAB pop. The new pipeline
/// has no LLVM-IR emit backend; emit the intrinsic directly as a
/// hand-rolled CompiledFunction that tail-calls `___torajs_libc_free`
/// (the slow path). Loses the TLAB hot-loop optimization until a real
/// port lands (swap-3+ backlog: TLAB.push fast path in native ARM64);
/// gains correct drop semantics — every block makes it back to the
/// allocator instead of leaking.
///
/// ARM64 body:
///   `B ___torajs_libc_free`   ; tail call — x0 = user_ptr is already in
///                              ; the right register, x1 = size is
///                              ; discarded by libc_free's signature
///                              ; (which is `void(ptr)`).
fn synthesize_obj_drop_sized() -> CompiledFunction {
    let mut bytes = Vec::with_capacity(4);
    let reloc_offset = bytes.len() as u32;
    bytes.extend_from_slice(&b_imm26(0).to_le_bytes());
    CompiledFunction {
        name: "___torajs_obj_drop_sized".into(),
        bytes,
        relocs: vec![Reloc {
            byte_offset: reloc_offset,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("___torajs_libc_free".into()),
            },
        }],
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// SD-4c-prereq swap-2h — `__torajs_obj_alloc(size) -> ptr`.
/// ssa_inkwell's `obj_builders::define_obj_alloc` inlines a TLAB
/// fast path (size-class bucket → TLAB.pop → return slot+16) with a
/// fallback to `___torajs_libc_malloc(size)`. The new pipeline has no
/// LLVM-IR emit backend; emit the intrinsic directly as a hand-rolled
/// CompiledFunction that tail-calls the fallback. Loses the TLAB
/// hot-loop optimization until a native ARM64 TLAB.pop port lands
/// (swap-3+ backlog, paired with `synthesize_obj_drop_sized`'s
/// TLAB.push); gains correct alloc semantics — `___torajs_libc_malloc`
/// already produces the 16-byte-header layout `obj_alloc` returns, so
/// the tail call is byte-for-byte the inline fallback path.
///
/// ARM64 body:
///   `B ___torajs_libc_malloc`  ; tail call — x0 = size is already in
///                              ; the right register, return ptr in x0
///                              ; flows straight back to the caller.
fn synthesize_obj_alloc() -> CompiledFunction {
    let mut bytes = Vec::with_capacity(4);
    let reloc_offset = bytes.len() as u32;
    bytes.extend_from_slice(&b_imm26(0).to_le_bytes());
    CompiledFunction {
        name: "___torajs_obj_alloc".into(),
        bytes,
        relocs: vec![Reloc {
            byte_offset: reloc_offset,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("___torajs_libc_malloc".into()),
            },
        }],
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// Rewrite `CallSite{Func(fid)}` relocs that target an extern declaration
/// into `CallSite{Extern("_<name>")}` so the link layer resolves them
/// through the archive symbol table (`___torajs_*` Apple form) instead
/// of a stale fn_vaddrs slot. Page21 / PageOff12 / AbsPtr64 with
/// `target_sym = "__torajs_fn_<fid>"` pointing at an extern get the same
/// treatment — FnAddr of an extern becomes an external sym ref.
fn rewrite_extern_relocs(
    compiled: &mut [torajs_codegen::CompiledFunction],
    ssa_funcs: &[torajs_core::ssa::Function],
) {
    let is_extern: Vec<bool> = ssa_funcs.iter().map(|f| f.is_declaration()).collect();
    for cf in compiled.iter_mut() {
        for r in cf.relocs.iter_mut() {
            match &mut r.kind {
                RelocKind::CallSite {
                    target: CallTarget::Func(fid),
                } if is_extern[fid.0 as usize] => {
                    let name = ssa_funcs[fid.0 as usize].name.clone();
                    r.kind = RelocKind::CallSite {
                        target: CallTarget::Extern(format!("_{name}")),
                    };
                }
                RelocKind::Page21 { target_sym }
                | RelocKind::PageOff12 { target_sym }
                | RelocKind::AbsPtr64 { target_sym } => {
                    if let Some(fid) = parse_fn_addr_sym(target_sym)
                        && fid < is_extern.len()
                        && is_extern[fid]
                    {
                        *target_sym = format!("_{}", ssa_funcs[fid].name);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse `"__torajs_fn_<n>"` → `<n>` (the original FuncId index), or
/// `None` for any other sym name. Matches the codegen FnAddr convention
/// in `crates/torajs-codegen/src/compile/refs.rs:106`.
fn parse_fn_addr_sym(sym: &str) -> Option<usize> {
    sym.strip_prefix("__torajs_fn_")
        .and_then(|tail| tail.parse().ok())
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
