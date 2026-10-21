//! The analysis as the egraph pipeline runs it: its off switch, its
//! per-function counts, and the cache later passes read.
//!
//! The facts outlive the analysis: every pass between it and the last
//! reader keeps them id-valid. A function never reuses a value id (new
//! values are pushed), a pass that redefines a value in place leaves
//! it a subset of the values it took (a demoted add, a collapsed
//! loop's sum), and a pass that gives an id a new meaning drops its
//! fact (srem_fquot and srem_parity do).

use crate::id_hash::HashMap;

use torajs_core::ssa::{Function, Module, Type, ValueId};

use super::{FactCache, NumFact, analyze_function_in, transfer};
use torajs_core::ssa::dominator::DominatorTree;

/// The integer range analysis and the cache that tells its later
/// readers which bodies it still describes as analyzed, or `None`
/// under `TORAJS_INTERVAL_OFF=1`; `TORAJS_INTERVAL_STATS=1` prints each
/// fn's fact counts to stderr.
pub fn analyze_gated(module: &Module) -> Option<(Vec<HashMap<ValueId, NumFact>>, FactCache)> {
    if std::env::var("TORAJS_INTERVAL_OFF").as_deref() == Ok("1") {
        return None;
    }
    let names: Vec<String> = module.funcs.iter().map(|f| f.name.clone()).collect();
    let (facts, edges): (Vec<_>, Vec<_>) = module
        .funcs
        .iter()
        .map(|f| {
            if f.is_declaration() {
                (HashMap::default(), None)
            } else {
                let a = analyze_function_in(f, &names, &DominatorTree::compute(f));
                (a.facts, Some(a.edges))
            }
        })
        .unzip();
    if std::env::var("TORAJS_INTERVAL_STATS").as_deref() == Ok("1") {
        for (func, fm) in module.funcs.iter().zip(&facts) {
            if !func.is_declaration() {
                let st = stats_for(fm, func);
                eprintln!("torajs-interval-stats: {} {st:?}", func.name);
            }
        }
    }
    let cache = FactCache::new(module, names, edges);
    Some((facts, cache))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IntervalStats {
    /// Values with a finite (non-sentinel, non-full-i64) fact.
    pub bounded: u32,
    /// i64 values provably inside i32 range (sext_elide seeds).
    pub sext32: u32,
    /// f64-typed values proven integer-valued.
    pub f64_int_valued: u32,
}

pub fn stats_for(facts: &HashMap<ValueId, NumFact>, func: &Function) -> IntervalStats {
    let mut s = IntervalStats::default();
    for (v, f) in facts {
        let ty = func.values[v.0 as usize].ty;
        if f != &NumFact::full_i64() && f.lo > transfer::NEG_INF && f.hi < transfer::POS_INF {
            s.bounded += 1;
        }
        if ty == Type::I64 && f.is_sext32() {
            s.sext32 += 1;
        }
        if ty == Type::F64 {
            s.f64_int_valued += 1;
        }
    }
    s
}
