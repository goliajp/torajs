//! Integer clones of self-recursive number functions.
//!
//! A parameter that arrives as an f64 nothing is known of
//! (`fib(parseInt(argv[2]))`) keeps every operation on it in f64 for
//! the whole recursion: the interval analysis gives a parameter no
//! fact, and calls are where the loop-versioning entry checks cannot
//! reach. A function qualifies when it calls itself and every
//! argument it passes itself for an f64 parameter is such a
//! parameter plus or minus an integer constant, or an integer
//! constant; the recursion then closes over integers.
//!
//! The clone takes those parameters as i64 and returns what the
//! original returns ([`body`]). Its entry asserts the window each one
//! lies in (a branch to an empty `unreachable` block, removed by
//! [`crate::strip_assumes`] once the analyses have read it), its own
//! calls pick the clone when the argument is inside the window and
//! the original otherwise, and a direct call from anywhere else
//! checks the f64 once — the `Round::Exact` chain of `int_entry` —
//! and takes the clone only when it holds an integer inside the
//! window ([`dispatch`], [`external`]). NaN, -0, a fraction, an
//! infinity and anything outside the window take the original, so no
//! input is answered differently. A call through a function value
//! and a call with too few arguments keep calling the original.
//!
//! Runs after `frem_narrow` and before `int_entry` and the interval
//! analysis, so the clone goes through the whole tail of the
//! pipeline. `TORAJS_FN_INT_CLONE_OFF=1` skips it.

mod body;
mod dispatch;
mod external;
mod hoist;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    FuncId, Function, InstKind, LOCAL_LABEL_PREFIX, Module, Operand, Type, ValueId,
};

use crate::float_demote::cfg::multi_def_values;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FnIntCloneStats {
    pub functions_cloned: u32,
    pub params_narrowed: u32,
    /// calls from outside a clone's family that now check and dispatch
    pub external_sites: u32,
    /// calls a clone makes to itself
    pub internal_sites: u32,
    /// window comparisons in front of those calls, before branch folding
    pub internal_checks: u32,
    /// (original, clone)
    pub clones: Vec<(FuncId, FuncId)>,
}

/// The window a narrowed parameter is asserted to lie in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Window {
    pub(crate) lo: i64,
    pub(crate) hi: i64,
}

/// Half of the exact range, as `int_entry` takes it: every value the
/// clone derives stays within `MAX_OFFSET` of a parameter, so it
/// stays exact as an f64 too.
const EXACT_MAX: i64 = 1 << 52;
const UNSIGNED: Window = Window {
    lo: 0,
    hi: EXACT_MAX - 1,
};
const SYMMETRIC: Window = Window {
    lo: -EXACT_MAX,
    hi: EXACT_MAX,
};
const MAX_OFFSET: i64 = 1 << 20;
const FUNCTION_INST_BUDGET: usize = 512;
const MODULE_INST_BUDGET: usize = 2000;
const MARK_ARGUMENTS: &str = "__torajs_arr_mark_arguments";

/// A value that is narrowed parameter `param` plus `off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Affine {
    pub(crate) param: usize,
    pub(crate) off: i64,
}

pub fn clone_functions(module: &mut Module) -> FnIntCloneStats {
    let mut stats = FnIntCloneStats::default();
    let mark = module.funcs.iter().position(|f| f.name == MARK_ARGUMENTS);
    // no source name is a local label, so only clones can collide
    let mut names: HashSet<String> = HashSet::default();
    let mut budget = MODULE_INST_BUDGET;
    let originals = module.funcs.len();
    let mut planned: Vec<Vec<Option<Window>>> = Vec::new();
    // per original function, worked out once while it is unchanged
    let mut loops: Vec<Option<hoist::Loops>> = (0..originals).map(|_| None).collect();
    for fi in 0..originals {
        let Some(windows) = plan(&module.funcs[fi], FuncId(fi as u32), mark) else {
            continue;
        };
        let size = inst_count(&module.funcs[fi]);
        if size > budget || !external::has_external_site(module, fi, &windows, &mut loops) {
            continue;
        }
        budget -= size;
        let clone_id = FuncId(module.funcs.len() as u32);
        let mut name = format!("{LOCAL_LABEL_PREFIX}{}.int", module.funcs[fi].name);
        while names.contains(&name) {
            name.push('_');
        }
        names.insert(name.clone());
        let (clone, sites, checks) = body::build_clone(
            &module.funcs[fi],
            FuncId(fi as u32),
            clone_id,
            &windows,
            name,
        );
        module.funcs.push(clone);
        stats.functions_cloned += 1;
        stats.params_narrowed += windows.iter().flatten().count() as u32;
        stats.internal_sites += sites;
        stats.internal_checks += checks;
        stats.clones.push((FuncId(fi as u32), clone_id));
        planned.push(windows);
    }
    for (&(orig, clone), windows) in stats.clones.iter().zip(&planned) {
        stats.external_sites +=
            external::redirect_external(module, orig, clone, windows, &mut loops);
    }
    stats
}

