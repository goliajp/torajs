//! Which function a direct call reaches once a clone exists.
//!
//! Inside the clone, an argument it passes itself is an i64 already;
//! a comparison with each end of the window it can leave picks the
//! clone or, outside, the original with the argument's f64 image.
//! The comparisons sit directly on the branches, where branch folding
//! removes the ones the facts on the argument settle. Both arms write
//! the call's result into the one cell; calls from elsewhere are
//! routed in [`super::external`].

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BlockId, FuncId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::body::bridge;
use super::{Affine, Window, int_const};
use crate::cfg_edit::{emit, new_block};
use crate::float_demote::{push_value, split_block_at};

/// What a call site does once the clone exists.
enum Route {
    /// the clone, with these arguments, after these checks
    Clone(Vec<Operand>, Vec<(Operand, IPred, i64)>),
    Original,
}

/// Route the clone's calls to itself; answers (sites, checks).
pub(super) fn redirect_internal(
    f: &mut Function,
    me: FuncId,
    clone_id: FuncId,
    windows: &[Option<Window>],
    aff: &HashMap<ValueId, Affine>,
) -> (u32, u32) {
    let mut sites = calls_to(f, me);
    sites.sort_unstable_by(|a, b| b.cmp(a));
    let (mut count, mut checks) = (0, 0);
    for (b, i) in sites {
        let InstKind::Call(_, args) = &f.blocks[b].insts[i].kind else {
            unreachable!("a collected call site");
        };
        count += 1;
        match internal_route(args, windows, aff) {
            Route::Clone(fast, tests) if tests.is_empty() => {
                f.blocks[b].insts[i].kind = InstKind::Call(clone_id, fast);
            }
            Route::Clone(fast, tests) => {
                checks += tests.len() as u32;
                let (slow, open) = split_call(f, b, i, windows);
                let mut at = BlockId(b as u32);
                for (v, pred, k) in tests {
                    let out = emit(
                        f,
                        at,
                        Type::Bool,
                        InstKind::ICmp(pred, v, Operand::ConstI64(k)),
                    );
                    let next = new_block(f);
                    f.blocks[at.0 as usize].term = Terminator::CondBr {
                        cond: out,
                        then_blk: slow,
                        else_blk: next,
                    };
                    at = next;
                }
                fast_call(f, at, open, clone_id, fast, slow);
            }
            Route::Original => {
                let mut out = Vec::new();
                bridge_narrowed(f, b, i, windows, &mut out);
                f.blocks[b].insts.splice(i..i, out);
            }
        }
    }
    (count, checks)
}

fn internal_route(
    args: &[Operand],
    windows: &[Option<Window>],
    aff: &HashMap<ValueId, Affine>,
) -> Route {
    let mut fast = args.to_vec();
    let mut tests = Vec::new();
    for (i, w) in windows.iter().enumerate() {
        let Some(w) = w else { continue };
        match &args[i] {
            Operand::Value(v) => {
                let a = aff[v];
                let from = windows[a.param].expect("affine in a narrowed parameter");
                if from.hi + a.off > w.hi {
                    tests.push((args[i], IPred::Sgt, w.hi));
                }
                if from.lo + a.off < w.lo {
                    tests.push((args[i], IPred::Slt, w.lo));
                }
            }
            c => match int_const(c) {
                Some(k) if (w.lo..=w.hi).contains(&k) => fast[i] = Operand::ConstI64(k),
                _ => return Route::Original,
            },
        }
    }
    Route::Clone(fast, tests)
}

/// `sitofp` of the call's narrowed i64 arguments into `out`, the call
/// reading them.
fn bridge_narrowed(
    f: &mut Function,
    b: usize,
    i: usize,
    windows: &[Option<Window>],
    out: &mut Vec<Inst>,
) {
    let mut kind = f.blocks[b].insts[i].kind.clone();
    if let InstKind::Call(_, args) = &mut kind {
        for (pos, a) in args.iter_mut().enumerate() {
            if let (Some(_), Operand::Value(v)) = (windows.get(pos).copied().flatten(), *a)
                && f.values[v.0 as usize].ty == Type::I64
            {
                *a = Operand::Value(bridge(&mut f.values, out, v));
            }
        }
    }
    f.blocks[b].insts[i].kind = kind;
}

/// Move the call at `(b, i)` into a block of its own, where it stays
/// as the slow arm writing its result into a cell. Answers that block
/// and the block after it.
pub(super) fn split_call(
    f: &mut Function,
    b: usize,
    i: usize,
    windows: &[Option<Window>],
) -> (BlockId, BlockId) {
    let parts = split_block_at(f, BlockId(b as u32), &[i, i + 1]);
    let (slow, open) = (parts[0], parts[1]);
    let mut out = Vec::new();
    bridge_narrowed(f, slow.0 as usize, 0, windows, &mut out);
    let s = slow.0 as usize;
    if let Some(r) = f.blocks[s].insts[0].result {
        let ty = f.values[r.0 as usize].ty;
        let fresh = push_value(&mut f.values, ty);
        f.blocks[s].insts[0].result = Some(fresh);
        f.blocks[s].insts.push(Inst {
            result: Some(r),
            kind: InstKind::Copy(ty, Operand::Value(fresh)),
            origin: None,
        });
    }
    f.blocks[s].insts.splice(0..0, out);
    (slow, open)
}

/// The clone's call in `at`, writing the cell the slow arm writes.
pub(super) fn fast_call(
    f: &mut Function,
    at: BlockId,
    open: BlockId,
    clone_id: FuncId,
    args: Vec<Operand>,
    slow: BlockId,
) {
    let s = slow.0 as usize;
    let cell = f.blocks[s].insts.last().and_then(|c| match &c.kind {
        InstKind::Copy(ty, _) => c.result.map(|r| (r, *ty)),
        _ => None,
    });
    let origin = f.blocks[s]
        .insts
        .iter()
        .find(|c| matches!(c.kind, InstKind::Call(..)))
        .and_then(|c| c.origin);
    let result = cell.as_ref().map(|(_, ty)| push_value(&mut f.values, *ty));
    let insts = &mut f.blocks[at.0 as usize].insts;
    insts.push(Inst {
        result,
        kind: InstKind::Call(clone_id, args),
        origin,
    });
    if let (Some((r, ty)), Some(fresh)) = (cell, result) {
        insts.push(Inst {
            result: Some(r),
            kind: InstKind::Copy(ty, Operand::Value(fresh)),
            origin: None,
        });
    }
    f.blocks[at.0 as usize].term = Terminator::Br(open);
}

/// Every `(block, index)` holding a direct call to `callee`.
pub(super) fn calls_to(f: &Function, callee: FuncId) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (b, block) in f.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            if matches!(&inst.kind, InstKind::Call(c, _) if *c == callee) {
                out.push((b, i));
            }
        }
    }
    out
}
