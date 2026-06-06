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

use torajs_codegen::compile_function;
use torajs_core::ssa::{FuncId, Module, Type};
use torajs_core::{TORAJS_STATICLIBS, ast, check, lexer, modules, parser, ssa_lower};
use torajs_link::archive_emit::link_to_exec_with_archives;
use torajs_link::exec::{
    LinkConfig, UserDataGlobalEntry, UserStringEntry, UserStringKind, UserVtableEntry,
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

fn lower_to_ssa(input: &str) -> Result<Module, ExitCode> {
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

fn build_link_config(ssa_module: &Module) -> LinkConfig {
    // Compile each SSA function to aarch64 bytes + per-fn reloc table.
    // The synthesized top-level wrapper ssa_lower emits as "main" needs
    // to surface as the Apple Silicon `_main` entry — rename after
    // compile so `LinkConfig.entry = "_main"` resolves.
    let mut funcs: Vec<_> = ssa_module
        .funcs
        .iter()
        .map(compile_function)
        .collect::<Vec<_>>();
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
