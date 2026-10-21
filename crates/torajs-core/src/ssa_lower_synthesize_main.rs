//! `declare_intrinsic` + `synthesize_main` extracted from `ssa_lower.rs`
//! chunk 367.
//!
//! - `declare_intrinsic` — helper used at intrinsic-table registration
//!   sites (and by `ssa_lower_substr_trim_into::declare_all`).
//! - `synthesize_main` — top-level driver that constructs a `LowerCtx`
//!   over the program's top-level stmts, primes escape-capture / deque /
//!   escape-obj sets, walks each top-level stmt, and drains microtasks +
//!   cycle-collector before returning. A module that awaits at its top
//!   level is lowered as a body function instead, and `main` is a
//!   wrapper that calls it and then drains and exits the same way
//!   (`ssa_lower_fn::async_main`).

use std::collections::HashMap;

use crate::ast::Stmt;
use crate::ssa::{self, FuncId, InstKind, Module, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_fn::async_main::{BODY_NAME, ModuleCalls, emit_body_call};
use main_ctx::{MainEnv, MainTarget};

pub(crate) mod main_ctx;

pub(crate) fn declare_intrinsic(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    name: &str,
    param_tys: &[Type],
    ret_ty: Type,
) -> FuncId {
    let mut f = ssa::Function::new(name, ret_ty);
    for (i, &t) in param_tys.iter().enumerate() {
        f.add_param(t, &format!("a{i}"));
    }
    // No blocks → declaration only; backend supplies the body.
    let id = FuncId(module.funcs.len() as u32);
    fn_table.insert(name.to_string(), id);
    module.funcs.push(f);
    id
}

/// The top level as `main`, or, given the id the module body will
/// have, as that body plus the wrapper `main` around it. Answers the
/// function lowered from the statements, the wrapper, and the strings
/// both interned (numbered from `string_id_base`). `frame_result_reprs`
/// is `ModuleScans::frame_result_reprs`.
pub(crate) fn synthesize_main(
    stmts: &[&Stmt],
    mut env: MainEnv<'_>,
    string_id_base: usize,
    body: Option<FuncId>,
    frame_result_reprs: HashMap<FuncId, i64>,
) -> (
    ssa::Function,
    Option<ssa::Function>,
    Vec<ssa::StringLiteral>,
) {
    let (name, ret) = match body {
        Some(_) => (BODY_NAME, Type::Void),
        None => ("main", Type::I32),
    };
    let mut f = ssa::Function::new(name, ret);
    let entry = f.add_block();
    let mut new_strings: Vec<ssa::StringLiteral> = Vec::new();
    let mut scans = crate::ssa_lower_ctx_state::ModuleScans::of(env.ast, env.expr_types);
    scans.frame_result_reprs = frame_result_reprs;
    {
        let target = MainTarget {
            f: &mut f,
            entry,
            new_strings: &mut new_strings,
            string_id_base,
            suspend_body: body.is_some(),
        };
        let mut ctx = env.ctx(target, &scans);
        lower_top_level(&mut ctx, stmts);
        if ctx.cur_open() {
            if body.is_some() {
                // the wrapper drains and exits once the body is done
                ctx.emit_drops_for_owned_locals();
                let cb = ctx.cur_block;
                ctx.f.set_term(cb, Terminator::Ret(None));
            } else {
                drain_and_exit(&mut ctx, None);
            }
        }
    }
    let Some(body) = body else {
        return (f, None, new_strings);
    };
    let mut wrapper = ssa::Function::new("main", Type::I32);
    let entry = wrapper.add_block();
    let mut wrapper_strings: Vec<ssa::StringLiteral> = Vec::new();
    {
        let target = MainTarget {
            f: &mut wrapper,
            entry,
            new_strings: &mut wrapper_strings,
            string_id_base: string_id_base + new_strings.len(),
            suspend_body: false,
        };
        let mut ctx = env.ctx(target, &scans);
        emit_body_call(&mut ctx, body);
        let exit_code = ModuleCalls::of(&ctx).exit_code;
        drain_and_exit(&mut ctx, Some(exit_code));
    }
    new_strings.extend(wrapper_strings);
    (f, Some(wrapper), new_strings)
}

/// Prime the binding sets and lower each top-level statement, up to
/// the first that closes the block.
fn lower_top_level(ctx: &mut LowerCtx<'_>, stmts: &[&Stmt]) {
    crate::undef_f64_fields::prime(ctx);
    // T-15.g.5 — prime the binding sets BEFORE lowering any
    // top-level let-decl (an unprimed escape-captured `let`
    // stack-allocs and SIGABRTs at env_drop; see the helper doc).
    let _ = ctx.prime_body_binding_sets(stmts.iter().copied());
    crate::ssa_lower::alias_hazard::prime(ctx, stmts.iter().copied());
    crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, stmts.iter().copied());
    let mut prev: Option<&Stmt> = None;
    for s in stmts {
        if !ctx.try_lower_while_fast(prev, s) {
            ctx.lower_top_stmt(s);
        }
        // Terminating statement (top-level throw in particular)
        // closed the block — stop lowering siblings, same as the
        // Block / Multi / try-body / switch-case walks. Without
        // this guard dead siblings append into the terminated
        // block and execute before its terminator.
        if !ctx.cur_open() {
            break;
        }
        prev = Some(*s);
    }
}

