//! The split of an async body at its suspension points (LLVM CoroSplit,
//! in its place: after inlining, slot promotion, heap-to-stack and the
//! e-graph, before φ promotion, so every value still has one
//! definition).
//!
//! Each body listed in `Module::suspend_fns` was lowered as its resume
//! function: one parameter, the frame, and `InstKind::Suspend` at each
//! `await`. Here it becomes a function the microtask queue can enter at
//! any of its suspension points:
//!
//! 1. every suspension ends its block, and the landing after it is
//!    where the function resumes (`cut`);
//! 2. what lives across a suspension point is carried: values are
//!    rematerialized or demoted to stack slots (`spill`), and the
//!    stack slots that cross are copied through the frame or live in
//!    it (`frame`);
//! 3. each suspension becomes a park (spill the crossing cells, record
//!    the point, hand the frame over, return) and its landing a refill;
//!    every return settles the result through the runtime; a new entry
//!    dispatches on the recorded point (`rewrite`);
//! 4. the frame's trace learns, per point, the words holding what the
//!    body owns there (`trace`);
//! 5. the result is checked (`verify`), and the frame's byte size, now
//!    known, replaces its placeholder in the allocation and the drop.

mod branch;
mod cut;
mod frame;
mod rewrite;
mod spill;
mod trace;
mod verify;

#[cfg(test)]
mod tests;

use torajs_core::ssa::id_hash::{HashMap, HashSet};
use torajs_core::ssa::suspend::FRAME_PARAMS_OFF;
use torajs_core::ssa::{Function, InstKind, Module, Operand, SuspendFn, SuspendRuntime, ValueId};

#[derive(Debug, Default, Clone, Copy)]
pub struct SuspendSplitStats {
    pub functions: u32,
    pub points: u32,
    pub frame_words: u32,
}

pub fn split_module(module: &mut Module) -> SuspendSplitStats {
    let mut stats = SuspendSplitStats::default();
    let Some(rt) = module.suspend_runtime else {
        return stats;
    };
    let mut sizes: HashMap<i64, i64> = HashMap::default();
    for s in module.suspend_fns.clone() {
        let f = &mut module.funcs[s.resume.0 as usize];
        let (words, size) = split_function(f, &s, &rt);
        trace::add_points(&mut module.funcs[s.trace.0 as usize], &words, rt.visit_sig);
        stats.functions += 1;
        stats.points += words.len() as u32;
        stats.frame_words += (size / 8) as u32;
        sizes.insert(s.size_placeholder, size as i64);
    }
    let drops: HashSet<u32> = module.suspend_fns.iter().map(|s| s.drop.0).collect();
    patch_sizes(module, &rt, &sizes, &drops);
    stats
}

/// Split one resume function; answers, per suspension point, the
/// frame words its trace reports, and the frame's byte size.
pub(crate) fn split_function(
    f: &mut Function,
    s: &SuspendFn,
    rt: &SuspendRuntime,
) -> (Vec<Vec<u64>>, u64) {
    let frame = f.params[0];
    let params_end = FRAME_PARAMS_OFF + 8 * u64::from(s.nparams);
    let points = cut::cut_points(f, frame);
    let crossing: HashSet<ValueId> = points
        .iter()
        .flat_map(|p| p.cross.iter().copied())
        .collect();
    let slots = spill::carry_values(f, &crossing, frame, (FRAME_PARAMS_OFF, params_end));
    // a demoted value crosses as its slot
    let crossing_slots: HashSet<ValueId> = (crossing.iter())
        .map(|v| slots.get(v).copied().unwrap_or(*v))
        .collect();
    let layout = frame::lay_out(f, &crossing_slots, params_end);
    let cells: Vec<Vec<(ValueId, u64, torajs_core::ssa::Type)>> = (points.iter())
        .map(|p| {
            let mut c: Vec<_> = (p.cross.iter())
                .map(|v| slots.get(v).copied().unwrap_or(*v))
                .filter_map(|a| layout.copied.get(&a).map(|&(off, ty)| (a, off, ty)))
                .collect();
            c.sort_by_key(|&(_, off, _)| off);
            c
        })
        .collect();
    let words = trace::point_words(f, &points, &slots, &layout, &cells);
    frame::move_into_frame(f, &layout.moved, frame);
    rewrite::settle_returns(f, s, rt, frame);
    rewrite::park_and_land(f, &points, &cells, rt, frame);
    rewrite::dispatch(f, &points, frame);
    verify::verify(f);
    (words, layout.size)
}

/// The frame sizes into the wrappers' allocations (wherever a wrapper
/// was inlined) and the drops' frees and parked-size words.
fn patch_sizes(
    module: &mut Module,
    rt: &SuspendRuntime,
    sizes: &HashMap<i64, i64>,
    drops: &HashSet<u32>,
) {
    let size_of = |op: &mut Operand| {
        if let Operand::ConstI64(c) = op
            && let Some(&n) = sizes.get(c)
        {
            *op = Operand::ConstI64(n);
        }
    };
    for (fi, f) in module.funcs.iter_mut().enumerate() {
        let in_drop = drops.contains(&(fi as u32));
        for b in &mut f.blocks {
            for i in &mut b.insts {
                match &mut i.kind {
                    InstKind::Call(fid, args)
                        if *fid == rt.obj_alloc || *fid == rt.obj_drop_sized =>
                    {
                        args.iter_mut().for_each(size_of)
                    }
                    InstKind::Store(v, _, _) if in_drop => size_of(v),
                    _ => {}
                }
            }
        }
    }
}
