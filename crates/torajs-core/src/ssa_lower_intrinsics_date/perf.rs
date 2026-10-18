//! `performance.now()` / `performance.timeOrigin` (`torajs-date`
//! `perf.rs`), declared only for a program that names `performance`.

use std::collections::HashMap;

use crate::ast::{Ast, Expr};
use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) const NOW: &str = "__torajs_perf_now";
pub(crate) const TIME_ORIGIN: &str = "__torajs_perf_time_origin";

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>, ast: &Ast) {
    if !(ast.exprs.iter()).any(|e| matches!(e, Expr::Ident(n) if n == "performance")) {
        return;
    }
    declare_intrinsic(module, fn_table, NOW, &[], Type::F64);
    declare_intrinsic(module, fn_table, TIME_ORIGIN, &[], Type::F64);
}
