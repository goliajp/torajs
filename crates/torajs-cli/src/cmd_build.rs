//! `tr build` — AOT pipeline. Routes the source through
//! `torajs-codegen` (aarch64 emit) + `torajs-obj` (relocs) +
//! `torajs-link` (Mach-O exec with `link_to_exec_with_archives`).
//! All user-code data sections (`strings` / `data_globals` /
//! relative `vtable_globals` / `__DATA_CONST` rebase chain) are wired into
//! the link layer; this module materializes `ssa::Module` →
//! `LinkConfig` and shells out to the staticlib-aware emit path.

use std::process::ExitCode;

use crate::cmd_build_extern_relocs::rewrite_extern_relocs;
use crate::cmd_build_ssa_string_registries::{
    build_class_layout_entries, build_class_names, build_data_globals, build_fn_name_globals,
    build_user_strings, build_vtable_globals,
};
use crate::cmd_build_synthesize::{
    ENTRY_SYM, USER_MAIN_SYM, synthesize_main_argv_wrapper, synthesize_obj_alloc,
    synthesize_obj_drop_sized,
};
use crate::runtime_image::TORAJS_RUNTIME_IMAGE;
use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_codegen::reloc::{CallTarget, RelocKind};
use torajs_codegen::{ModuleFacts, compile_function_at};
use torajs_core::ssa::{FuncId, Module};
use torajs_link::archive_emit::link_to_exec_for_exit;
use torajs_link::exec::LinkConfig;
use torajs_link::phase_timer::PhaseTimer;
use torajs_link::resolve::SymTable;

use crate::cmd_build_lower::lower_to_ssa;

