//! The module body that awaits at its top level
//! (`ast::desugar_async_ssa_gate` sets `AsyncSsa::main`): ECMA-262
//! §16.2.1.5.3 Evaluate of a module with top-level await, for a
//! program of one module. The body is lowered like an async
//! function's (`suspend_body`), with no parameters and no result: the
//! module's evaluation promise has no importer to observe it, so none
//! is made. Its awaits run the jobs ahead of their continuation in
//! place (`suspend::in_place`), so it is an ordinary function, which
//! `synthesize_main` puts behind a wrapper `main` that calls it, takes
//! what escaped it, then drains the job queue and exits (the drain and
//! the exit are the ones a module without top-level await runs), with
//! the exit code from `torajs-promise` `module_await`.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::ssa::{FuncId, InstKind, Module, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_synthesize_main::declare_intrinsic;

/// The module body's function name (`main` is the wrapper's): a local
/// label (`ssa::LOCAL_LABEL_PREFIX`), as no source name begins with
/// `.`, so a stripped build gives it no symbol-table row.
pub(crate) const BODY_NAME: &str = ".Lmodule_body";

const AWAIT_BYTE: &str = "__torajs_microtask_await_byte";
const AWAIT_PROMISE: &str = "__torajs_module_await_promise";
const AWAIT_SLOW: &str = "__torajs_module_await_slow";
const AWAIT_SLOW_FRAME: &str = "__torajs_module_await_slow_frame";
const YIELD: &str = "__torajs_microtask_yield";
const YIELD_RETHROW: &str = "__torajs_module_yield_rethrow";
const BODY_END: &str = "__torajs_module_body_end";
const EXIT_CODE: &str = "__torajs_module_exit_code";
const CANCELLED: &str = "__torajs_module_cancelled";

/// The runtime calls of the module body and its wrapper.
#[derive(Clone, Copy)]
pub(crate) struct ModuleCalls {
    pub(crate) await_byte: FuncId,
    pub(crate) await_promise: FuncId,
    pub(crate) await_slow: FuncId,
    pub(crate) await_slow_frame: FuncId,
    pub(crate) yield_direct: FuncId,
    pub(crate) yield_rethrow: FuncId,
    pub(crate) body_end: FuncId,
    pub(crate) exit_code: FuncId,
    pub(crate) cancelled: FuncId,
}

impl ModuleCalls {
    /// The calls [`declare`] put in the function table.
    pub(crate) fn of(ctx: &LowerCtx<'_>) -> ModuleCalls {
        let t = ctx.fn_table;
        ModuleCalls {
            await_byte: t[AWAIT_BYTE],
            await_promise: t[AWAIT_PROMISE],
            await_slow: t[AWAIT_SLOW],
            await_slow_frame: t[AWAIT_SLOW_FRAME],
            yield_direct: t[YIELD],
            yield_rethrow: t[YIELD_RETHROW],
            body_end: t[BODY_END],
            exit_code: t[EXIT_CODE],
            cancelled: t[CANCELLED],
        }
    }
}

/// Declare the calls, for a module that awaits at its top level.
pub(crate) fn declare(ast: &Ast, module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    if !ast.async_ssa.main {
        return;
    }
    let mut decl = |name: &str, params: &[Type], ret: Type| {
        declare_intrinsic(module, fn_table, name, params, ret);
    };
    decl(AWAIT_BYTE, &[Type::Ptr], Type::I64);
    decl(
        AWAIT_PROMISE,
        &[Type::Promise, Type::I32, Type::I32],
        Type::I64,
    );
    let slow = [Type::Promise, Type::I64, Type::I64, Type::I64];
    decl(AWAIT_SLOW, &slow, Type::I64);
    decl(AWAIT_SLOW_FRAME, &slow, Type::I64);
    decl(YIELD, &[], Type::Void);
    decl(YIELD_RETHROW, &[], Type::Void);
    decl(BODY_END, &[], Type::Void);
    decl(EXIT_CODE, &[], Type::I32);
    decl(CANCELLED, &[], Type::I64);
}

/// The wrapper's start: run the body, then take a throw that escaped
/// it (`__torajs_module_body_end`).
pub(crate) fn emit_body_call(ctx: &mut LowerCtx<'_>, body: FuncId) {
    let calls = ModuleCalls::of(ctx);
    let b = ctx.cur_block;
    ctx.f.append_void(b, InstKind::Call(body, Vec::new()));
    ctx.f
        .append_void(b, InstKind::Call(calls.body_end, Vec::new()));
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_body_is_a_local_label() {
        assert!(crate::ssa::is_local_label(super::BODY_NAME));
    }
}
