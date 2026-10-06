//! Retain/release pairing for a class instance, whose release is not
//! one instruction but the inline sequence lowering expands:
//!
//! ```text
//! B:    rc_inc %x
//!       ...rc-transparent...
//!       %n = (%x == null) | (%x == @undefined_cell)
//!       cond_br %n, after, dec
//! dec:  %rc = load i32, %x
//!       %rc1 = sub %rc, 1
//!       store %rc1, %x
//!       %z = icmp eq %rc1, 0
//!       cond_br %z, walk, buf
//! buf:  call __torajs_cycle_buffer(%x)      ; or the leaf form
//!       br after
//! walk: ...release the fields, free... → after
//! ```
//!
//! With the retain in front, a live `%x` enters `dec` holding at least
//! two references: the count cannot reach zero, `walk` cannot run, and
//! the pair's whole effect is a count put back where it was plus a
//! candidate-root note in `buf`. That note records a reference that
//! went away while others remain; here none went away — the owner
//! changed — so the instance is no more a cycle root than before the
//! pair. A nullish `%x` makes the retain a no-op and takes `after`
//! directly. Either way the block can drop the retain and branch to
//! `after`.
//!
//! Same guards as the single-instruction pairing in `rc_peephole`: the
//! retain and the test share a block, the operands are the same value,
//! and everything between is rc-transparent. `dec` must have no other
//! way in, and every path out of `buf` must reach `after` through the
//! buffer calls alone.
//!
//! A class with a synthesized `__drop_<C>` releases through one call
//! instead (`call __drop_C(%x)`, the same dec / buffer / walk inside
//! the callee). That call pairs with a retain under the same window
//! rules, up the same chain of blocks: `rc_peephole`'s own pairing
//! sees a block at a time, and the chain is what `new C(a, b)` leaves
//! behind when the caller's bindings are released after the ctor
//! retained them.

use crate::id_hash::HashSet;

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use crate::branch_fold::sweep_unreachable_blocks;
use crate::throw_check_fold::{remove_unused, successors};

const UNDEF_CELL: &str = "___TORAJS_UNDEF_CELL";
const OBJ_PROPS_OFF: u64 = 24;
/// `buf` is one call, or the leaf form's test and its call block.
const MAX_BUFFER_BLOCKS: usize = 3;
/// Where an instance's fields start; everything below is header.
const OBJ_FIELDS_OFF: u64 = 32;
/// How far up a straight chain a release looks for its retain.
const MAX_CHAIN_HOPS: usize = 8;

/// Elide every retain paired with an inline class-instance release in
/// `func`, or with a call to one of the synthesized per-class release
/// fns `drop_fids`. `transparent` is the window predicate of the
/// caller; `buffer_fids` are the candidate-root calls `buf` may make
/// and `alloc_fid` the class-instance allocator.
///
/// Runs in rounds: an elided pair turns its block's branch into a
/// jump and strands the release's blocks, which can put a later
/// release in line with a retain several blocks up (`new C(a, b)`
/// retains both arguments into the instance, then the caller's
/// bindings are released one after the other).
pub(crate) fn elide_obj_pairs(
    func: &mut Function,
    transparent: impl Fn(&InstKind) -> bool,
    buffer_fids: &HashSet<u32>,
    alloc_fid: Option<u32>,
    drop_fids: &HashSet<u32>,
) -> u32 {
    let mut elided = 0;
    loop {
        let round = elide_round(func, &transparent, buffer_fids, alloc_fid, drop_fids);
        if round == 0 {
            return elided;
        }
        elided += round;
    }
}