pub(crate) fn run(args: &[String]) -> ExitCode {
    if matches!(
        args.first().map(String::as_str),
        Some("--help") | Some("-h")
    ) {
        println!("tr build — AOT via codegen + obj + link");
        println!();
        println!("USAGE: tr build <input.ts> -o <output> [--no-strip]");
        println!();
        println!("  --no-strip   keep the runtime's symbols in the artifact's symbol table");
        println!("               (default strips them; user functions always stay)");
        return ExitCode::SUCCESS;
    }

    let mut input: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut no_strip = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-strip" => {
                no_strip = true;
                i += 1;
            }
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

    let mut t = PhaseTimer::start(0);
    let ssa_module = match lower_to_ssa(input) {
        Ok(m) => m,
        Err(code) => return code,
    };
    t.tick("lower_to_ssa");

    // Phase 0 step 8b — egraph mid-end pass. Honors TORAJS_EGRAPH_OFF=1.
    let ssa_module = torajs_egraph::transform_module(ssa_module);
    t.tick("egraph");

    let cfg = build_link_config(&ssa_module, true, !no_strip);
    t.tick("build_link_config");

    let bytes = match link_to_exec_for_exit(&cfg) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("link error: {e:?}");
            return ExitCode::from(1);
        }
    };
    t.tick("link_to_exec_with_archives");

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

    std::mem::forget((ssa_module, cfg, bytes)); // the process exits next
    ExitCode::SUCCESS
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
    // Indexed by FuncId, like the `funcs` vec below.
    let facts = ModuleFacts::of(ssa_module);
    let mut funcs: Vec<_> = ssa_module
        .funcs
        .iter()
        .enumerate()
        .map(|(i, f)| {
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
                compile_function_at(f, FuncId(i as u32), &facts)
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
    // r498 — argv-init on demand: the captured argc/argv/envp
    // globals are only readable through the `__torajs_process_*`
    // intrinsic family (`process` / `Bun` are compiler-resolved
    // namespaces; there is no runtime object that could reach the
    // registry dynamically), so a program whose compiled fns carry
    // no such reloc can never observe whether init ran. Skipping
    // the call lets dead-strip drop the whole torajs-process member.
    let needs_argv = funcs.iter().any(|f| {
        f.relocs.iter().any(|r| match &r.kind {
            RelocKind::CallSite {
                target: CallTarget::Extern(name),
            } => name.trim_start_matches('_').starts_with("torajs_process_"),
            _ => false,
        })
    });
    funcs.push(synthesize_main_argv_wrapper(needs_argv));
    funcs
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

pub(crate) fn build_link_config(
    ssa_module: &Module,
    dead_strip: bool,
    strip_member_symbols: bool,
) -> LinkConfig {
    let mut t = PhaseTimer::start(1);
    let mut funcs = compile_module_funcs(ssa_module);
    t.tick("compile_module_funcs");
    // S2-5 blade 2b: stub the arms of the families the program never enters (dispatch judge);
    // TORAJS_DISPATCH_STUB_ALL / _OFF / _NS, TORAJS_LINK_STUB_FORCE=<sym,…> force (diagnostics).
    let class_layouts = build_class_layout_entries(ssa_module, &mut funcs);
    let vtable_globals = build_vtable_globals(ssa_module);
    let mut judged_stubs = Vec::new();
    if crate::cmd_build_dispatch_stubs::stub_all_enabled() {
        crate::cmd_build_dispatch_stubs::append_dispatch_stubs(&mut funcs, u16::MAX, true, true);
        judged_stubs = crate::cmd_build_dispatch_stubs::regexp_world_stubs(true, false, true, true);
    } else if !crate::cmd_build_dispatch_stubs::stub_off() {
        let live =
            torajs_link::user_gc::live_user_fns(&funcs, ENTRY_SYM, &vtable_globals, &class_layouts);
        let j = crate::cmd_build_dispatch_judge::judge(ssa_module, &live);
        crate::cmd_build_dispatch_stubs::append_dispatch_stubs(
            &mut funcs,
            j.stub_arm_bits,
            j.stub_ns_static || crate::cmd_build_dispatch_stubs::stub_ns_forced(),
            j.stub_printers,
        );
        judged_stubs = crate::cmd_build_dispatch_stubs::regexp_world_stubs(
            j.stub_regexp_protocol,
            j.regexp_protocol_answers_zero,
            j.stub_regexp_algorithms,
            j.stub_ctor_regexp,
        );
        crate::cmd_build_dispatch_stubs::append_absent_settle_stub(&mut funcs, ssa_module);
    }

    t.tick("dispatch_stubs");
    let mut strings = build_user_strings(ssa_module);
    let class_names = build_class_names(ssa_module, &mut strings);
    let data_globals = build_data_globals(ssa_module);
    let fn_name_globals = build_fn_name_globals(ssa_module);

    let mut cfg = LinkConfig {
        funcs,
        entry: ENTRY_SYM.to_string(),
        sym_table: SymTable::new(),
        codesign_ident: "tora".into(),
        dead_strip,
        strip_member_symbols,
        elidable_sites: Vec::new(),
        guarded_stubs: {
            let mut g = crate::cmd_build_elide::guarded_stubs();
            g.extend(judged_stubs);
            g
        },
        // borrowed straight off the baked static; a copy would be a
        // per-build memcpy of the whole runtime
        image: std::borrow::Cow::Borrowed(TORAJS_RUNTIME_IMAGE),
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
        // `__torajs_fn_<fid>` alias (resolved to the fn's vaddr at
        // link time) with its name's
        // `__torajs_str_dyn_<sid>` alias — the RawBytes flavour
        // points at the raw char payload without the 16-byte Str
        // header so the runtime helper can putc each byte directly.
        // `apply_user_string_overrides` registers both flavours
        // downstream. Empty when no top-level fn declarations landed
        // an entry (every decl filtered as mangled / closure-lifted).
        fn_name_globals,
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
    };
    // r498 knife 4 — user-fn dead-strip; roots come from the
    // materialized tables in `cfg` itself (module doc in
    // torajs_link::user_gc).
    t.tick("tables");
    torajs_link::user_gc::strip_dead_user_fns(&mut cfg);
    t.tick("user_gc");
    // r499/r501 — the drains and the adapter mints become conditional
    // on their guards' liveness (policy in cmd_build_elide). Collected
    // AFTER the strip: a mint in a fn the artifact will not carry has
    // no reloc left to patch.
    cfg.elidable_sites = crate::cmd_build_elide::collect_elidable_sites(&cfg.funcs);
    // r502 — and the class register calls, when no class cell leaves
    // its prologue (policy in cmd_build_elide_class).
    cfg.elidable_sites
        .extend(crate::cmd_build_elide_class::class_register_sites(
            &cfg.funcs, ssa_module,
        ));
    t.tick("elidable_sites");
    cfg
}