/// The program's end: run the queued jobs, take the exit code, tear
/// the top level down, return the code. `exit_code` is the module
/// body's exit-code call (`__torajs_module_exit_code`), which runs
/// the unhandled-rejection sweep itself.
fn drain_and_exit(ctx: &mut LowerCtx<'_>, exit_code: Option<FuncId>) {
    // v0.5 T-15.e — drain pending Promise callbacks FIRST,
    // before any top-level teardown: a deferred settle / user
    // `.then` callback runs user code, which must observe the
    // top-level locals, globals and class registry alive.
    // (Draining after the drops ran callbacks against a
    // partially-torn-down world — rotation 348.) The same
    // checkpoint runs FinalizationRegistry cleanup jobs, whose
    // throw is uncaught like any top-level one.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.host_jobs_drain, vec![]),
    );
    ctx.emit_throw_check(None);
    // P10.5-A3-b — sweep unhandled rejections + capture the
    // exit code while the heap is still fully alive. The
    // reporter resolves a rejected reason's `name` through
    // its class prototype chain, and instance→proto edges
    // are borrowed from the class registry, so the sweep
    // must precede the registry-stake drops below (the
    // rotation-348 promise-pool UAF / rc-underflow family).
    let exit_code = match exit_code {
        Some(fid) => {
            let call = InstKind::Call(fid, vec![]);
            ctx.f.append_inst(ctx.cur_block, call, Type::I32, None)
        }
        None => crate::ssa_lower_main_exit::emit_exit_code(ctx),
    };
    ctx.emit_drops_for_owned_locals();
    ctx.emit_drops_for_globals();
    // r505 (A12) — the class cells the prologue fn handed to
    // the registry.
    crate::ssa_lower_main_exit::emit_class_cell_registry_release(ctx);
    // V3-10.b — drain the cycle collector buffer one last
    // time before returning from main. Cheap when the
    // buffer is empty; sweeps any orphaned cycles
    // accumulated during program lifetime so they don't
    // leak past process exit.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.cycle_at_exit_drain, vec![]),
    );
    // Census lines on fd 2, from a `--cfg torajs_census` build
    // only; the names are declared under the same cfg.
    if cfg!(torajs_census) {
        for name in crate::ssa_lower_intrinsics_runtime_misc::CENSUS_DUMPS {
            let dump = ctx.fn_table[name];
            ctx.f
                .append_void(ctx.cur_block, InstKind::Call(dump, vec![]));
        }
    }
    crate::ssa_lower_main_exit::emit_ret(ctx, exit_code);
}
