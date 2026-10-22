//! The `.then` / `.catch` handler twin table, `__torajs_then_twins`.
//!
//! A typed handler handed a value its lane cannot hold runs its any
//! twin through the handler's boxed adapter (torajs-promise
//! `then_twin`). A closure cell carries the adapter at `+32` only while
//! the link keeps its mint's store of it (it elides the store where no
//! dynamic invocation is live), and the bits-ABI wrap of an f64-faced
//! handler carries none, so the runtime finds them through this table:
//! one row per closure a `.then` / `.catch` call may hand over (its
//! body's address, its adapter), one per wrap variant over a closure
//! the module synthesized (the thunk, twice: the twin is capture 0's),
//! and a 0 word ending the walk, all behind a first word naming the
//! module's forwarder to the runtime's `__torajs_then_twin_run`, the
//! one entry into the twin code. Laid out as a relative vtable it lives
//! in `__TEXT` and roots exactly the forwarder and the adapters it
//! names; a module whose `.then` / `.catch` calls hand over no such
//! handler emits none and links the runtime's empty default, whose 0
//! first word keeps the twin code out of the program.
//!
//! A declared function handed over as a bare fn has no row: its typed
//! body has no all-`any` twin to run (the checker cuts one only behind
//! a value that may be called through `any`), and its adapter alone
//! would convert the value into the lane exactly as the reaction does.

use std::collections::HashMap;

use super::PromiseThunks;
use crate::ast::{Ast, Expr};
use crate::num_width::WidthTable;
use crate::ssa::{self, FuncId, InstKind, Module, Operand, SigId, THEN_TWINS_TABLE, Terminator};
use crate::ssa::{Type, VtableGlobal};

/// The runtime's twin runner: `(src, param, v, cb, closure, out) ->
/// twin_ran`.
const TWIN_RUN_SYM: &str = "__torajs_then_twin_run";

/// Each handler argument of a `.then` / `.catch` call, as the fns it
/// may name: `f` gets an identifier's declaration, every closure a
/// binding may hold, an inline closure's lifted fn, and `""` for an
/// argument that names none (a field read, a fn-typed parameter).
pub(crate) fn then_handlers<'a>(ast: &'a Ast, table: &'a WidthTable, mut f: impl FnMut(&'a str)) {
    for args in then_calls(ast) {
        for a in args.iter().take(2) {
            match ast.get_expr(*a) {
                Expr::Ident(n) if table.bound_closures(*a).is_empty() => f(n.as_str()),
                Expr::Ident(_) => table.bound_closures(*a).iter().for_each(|c| f(c.as_str())),
                Expr::Closure { fn_name, .. } => f(fn_name.as_str()),
                _ => f(""),
            }
        }
    }
}

/// The argument lists of every `.then` / `.catch` call.
fn then_calls(ast: &Ast) -> impl Iterator<Item = &[crate::ast::ExprId]> {
    ast.exprs.iter().filter_map(|e| {
        let Expr::Call { callee, args } = e else {
            return None;
        };
        let Expr::Member { name, .. } = ast.get_expr(*callee) else {
            return None;
        };
        matches!(name.as_str(), "then" | "catch").then_some(args.as_slice())
    })
}

/// Append the module's twin table to its vtables, when it has a row.
pub(crate) fn push_then_twins(
    boxed_entries: &HashMap<FuncId, (FuncId, SigId)>,
    thunks: &PromiseThunks,
    module: &mut Module,
) {
    let handlers = (thunks.handlers.iter())
        .filter_map(|fid| boxed_entries.get(fid).map(|&(adapter, _)| (*fid, adapter)));
    let wraps = (thunks.map.iter())
        .filter(|((inner_closure, _, _), _)| *inner_closure)
        .map(|(_, &(thunk, _))| (thunk, thunk));
    let mut rows: Vec<(FuncId, FuncId)> = handlers.chain(wraps).collect();
    let Some(run) = thunks.twin_run else {
        return;
    };
    if rows.is_empty() && !thunks.unnamed {
        return;
    }
    rows.sort_unstable_by_key(|&(handler, _)| handler.0);
    let fn_ids = (rows.into_iter())
        .flat_map(|(handler, adapter)| [Some(handler), Some(adapter)])
        .chain([None]);
    let fn_ids = [Some(run)].into_iter().chain(fn_ids).collect();
    module.vtable_globals.push(VtableGlobal {
        class_name: THEN_TWINS_TABLE.to_string(),
        fn_ids,
    });
}

/// The module's forwarder to the runtime's twin runner, whose address
/// the table's first word holds: a user body, so the table can name it
/// and its call roots the runner.
pub(super) fn build_twin_run(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, SigId>,
) -> FuncId {
    let tys = [
        Type::I64,
        Type::I64,
        Type::I64,
        Type::Ptr,
        Type::I64,
        Type::Ptr,
    ];
    let runner = crate::ssa_lower_synthesize_main::declare_intrinsic(
        module,
        fn_table,
        TWIN_RUN_SYM,
        &tys,
        Type::I64,
    );
    let name = "__then_twin_run";
    let fid = FuncId(module.funcs.len() as u32);
    let sig = crate::ssa_lower::intern_fn_sig(fn_sigs, tys.to_vec(), Type::I64);
    fn_table.insert(name.to_string(), fid);
    fn_sig_ids.insert(fid, sig);
    let mut f = ssa::Function::new(name, Type::I64);
    let args = (tys.iter().enumerate())
        .map(|(i, &t)| Operand::Value(f.add_param(t, &format!("a{i}"))))
        .collect();
    let entry = f.add_block();
    let r = f.append_inst(entry, InstKind::Call(runner, args), Type::I64, None);
    f.set_term(entry, Terminator::Ret(Some(Operand::Value(r))));
    module.funcs.push(f);
    fid
}