fn elide_round(
    func: &mut Function,
    transparent: &impl Fn(&InstKind) -> bool,
    buffer_fids: &HashSet<u32>,
    alloc_fid: Option<u32>,
    drop_fids: &HashSet<u32>,
) -> u32 {
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); func.blocks.len()];
    for (b, block) in func.blocks.iter().enumerate() {
        for s in successors(&block.term) {
            preds[s.0 as usize].push(b);
        }
    }
    let pred_count: Vec<u32> = preds.iter().map(|p| p.len() as u32).collect();
    // the instances this function minted: a store past the header of
    // one writes a field, never a count
    let cells: HashSet<ValueId> = func
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter_map(|i| match (&i.kind, i.result) {
            (InstKind::Call(f, _), Some(r)) if Some(f.0) == alloc_fid => Some(r),
            _ => None,
        })
        .collect();
    let mut elided = 0;
    let mut dead: Vec<ValueId> = Vec::new();
    let cells = &cells;
    let window = |x: ValueId| {
        move |i: &Inst| {
            i.result != Some(x)
                && (transparent(&i.kind)
                    || matches!(i.kind, InstKind::GlobalRef(_))
                    || keeps_counts(&i.kind, cells))
        }
    };
    for b in 0..func.blocks.len() {
        // one call-form release per block per round: a removal shifts
        // the indices after it, and the next round sees the rest
        let calls: Vec<(usize, ValueId)> = func.blocks[b]
            .insts
            .iter()
            .enumerate()
            .filter_map(|(i, inst)| Some((i, call_release(inst, drop_fids)?)))
            .collect();
        if let Some((i, ri, rb)) = calls.iter().find_map(|&(i, x)| {
            let (rb, ri) = retain_before(func, b, i, x, &preds, window(x))?;
            Some((i, ri, rb))
        }) {
            // the retain sits before the call when both share a block
            func.blocks[b].insts.remove(i);
            func.blocks[rb].insts.remove(ri);
            elided += 1;
            continue;
        }
        let Some(release) = release_at_end(func, b, &pred_count, buffer_fids) else {
            continue;
        };
        let end = func.blocks[b].insts.len();
        let Some((rb, ri)) =
            retain_before(func, b, end, release.value, &preds, window(release.value))
        else {
            continue;
        };
        func.blocks[rb].insts.remove(ri);
        func.blocks[b].term = Terminator::Br(release.after);
        dead.extend(release.test);
        elided += 1;
    }
    if elided > 0 {
        remove_unused(func, &dead);
        sweep_unreachable_blocks(func);
    }
    elided
}

