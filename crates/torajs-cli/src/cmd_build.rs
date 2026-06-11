//! `tr build` — AOT pipeline. Routes the source through
//! `torajs-codegen` (aarch64 emit) + `torajs-obj` (relocs) +
//! `torajs-link` (Mach-O exec with `link_to_exec_with_archives`).
//! All seven user-code data sections (`strings` / `data_globals` /
//! `vtable_globals` / `__DATA_CONST` rebase chain) are wired into
//! the link layer; this module materializes `ssa::Module` →
//! `LinkConfig` and shells out to the staticlib-aware emit path.

use std::process::ExitCode;

use torajs_codegen::CompiledFunction;
use torajs_codegen::compile_function_with_sigs;
use torajs_codegen::enc::{add_imm, b_imm26, bl_imm26, ret as enc_ret, str_x_imm12};
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reg::Gpr;
use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
use torajs_core::ssa::{FuncId, Module, Type};
use torajs_core::{TORAJS_STATICLIBS, ast, check, lexer, modules, parser, ssa_lower};
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

/// Renamed user-main sym — the `__torajs_main_entry` wrapper below
/// occupies the real `_main` symbol so it can run argv-init before
/// jumping to the user main body. Mirrors the LLVM-era
/// `ssa_inkwell::lower.rs` inserting a `__torajs_argv_init(argc,
/// argv, envp)` call at the top of `main`'s entry block.
const USER_MAIN_SYM: &str = "_main_user";

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
    // Rename the SSA-synthesized top-level wrapper from "main" to the
    // internal `_main_user` symbol; the `_main` entry sym goes to the
    // argv-init wrapper synthesized below so `process.argv` /
    // `Bun.argv` see the kernel-supplied argc/argv before the user
    // body runs.
    for cf in funcs.iter_mut() {
        if cf.name == "main" {
            cf.name = USER_MAIN_SYM.to_string();
        }
    }
    funcs.push(synthesize_main_argv_wrapper());

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

/// SD-4c gap2 — wrapper that occupies `_main` and forwards the
/// kernel-supplied `(argc=x0, argv=x1, envp=x2)` triple to
/// `__torajs_argv_init` before invoking the user main body. Mirrors
/// `ssa_inkwell::lower.rs`'s entry-block call insertion, but at the
/// raw ARM64 level since the NEW pipeline has no LLVM IR backend.
///
/// The wrapper preserves x0..x2 across the init call by storing
/// them on stack and restoring before the user-main BL. Strictly
/// speaking only argv (x1) + envp (x2) need preservation post-init
/// since user main takes no args; we save all three for symmetry
/// + future-proofing. AAPCS64 §6.4 requires 16-byte sp alignment,
/// so the frame is 48 bytes (3 × 16 = 48, holding x29/x30 + the
/// saved triple).
///
/// ARM64 body (12 inst × 4 = 48 bytes):
///   stp  x29, x30, [sp, #-48]!   ; save fp/lr + alloca 48 bytes
///   mov  x29, sp
///   stp  x0,  x1,  [sp, #16]     ; preserve argc + argv (unused
///                                 ; post-init, kept for clarity)
///   str  x2,        [sp, #32]    ; preserve envp (unused post-init)
///   bl   ___torajs_argv_init     ; argc/argv/envp already in x0..2
///   bl   __main_user             ; user main(), returns i32 in x0
///   ldp  x29, x30, [sp], #48     ; restore fp/lr + free frame
///   ret
fn synthesize_main_argv_wrapper() -> CompiledFunction {
    let mut bytes = Vec::with_capacity(48);
    // stp x29, x30, [sp, #-48]!  — pre-index frame allocation
    //   encoding: 0xA9BD7BFD = stp x29, x30, [sp, #-48]!
    //   immediate field = (-48 / 8) as 7-bit signed = -6 = 0x7A
    //   full word: 1010 1001 1011 1010 0111 1011 1111 1101 = 0xA9BA7BFD
    // Easier to hand-pick the matching pre-index pattern. Use enc::
    // stp_pre_index when available; otherwise inline the constant.
    bytes.extend_from_slice(
        &torajs_codegen::enc::stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -48).to_le_bytes(),
    );
    // mov x29, sp  =  add x29, sp, #0
    bytes.extend_from_slice(&add_imm(Gpr::X29, Gpr::SP, 0).to_le_bytes());
    // str x0, [sp, #16]  / str x1, [sp, #24]
    bytes.extend_from_slice(&str_x_imm12(Gpr::X0, Gpr::SP, 16).to_le_bytes());
    bytes.extend_from_slice(&str_x_imm12(Gpr::X1, Gpr::SP, 24).to_le_bytes());
    // str x2, [sp, #32]
    bytes.extend_from_slice(&str_x_imm12(Gpr::X2, Gpr::SP, 32).to_le_bytes());
    // bl __torajs_argv_init (args already in x0/x1/x2 from dyld)
    let argv_init_bl_off = bytes.len() as u32;
    bytes.extend_from_slice(&bl_imm26(0).to_le_bytes());
    // bl _main_user (no args; returns i32 in x0)
    let user_main_bl_off = bytes.len() as u32;
    bytes.extend_from_slice(&bl_imm26(0).to_le_bytes());
    // ldp x29, x30, [sp], #48  — post-index frame release
    bytes.extend_from_slice(
        &torajs_codegen::enc::ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 48).to_le_bytes(),
    );
    // ret x30
    bytes.extend_from_slice(&enc_ret(Gpr::X30).to_le_bytes());
    CompiledFunction {
        name: ENTRY_SYM.to_string(),
        bytes,
        relocs: vec![
            Reloc {
                byte_offset: argv_init_bl_off,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern("___torajs_argv_init".into()),
                },
            },
            Reloc {
                byte_offset: user_main_bl_off,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern(USER_MAIN_SYM.into()),
                },
            },
        ],
        // The wrapper saves its own fp/lr inline; the frame layout is
        // a hand-built stp/ldp pair, not the standard leaf prologue.
        // Mark as `leaf_no_spill` so codegen's epilogue emit doesn't
        // try to wrap this fn — it's already complete.
        frame: FrameLayout::leaf_no_spill(),
    }
}

/// SD-4c-prereq swap-2d — `__torajs_obj_drop_sized(user_ptr, size) -> void`.
/// The LLVM-era `obj_builders::define_obj_drop_sized` inlined a TLAB
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
/// The LLVM-era `obj_builders::define_obj_alloc` inlined a TLAB
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