/// The window of each parameter the clone narrows, by position; none
/// when the function does not qualify.
fn plan(func: &Function, me: FuncId, mark: Option<usize>) -> Option<Vec<Option<Window>>> {
    let dense = func
        .blocks
        .iter()
        .enumerate()
        .all(|(i, b)| b.id.0 as usize == i);
    if func.is_declaration() || !dense || func.name.starts_with("__") {
        return None;
    }
    if inst_count(func) > FUNCTION_INST_BUDGET {
        return None;
    }
    let mut self_calls: Vec<&[Operand]> = Vec::new();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        match &inst.kind {
            InstKind::Call(f, args) if *f == me => self_calls.push(args),
            // `arguments` reads the parameters as they arrived
            InstKind::Call(f, _) if Some(f.0 as usize) == mark => return None,
            _ => {}
        }
    }
    if self_calls.is_empty() || self_calls.iter().any(|a| a.len() != func.params.len()) {
        return None;
    }
    // every f64 parameter closes, or none is narrowed: one whose
    // argument is a call's result (`ackermann`'s second) would take a
    // full conversion check on every call
    let narrowed: Vec<bool> = func
        .params
        .iter()
        .map(|p| func.values[p.0 as usize].ty == Type::F64)
        .collect();
    let aff = affine_values(func, &narrowed);
    let closed = self_calls.iter().all(|args| {
        args.iter()
            .enumerate()
            .all(|(i, arg)| !narrowed[i] || closes(arg, &aff))
    });
    if !closed || !narrowed.contains(&true) {
        return None;
    }
    let windows = (0..func.params.len())
        .map(|i| {
            narrowed[i].then(|| {
                let negative = self_calls
                    .iter()
                    .any(|args| matches!(args[i], Operand::ConstF64(c) if c < 0.0));
                if negative { SYMMETRIC } else { UNSIGNED }
            })
        })
        .collect();
    Some(windows)
}

/// The argument keeps the recursion on integers.
fn closes(arg: &Operand, aff: &HashMap<ValueId, Affine>) -> bool {
    match arg {
        Operand::Value(v) => aff.contains_key(v),
        op => int_const(op).is_some(),
    }
}

/// An f64 constant that is an integer, not -0, inside the exact range.
pub(crate) fn int_const(op: &Operand) -> Option<i64> {
    match op {
        Operand::ConstF64(c)
            if c.is_finite()
                && c.fract() == 0.0
                && !(*c == 0.0 && c.is_sign_negative())
                && c.abs() <= EXACT_MAX as f64 =>
        {
            Some(*c as i64)
        }
        _ => None,
    }
}

/// Every single-def value that is a narrowed parameter plus an
/// integer constant, through `fadd` / `fsub` / `copy`.
pub(crate) fn affine_values(func: &Function, narrowed: &[bool]) -> HashMap<ValueId, Affine> {
    let cells = multi_def_values(func);
    let mut out: HashMap<ValueId, Affine> = HashMap::default();
    for (i, p) in func.params.iter().enumerate() {
        if narrowed[i] {
            out.insert(*p, Affine { param: i, off: 0 });
        }
    }
    loop {
        let before = out.len();
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            let Some(r) = inst.result else { continue };
            if cells.contains(&r) || out.contains_key(&r) {
                continue;
            }
            if let Some(a) = step(&inst.kind, &out).filter(|a| a.off.abs() <= MAX_OFFSET) {
                out.insert(r, a);
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

fn step(kind: &InstKind, aff: &HashMap<ValueId, Affine>) -> Option<Affine> {
    use torajs_core::ssa::BinOp::{FAdd, FSub};
    let shifted = |x: &ValueId, by: i64| {
        aff.get(x).map(|a| Affine {
            param: a.param,
            off: a.off + by,
        })
    };
    match kind {
        InstKind::BinOp(FAdd, Operand::Value(x), c) if aff.contains_key(x) => {
            shifted(x, int_const(c)?)
        }
        InstKind::BinOp(FAdd, c, Operand::Value(x)) if aff.contains_key(x) => {
            shifted(x, int_const(c)?)
        }
        InstKind::BinOp(FSub, Operand::Value(x), c) => shifted(x, -int_const(c)?),
        InstKind::Copy(Type::F64, Operand::Value(x)) => shifted(x, 0),
        _ => None,
    }
}

fn inst_count(func: &Function) -> usize {
    (func.blocks.iter().flat_map(|b| &b.insts))
        .filter(|i| i.kind != InstKind::ThrowSignal)
        .count()
}

#[cfg(test)]
mod tests;