/// `call __drop_C(%x)` — the call-form release of `x`, if `inst` is one.
fn call_release(inst: &Inst, drop_fids: &HashSet<u32>) -> Option<ValueId> {
    match &inst.kind {
        InstKind::Call(f, args) if inst.result.is_none() && drop_fids.contains(&f.0) => {
            match args.as_slice() {
                [Operand::Value(x)] => Some(*x),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The retain of `x` that reaches instruction `end` of block `b`
/// through a window `keeps_window` accepts: in `b` itself, or up the
/// chain of blocks each of which is the only way into the next.
/// Predecessors are those of the round's start; a pair elided since
/// only removed edges, so a chain found here is still one.
fn retain_before(
    func: &Function,
    b: usize,
    end: usize,
    x: ValueId,
    preds: &[Vec<usize>],
    keeps_window: impl Fn(&Inst) -> bool,
) -> Option<(usize, usize)> {
    let mut cur = b;
    let mut end = end;
    let mut hops = 0;
    loop {
        let insts = &func.blocks[cur].insts[..end];
        let retain = insts
            .iter()
            .rposition(|i| matches!(&i.kind, InstKind::RcInc(Operand::Value(v)) if *v == x));
        if let Some(ri) = retain {
            return insts[ri + 1..]
                .iter()
                .all(&keeps_window)
                .then_some((cur, ri));
        }
        if !insts.iter().all(&keeps_window) || hops == MAX_CHAIN_HOPS {
            return None;
        }
        let [p] = preds[cur].as_slice() else {
            return None;
        };
        if !matches!(func.blocks[*p].term, Terminator::Br(t) if t.0 as usize == cur) {
            return None;
        }
        cur = *p;
        end = func.blocks[cur].insts.len();
        hops += 1;
    }
}

/// Instructions that move counts only upward or write a field of an
/// instance this function minted: neither can free anything or lower
/// the count the paired release is about to read.
fn keeps_counts(kind: &InstKind, cells: &HashSet<ValueId>) -> bool {
    match kind {
        InstKind::RcInc(_) | InstKind::RcIncAny(_) => true,
        InstKind::Store(_, Operand::Value(base), off) => {
            *off >= OBJ_FIELDS_OFF && cells.contains(base)
        }
        _ => false,
    }
}

struct Release {
    value: ValueId,
    after: BlockId,
    /// The nullish test's values, dead once the branch is gone.
    test: [ValueId; 4],
}

/// The inline release block `b` ends in, if it ends in one.
fn release_at_end(
    func: &Function,
    b: usize,
    pred_count: &[u32],
    buffer_fids: &HashSet<u32>,
) -> Option<Release> {
    let block = &func.blocks[b];
    let Terminator::CondBr {
        cond: Operand::Value(n),
        then_blk: after,
        else_blk: dec,
    } = block.term
    else {
        return None;
    };
    let def = |v: ValueId| block.insts.iter().find(|i| i.result == Some(v));
    let InstKind::BinOp(BinOp::Or, Operand::Value(is_null), Operand::Value(is_undef)) =
        def(n)?.kind
    else {
        return None;
    };
    let InstKind::ICmp(IPred::Eq, Operand::Value(x), Operand::ConstPtrNull) = def(is_null)?.kind
    else {
        return None;
    };
    let InstKind::ICmp(IPred::Eq, Operand::Value(x2), Operand::Value(cell)) = def(is_undef)?.kind
    else {
        return None;
    };
    if x2 != x || !matches!(&def(cell)?.kind, InstKind::GlobalRef(s) if s == UNDEF_CELL) {
        return None;
    }
    if after == dec || pred_count[dec.0 as usize] != 1 {
        return None;
    }
    let buf = decrement_of(func, dec, x)?;
    reaches_through_buffer_calls(func, buf, after, x, buffer_fids).then_some(Release {
        value: x,
        after,
        test: [n, is_null, is_undef, cell],
    })
}

/// `dec` as exactly the inline decrement of `x`; answers the block
/// taken while references remain.
fn decrement_of(func: &Function, dec: BlockId, x: ValueId) -> Option<BlockId> {
    let block = &func.blocks[dec.0 as usize];
    let [load, sub, store, zero] = block.insts.as_slice() else {
        return None;
    };
    let px = Operand::Value(x);
    let (Some(rc), Some(rc1), Some(z)) = (load.result, sub.result, zero.result) else {
        return None;
    };
    let shape = load.kind == InstKind::Load(Type::I32, px, 0)
        && sub.kind == InstKind::BinOp(BinOp::Sub, Operand::Value(rc), Operand::ConstI32(1))
        && store.kind == InstKind::Store(Operand::Value(rc1), px, 0)
        && zero.kind == InstKind::ICmp(IPred::Eq, Operand::Value(rc1), Operand::ConstI32(0));
    match block.term {
        Terminator::CondBr {
            cond: Operand::Value(c),
            else_blk,
            ..
        } if shape && c == z => Some(else_blk),
        _ => None,
    }
}

/// Does every path from `buf` reach `after` running nothing but the
/// candidate-root calls on `x` and the leaf form's sidecar test?
fn reaches_through_buffer_calls(
    func: &Function,
    buf: BlockId,
    after: BlockId,
    x: ValueId,
    buffer_fids: &HashSet<u32>,
) -> bool {
    let px = Operand::Value(x);
    let allowed = |inst: &Inst| match &inst.kind {
        InstKind::Call(f, args) => {
            inst.result.is_none() && buffer_fids.contains(&f.0) && args.as_slice() == [px]
        }
        InstKind::Load(Type::Ptr, p, OBJ_PROPS_OFF) => *p == px,
        InstKind::ICmp(IPred::Ne, _, Operand::ConstPtrNull) => true,
        _ => false,
    };
    let mut seen: Vec<BlockId> = Vec::new();
    let mut stack = vec![buf];
    while let Some(blk) = stack.pop() {
        if blk == after || seen.contains(&blk) {
            continue;
        }
        let block = &func.blocks[blk.0 as usize];
        if seen.len() == MAX_BUFFER_BLOCKS || !block.insts.iter().all(allowed) {
            return false;
        }
        if matches!(block.term, Terminator::Ret(_) | Terminator::Unreachable) {
            return false;
        }
        seen.push(blk);
        stack.extend(successors(&block.term));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::FuncId;

    const BUFFER: FuncId = FuncId(7);
    const OTHER: FuncId = FuncId(8);
    const ALLOC: FuncId = FuncId(9);

    /// entry: [retain x] [extra] nullish(x) → (after, dec)
    /// dec → (walk, buf) · buf: buffer(x) → after · walk: free → after
    fn pair(retain: bool, extra: Option<InstKind>, buf_call: FuncId) -> (Function, BlockId) {
        let mut f = Function::new("t", Type::I64);
        let x = f.add_param(Type::Ptr, "x");
        let entry = f.add_block();
        if retain {
            f.append_void(entry, InstKind::RcInc(Operand::Value(x)));
        }
        if let Some(kind) = extra {
            f.append_void(entry, kind);
        }
        let dec = release_in(&mut f, entry, x, buf_call);
        (f, dec)
    }

    /// End `blk` with the inline release of `x`; answers its `dec`.
    fn release_in(f: &mut Function, blk: BlockId, x: ValueId, buf_call: FuncId) -> BlockId {
        let px = Operand::Value(x);
        let dec = f.add_block();
        let buf = f.add_block();
        let walk = f.add_block();
        let after = f.add_block();
        let is_null = f.append_inst(
            blk,
            InstKind::ICmp(IPred::Eq, px, Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        let cell = f.append_inst(blk, InstKind::GlobalRef(UNDEF_CELL.into()), Type::Ptr, None);
        let is_undef = f.append_inst(
            blk,
            InstKind::ICmp(IPred::Eq, px, Operand::Value(cell)),
            Type::Bool,
            None,
        );
        let n = f.append_inst(
            blk,
            InstKind::BinOp(BinOp::Or, Operand::Value(is_null), Operand::Value(is_undef)),
            Type::Bool,
            None,
        );
        f.set_term(
            blk,
            Terminator::CondBr {
                cond: Operand::Value(n),
                then_blk: after,
                else_blk: dec,
            },
        );
        let rc = f.append_inst(dec, InstKind::Load(Type::I32, px, 0), Type::I32, None);
        let rc1 = f.append_inst(
            dec,
            InstKind::BinOp(BinOp::Sub, Operand::Value(rc), Operand::ConstI32(1)),
            Type::I32,
            None,
        );
        f.append_void(dec, InstKind::Store(Operand::Value(rc1), px, 0));
        let z = f.append_inst(
            dec,
            InstKind::ICmp(IPred::Eq, Operand::Value(rc1), Operand::ConstI32(0)),
            Type::Bool,
            None,
        );
        f.set_term(
            dec,
            Terminator::CondBr {
                cond: Operand::Value(z),
                then_blk: walk,
                else_blk: buf,
            },
        );
        f.append_void(buf, InstKind::Call(buf_call, vec![px]));
        f.set_term(buf, Terminator::Br(after));
        f.append_void(walk, InstKind::Call(OTHER, vec![px]));
        f.set_term(walk, Terminator::Br(after));
        f.set_term(after, Terminator::Ret(Some(Operand::ConstI64(0))));
        dec
    }

    /// entry: cell = alloc() · retain x · store x into `base + off` ·
    /// retain cell · release x
    fn stored(into_cell: bool, off: u64) -> Function {
        let mut f = Function::new("t", Type::I64);
        let x = f.add_param(Type::Ptr, "x");
        let other = f.add_param(Type::Ptr, "other");
        let entry = f.add_block();
        let cell = f.append_inst(entry, InstKind::Call(ALLOC, Vec::new()), Type::Ptr, None);
        let base = if into_cell { cell } else { other };
        f.append_void(entry, InstKind::RcInc(Operand::Value(x)));
        f.append_void(
            entry,
            InstKind::Store(Operand::Value(x), Operand::Value(base), off),
        );
        f.append_void(entry, InstKind::RcInc(Operand::Value(cell)));
        release_in(&mut f, entry, x, BUFFER);
        f
    }

    /// entry: retain x → mid: [extra] → last: release x
    fn chained(extra: Option<InstKind>) -> (Function, BlockId) {
        let mut f = Function::new("t", Type::I64);
        let x = f.add_param(Type::Ptr, "x");
        let entry = f.add_block();
        let mid = f.add_block();
        let last = f.add_block();
        f.append_void(entry, InstKind::RcInc(Operand::Value(x)));
        f.set_term(entry, Terminator::Br(mid));
        if let Some(kind) = extra {
            f.append_void(mid, kind);
        }
        f.set_term(mid, Terminator::Br(last));
        release_in(&mut f, last, x, BUFFER);
        (f, mid)
    }

    const DROP: FuncId = FuncId(10);

    fn run(f: &mut Function) -> u32 {
        let buffers = HashSet::from_iter([BUFFER.0]);
        let drops = HashSet::from_iter([DROP.0]);
        elide_obj_pairs(
            f,
            crate::optimize::classify_pure,
            &buffers,
            Some(ALLOC.0),
            &drops,
        )
    }

    /// entry: retain x → mid: [extra] → last: call release(x) · ret
    fn chained_call(extra: Option<InstKind>, release: FuncId) -> Function {
        let mut f = Function::new("t", Type::I64);
        let x = f.add_param(Type::Ptr, "x");
        let entry = f.add_block();
        let mid = f.add_block();
        let last = f.add_block();
        f.append_void(entry, InstKind::RcInc(Operand::Value(x)));
        f.set_term(entry, Terminator::Br(mid));
        if let Some(kind) = extra {
            f.append_void(mid, kind);
        }
        f.set_term(mid, Terminator::Br(last));
        f.append_void(last, InstKind::Call(release, vec![Operand::Value(x)]));
        f.append_void(last, InstKind::Call(OTHER, Vec::new()));
        f.set_term(last, Terminator::Ret(Some(Operand::ConstI64(0))));
        f
    }

    #[test]
    fn a_retain_up_a_straight_chain_pairs_with_the_call_release() {
        let mut f = chained_call(None, DROP);
        assert_eq!(run(&mut f), 1);
        assert!(f.blocks[0].insts.is_empty());
        assert_eq!(f.blocks[2].insts.len(), 1);
        assert!(matches!(&f.blocks[2].insts[0].kind, InstKind::Call(c, _) if *c == OTHER));
    }

    #[test]
    fn a_call_along_the_chain_or_an_unknown_callee_keeps_both() {
        let mut f = chained_call(Some(InstKind::Call(OTHER, Vec::new())), DROP);
        assert_eq!(run(&mut f), 0);
        let mut f = chained_call(None, OTHER);
        assert_eq!(run(&mut f), 0);
        assert!(matches!(f.blocks[0].insts[0].kind, InstKind::RcInc(_)));
    }

    #[test]
    fn a_retain_and_a_call_release_in_one_block_pair_in_place() {
        let mut f = Function::new("t", Type::I64);
        let x = f.add_param(Type::Ptr, "x");
        let entry = f.add_block();
        f.append_void(entry, InstKind::Call(OTHER, Vec::new()));
        f.append_void(entry, InstKind::RcInc(Operand::Value(x)));
        f.append_void(entry, InstKind::Call(DROP, vec![Operand::Value(x)]));
        f.append_void(entry, InstKind::Call(DROP, vec![Operand::Value(x)]));
        f.set_term(entry, Terminator::Ret(Some(Operand::ConstI64(0))));
        assert_eq!(run(&mut f), 1);
        assert_eq!(f.blocks[0].insts.len(), 2);
        assert!(matches!(&f.blocks[0].insts[0].kind, InstKind::Call(c, _) if *c == OTHER));
        assert!(matches!(&f.blocks[0].insts[1].kind, InstKind::Call(c, _) if *c == DROP));
    }

    #[test]
    fn a_retain_in_front_of_the_inline_release_goes_with_it() {
        let (mut f, _) = pair(true, None, BUFFER);
        assert_eq!(run(&mut f), 1);
        assert_eq!(f.blocks.len(), 2);
        assert!(f.blocks[0].insts.is_empty());
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
    }

    #[test]
    fn a_release_with_no_retain_stays() {
        let (mut f, _) = pair(false, None, BUFFER);
        assert_eq!(run(&mut f), 0);
        assert_eq!(f.blocks.len(), 5);
    }

    #[test]
    fn a_call_between_the_pair_keeps_both() {
        let (mut f, _) = pair(true, Some(InstKind::Call(OTHER, Vec::new())), BUFFER);
        assert_eq!(run(&mut f), 0);
        assert!(matches!(f.blocks[0].insts[0].kind, InstKind::RcInc(_)));
    }

    #[test]
    fn a_second_way_into_the_decrement_keeps_both() {
        let (mut f, dec) = pair(true, None, BUFFER);
        let side = f.add_block();
        f.set_term(side, Terminator::Br(dec));
        assert_eq!(run(&mut f), 0);
    }

    #[test]
    fn an_unknown_call_on_the_still_referenced_path_keeps_both() {
        let (mut f, _) = pair(true, None, OTHER);
        assert_eq!(run(&mut f), 0);
    }

    #[test]
    fn a_store_into_a_minted_instance_and_another_retain_keep_the_window() {
        let mut f = stored(true, OBJ_FIELDS_OFF + 16);
        assert_eq!(run(&mut f), 1);
        assert!(
            !f.blocks[0].insts.iter().any(
                |i| matches!(&i.kind, InstKind::RcInc(Operand::Value(v)) if *v == f.params[0])
            )
        );
    }

    #[test]
    fn a_store_through_an_unknown_pointer_or_into_a_header_keeps_both() {
        assert_eq!(run(&mut stored(false, OBJ_FIELDS_OFF + 16)), 0);
        assert_eq!(run(&mut stored(true, 0)), 0);
    }

    #[test]
    fn a_retain_up_a_straight_chain_pairs_with_the_release() {
        let (mut f, _) = chained(None);
        assert_eq!(run(&mut f), 1);
        assert!(f.blocks[0].insts.is_empty());
    }

    #[test]
    fn a_call_or_a_second_way_in_along_the_chain_keeps_both() {
        let (mut f, _) = chained(Some(InstKind::Call(OTHER, Vec::new())));
        assert_eq!(run(&mut f), 0);
        let (mut f, mid) = chained(None);
        let side = f.add_block();
        f.set_term(side, Terminator::Br(mid));
        assert_eq!(run(&mut f), 0);
    }
}
