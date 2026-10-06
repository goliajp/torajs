//! Chain flattening — a concat chain whose result is only ever
//! appended onto another cell and dropped writes its pieces onto that
//! cell directly, and the chain's own cells are never built.
//!
//! A callback inlined into a `replace` walk leaves this in the loop
//! body (`v + ":" + k`, after the pair rewrite in the parent module):
//!
//! ```text
//! %t = call __torajs_substr_concat_substr_str(%v, @":")
//! %r = call __torajs_str_append_substr(%t, %k)
//! %s = call __torajs_str_append(%o, %r)
//! call __torajs_str_drop(%r)
//! ```
//!
//! `%r` exists only to be copied onto `%o`, so every piece it was
//! built from goes onto `%o` instead:
//!
//! ```text
//! %1 = call __torajs_str_append_substr(%o, %v)
//! %2 = call __torajs_str_append(%1, @":")
//! %s = call __torajs_str_append_substr(%2, %k)
//! ```
//!
//! A sink is a kernel call taking a Str right operand `%r` that a
//! kernel in the same block defined and that nothing but the sink and
//! one later `str_drop` reads. The chain is the sink's right operand
//! expanded through every operand the block's kernels defined and
//! nothing else reads; the rest are leaves. Between the first chain
//! definition and the sink only chain definitions, static strings and
//! drops may sit, and every drop there is of a chain value (deleted
//! with it) or of a leaf (it becomes the leaf's own release, run once
//! the leaf is on the cell). Anything else in that range keeps the
//! chain as it is: the leaves are read later than before, and a
//! window of nothing but string kernels on the chain's own values is
//! what keeps them meaning the same bytes.
//!
//! The sink's left cell may alias a leaf — `acc + (acc + x)`, or a
//! view of the cell — and that is left to the kernels: they decline
//! the in-place path on a pointer match, and a cell any leaf can name
//! is held by that leaf's owner too, so it is never solely the
//! accumulator's to grow. A kernel that consumed a leaf (the chain's
//! root appended onto a temp from an earlier block) releases it
//! through an explicit drop after the piece is on the cell — the same
//! release, one instruction later.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    Block, FuncId, Inst, InstKind, Operand, Type, ValueId, ValueInfo, visit_value_operands,
};

use super::kernels::{Kernels, Kind};

/// One operand the flattened sequence appends, with the release it
/// owes once the bytes are on the cell.
struct Piece {
    op: Operand,
    kind: Kind,
    drop: Option<Inst>,
}

struct Plan {
    sink: usize,
    /// Earliest chain definition: the window runs from here to the sink.
    first: usize,
    delete: Vec<usize>,
    pieces: Vec<Piece>,
    /// The kernel each piece goes onto the cell through.
    fids: Vec<FuncId>,
}

/// Flatten every chain in `block`; answers how many.
pub(super) fn flatten_block(
    block: &mut Block,
    values: &mut Vec<ValueInfo>,
    kernels: &Kernels,
    uses: &mut HashMap<ValueId, u32>,
) -> u32 {
    let mut flattened = 0;
    let mut from = 0;
    while let Some(plan) = find_plan(&block.insts, from, kernels, uses) {
        from = apply(block, values, uses, plan);
        flattened += 1;
    }
    flattened
}

fn find_plan(
    insts: &[Inst],
    from: usize,
    kernels: &Kernels,
    uses: &HashMap<ValueId, u32>,
) -> Option<Plan> {
    for s in from..insts.len() {
        let Some((sink, args)) = kernels.call(&insts[s]) else {
            continue;
        };
        let Operand::Value(r) = args[1] else {
            continue;
        };
        if sink.right != Kind::Str || insts[s].result.is_none() || count(uses, r) != 2 {
            continue;
        }
        let Some(d) = kernel_def(insts, r, s, kernels) else {
            continue;
        };
        let Some(e) = drop_at(insts, r, s, kernels) else {
            continue;
        };
        let mut plan = Plan {
            sink: s,
            first: d,
            delete: vec![d, e],
            pieces: Vec::new(),
            fids: Vec::new(),
        };
        if expand(insts, d, s, kernels, uses, &mut plan).is_some()
            && claim_window_drops(insts, kernels, &mut plan).is_some()
            && pick_flavors(kernels, sink.left, sink.consumes_left, &mut plan).is_some()
        {
            return Some(plan);
        }
    }
    None
}

fn count(uses: &HashMap<ValueId, u32>, v: ValueId) -> u32 {
    uses.get(&v).copied().unwrap_or(0)
}

/// Index before `s` of the binary kernel call defining `v`.
fn kernel_def(insts: &[Inst], v: ValueId, s: usize, kernels: &Kernels) -> Option<usize> {
    insts[..s]
        .iter()
        .position(|i| i.result == Some(v) && kernels.call(i).is_some())
}

/// Index after `at` of the drop releasing `v`.
fn drop_at(insts: &[Inst], v: ValueId, at: usize, kernels: &Kernels) -> Option<usize> {
    insts
        .iter()
        .enumerate()
        .skip(at + 1)
        .find(|(_, i)| kernels.dropped(i) == Some(&Operand::Value(v)))
        .map(|(j, _)| j)
}

/// Expand the chain definition at `i`: each operand is another chain
/// definition (read here and nowhere else but its drop) or a leaf.
fn expand(
    insts: &[Inst],
    i: usize,
    s: usize,
    kernels: &Kernels,
    uses: &HashMap<ValueId, u32>,
    plan: &mut Plan,
) -> Option<()> {
    let (k, args) = kernels.call(&insts[i])?;
    let slots = [
        (args[0], k.left, k.consumes_left),
        (args[1], k.right, false),
    ];
    for (op, kind, consumed) in slots {
        let inner = match op {
            Operand::Value(v) => kernel_def(insts, v, s, kernels).map(|j| (v, j)),
            _ => None,
        };
        let Some((v, j)) = inner else {
            let drop = consumed.then(|| Inst {
                result: None,
                kind: InstKind::Call(kernels.str_drop, vec![op]),
                origin: insts[i].origin,
            });
            plan.pieces.push(Piece { op, kind, drop });
            continue;
        };
        match (consumed, drop_at(insts, v, j, kernels), count(uses, v)) {
            (true, None, 1) => {}
            (false, Some(e), 2) => plan.delete.push(e),
            _ => return None,
        }
        plan.delete.push(j);
        plan.first = plan.first.min(j);
        expand(insts, j, s, kernels, uses, plan)?;
    }
    Some(())
}

/// The window between the first chain definition and the sink holds
/// nothing but chain definitions, static strings and drops; a drop of
/// a leaf there becomes that leaf's own release.
fn claim_window_drops(insts: &[Inst], kernels: &Kernels, plan: &mut Plan) -> Option<()> {
    let (first, sink) = (plan.first, plan.sink);
    for (k, inst) in insts[first..sink].iter().enumerate() {
        let i = first + k;
        if plan.delete.contains(&i) || matches!(inst.kind, InstKind::StaticStrRef(_)) {
            continue;
        }
        let dropped = kernels.dropped(inst)?;
        let mut owners = plan.pieces.iter_mut().filter(|p| p.op == *dropped);
        let piece = owners.next()?;
        if owners.next().is_some() || piece.drop.is_some() {
            return None;
        }
        piece.drop = Some(inst.clone());
        plan.delete.push(i);
    }
    Some(())
}

/// The first piece goes onto the sink's left operand the way the
/// sink did; every later one is appended onto the fresh cell that
/// grows from there.
fn pick_flavors(kernels: &Kernels, left: Kind, consumes: bool, plan: &mut Plan) -> Option<()> {
    let (mut left, mut consumes) = (left, consumes);
    for piece in &plan.pieces {
        plan.fids.push(kernels.flavor(left, consumes, piece.kind)?);
        left = Kind::Str;
        consumes = true;
    }
    Some(())
}

/// Rewrite the block; answers the index just past the new sequence.
fn apply(
    block: &mut Block,
    values: &mut Vec<ValueInfo>,
    uses: &mut HashMap<ValueId, u32>,
    plan: Plan,
) -> usize {
    let sink = &block.insts[plan.sink];
    let InstKind::Call(_, sink_args) = &sink.kind else {
        unreachable!("a sink is a kernel call");
    };
    let (sink_result, origin) = (sink.result, sink.origin);
    let last = plan.pieces.len() - 1;
    let mut acc = sink_args[0];
    let mut seq = Vec::with_capacity(plan.pieces.len() * 2);
    for (idx, (piece, fid)) in plan.pieces.into_iter().zip(plan.fids).enumerate() {
        let result = if idx == last {
            sink_result.expect("a sink defines its result")
        } else {
            values.push(ValueInfo {
                ty: Type::Str,
                name: None,
            });
            ValueId((values.len() - 1) as u32)
        };
        seq.push(Inst {
            result: Some(result),
            kind: InstKind::Call(fid, vec![acc, piece.op]),
            origin,
        });
        seq.extend(piece.drop);
        acc = Operand::Value(result);
    }
    let mut delete = plan.delete;
    delete.push(plan.sink);
    delete.sort_unstable();
    delete.dedup();
    let sink_at = plan.sink - delete.iter().filter(|&&i| i < plan.sink).count();
    for &i in delete.iter().rev() {
        let gone = block.insts.remove(i);
        // the sink's result lives on as the last append's
        if let Some(v) = gone.result.filter(|_| i != plan.sink) {
            uses.remove(&v);
        }
        visit_value_operands(&gone.kind, |v| *uses.entry(v).or_insert(1) -= 1);
    }
    for inst in &seq {
        visit_value_operands(&inst.kind, |v| *uses.entry(v).or_insert(0) += 1);
    }
    let len = seq.len();
    block.insts.splice(sink_at..sink_at, seq);
    sink_at + len
}

#[cfg(test)]
mod tests {
    use super::super::rewrite_str_appends;
    use super::super::test_support::*;
    use super::*;
    use torajs_core::ssa::Terminator;

    /// `(m, k, v) => v + ":" + k` inlined into the walk: `%1` is `v`,
    /// `%2` is `k`, `%3` the result cell, `%10` the literal.
    fn callback_two_pieces() -> Vec<Inst> {
        vec![
            static_str(10),
            call(SUBSTR_CONCAT_SUBSTR_STR, 11, &[1, 10]),
            call(SUBSTR_CONCAT_STR_SUBSTR, 12, &[11, 2]),
            drop_of(11),
            call(STR_APPEND, 13, &[3, 12]),
            drop_of(12),
        ]
    }

    #[test]
    fn the_callback_chain_goes_straight_onto_the_cell() {
        let mut m = module_with(callback_two_pieces());
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1);
        assert_eq!(stats.flattened, 1);
        let c = calls(&m, 0);
        assert_eq!(c.len(), 3, "three appends, no concat, no drop");
        assert_eq!((c[0].0, c[0].2.clone()), (STR_APPEND_SUBSTR, vec![3, 1]));
        assert_eq!(c[1].0, STR_APPEND);
        assert_eq!(c[1].2, vec![c[0].1.unwrap(), 10]);
        assert_eq!(c[2].0, STR_APPEND_SUBSTR);
        assert_eq!(
            (c[2].1, c[2].2.clone()),
            (Some(13), vec![c[1].1.unwrap(), 2])
        );
        assert!(matches!(
            m.funcs[BODY].blocks[0].insts[0].kind,
            InstKind::StaticStrRef(_)
        ));
        assert_eq!(m.funcs[BODY].values[c[0].1.unwrap() as usize].ty, Type::Str);
    }

    #[test]
    fn five_pieces_become_five_appends() {
        let mut m = module_with(vec![
            concat(10, 1, 2),
            concat(11, 10, 3),
            drop_of(10),
            concat(12, 11, 4),
            drop_of(11),
            concat(13, 12, 5),
            drop_of(12),
            call(STR_APPEND, 14, &[6, 13]),
            drop_of(13),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 1);
        let c = calls(&m, 0);
        let rights: Vec<u32> = c.iter().map(|x| x.2[1]).collect();
        assert_eq!(rights, vec![1, 2, 3, 4, 5]);
        assert!(c.iter().all(|x| x.0 == STR_APPEND));
        assert_eq!(c[0].2[0], 6);
        assert_eq!(c[4].1, Some(14));
        for w in c.windows(2) {
            assert_eq!(w[1].2[0], w[0].1.unwrap(), "each append grows the last");
        }
    }

    #[test]
    fn a_borrowing_sink_keeps_its_first_link_a_concat() {
        // `x = o + (a + b)` where `o` stays alive
        let mut m = module_with(vec![concat(10, 1, 2), concat(11, 3, 10), drop_of(10)]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 1);
        let c = calls(&m, 0);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].0, c[0].2.clone()), (STR_CONCAT, vec![3, 1]));
        assert_eq!(
            (c[1].0, c[1].1, c[1].2.clone()),
            (STR_APPEND, Some(11), vec![c[0].1.unwrap(), 2])
        );
    }

    #[test]
    fn a_view_sink_takes_its_first_piece_through_the_view_kernels() {
        // `view + (view2 + ":")`
        let mut m = module_with(vec![
            static_str(10),
            call(SUBSTR_CONCAT_SUBSTR_STR, 11, &[1, 10]),
            call(SUBSTR_CONCAT_SUBSTR_STR, 12, &[5, 11]),
            drop_of(11),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 1);
        let c = calls(&m, 0);
        assert_eq!(c.len(), 2);
        assert_eq!(
            (c[0].0, c[0].2.clone()),
            (SUBSTR_CONCAT_SUBSTR_SUBSTR, vec![5, 1])
        );
        assert_eq!(
            (c[1].0, c[1].1, c[1].2.clone()),
            (STR_APPEND, Some(12), vec![c[0].1.unwrap(), 10])
        );
    }

    #[test]
    fn a_result_that_is_also_stored_keeps_its_cell() {
        let mut insts = callback_two_pieces();
        let store = Inst {
            result: None,
            kind: InstKind::Store(v(12), v(20), 0),
            origin: None,
        };
        insts.insert(5, store);
        let mut m = module_with(insts);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1, "the pair still folds");
        assert_eq!(stats.flattened, 0);
        assert_eq!(calls(&m, 0).len(), 4);
    }

    #[test]
    fn a_result_that_is_also_returned_keeps_its_cell() {
        let mut m = module_with_blocks(vec![callback_two_pieces()], Terminator::Ret(Some(v(12))));
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 0);
    }

    #[test]
    fn a_foreign_instruction_in_the_window_keeps_the_chain() {
        let mut insts = callback_two_pieces();
        let between = Inst {
            result: Some(ValueId(21)),
            kind: InstKind::Copy(Type::Str, v(3)),
            origin: None,
        };
        insts.insert(4, between);
        let mut m = module_with(insts);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1);
        assert_eq!(stats.flattened, 0);
    }

    #[test]
    fn a_call_in_the_window_keeps_the_chain() {
        // a result-less one-argument call that is not a drop
        let mut insts = callback_two_pieces();
        let between = Inst {
            result: None,
            kind: InstKind::Call(FuncId(BODY as u32), vec![v(1)]),
            origin: None,
        };
        insts.insert(4, between);
        let mut m = module_with(insts);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.rewritten, 1);
        assert_eq!(stats.flattened, 0);
    }

    #[test]
    fn a_root_from_an_earlier_block_is_appended_and_released() {
        // a throw check split the callback body: the first link sits
        // in the block before the sink's
        let mut m = module_with_blocks(
            vec![
                vec![static_str(10), call(SUBSTR_CONCAT_SUBSTR_STR, 11, &[1, 10])],
                vec![
                    call(SUBSTR_CONCAT_STR_SUBSTR, 12, &[11, 2]),
                    drop_of(11),
                    call(STR_APPEND, 13, &[3, 12]),
                    drop_of(12),
                ],
            ],
            Terminator::Ret(None),
        );
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 1);
        assert_eq!(calls(&m, 0).len(), 1, "the earlier block is untouched");
        let c = calls(&m, 1);
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].0, c[0].2.clone()), (STR_APPEND, vec![3, 11]));
        assert_eq!((c[1].0, c[1].1, c[1].2.clone()), (STR_DROP, None, vec![11]));
        assert_eq!(
            (c[2].0, c[2].1, c[2].2.clone()),
            (STR_APPEND_SUBSTR, Some(13), vec![c[0].1.unwrap(), 2])
        );
    }

    #[test]
    fn a_leaf_dropped_in_the_window_is_released_after_its_append() {
        // `%9` is a temp from a call before the chain: `a + f() + b`
        let mut m = module_with(vec![
            concat(10, 1, 9),
            drop_of(9),
            concat(11, 10, 2),
            drop_of(10),
            call(STR_APPEND, 12, &[3, 11]),
            drop_of(11),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 1);
        let c = calls(&m, 0);
        assert_eq!(c.len(), 4);
        assert_eq!((c[0].0, c[0].2.clone()), (STR_APPEND, vec![3, 1]));
        assert_eq!(
            (c[1].0, c[1].2.clone()),
            (STR_APPEND, vec![c[0].1.unwrap(), 9])
        );
        assert_eq!((c[2].0, c[2].1, c[2].2.clone()), (STR_DROP, None, vec![9]));
        assert_eq!(
            (c[3].0, c[3].1, c[3].2.clone()),
            (STR_APPEND, Some(12), vec![c[1].1.unwrap(), 2])
        );
    }

    #[test]
    fn a_leaf_read_twice_and_dropped_in_the_window_keeps_the_chain() {
        // `(x + a) + x` with `x` a temp: its release cannot follow
        // both reads
        let mut m = module_with(vec![
            concat(10, 9, 1),
            concat(11, 10, 9),
            drop_of(10),
            drop_of(9),
            call(STR_APPEND, 12, &[3, 11]),
            drop_of(11),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 0);
    }

    #[test]
    fn a_leaf_read_twice_without_a_release_is_appended_twice() {
        let mut m = module_with(vec![
            concat(10, 9, 1),
            concat(11, 10, 9),
            drop_of(10),
            call(STR_APPEND, 12, &[3, 11]),
            drop_of(11),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 1);
        let rights: Vec<u32> = calls(&m, 0).iter().map(|x| x.2[1]).collect();
        assert_eq!(rights, vec![9, 1, 9]);
    }

    #[test]
    fn an_intermediate_read_by_the_sink_itself_keeps_the_chain() {
        // `%10` is both a link and the cell appended onto
        let mut m = module_with(vec![
            concat(10, 1, 2),
            concat(11, 10, 3),
            call(STR_APPEND, 12, &[10, 11]),
            drop_of(11),
        ]);
        let stats = rewrite_str_appends(&mut m);
        assert_eq!(stats.flattened, 0);
        assert_eq!(calls(&m, 0).len(), 4);
    }

    #[test]
    fn nested_right_operands_flatten_too() {
        // `(a + b) + (c + d)`
        let mut m = module_with(vec![
            concat(10, 1, 2),
            concat(11, 3, 4),
            concat(12, 10, 11),
            drop_of(10),
            drop_of(11),
            call(STR_APPEND, 13, &[5, 12]),
            drop_of(12),
        ]);
        let stats = rewrite_str_appends(&mut m);
        // the inner `(a + b) ++ (c + d)` is a sink of its own
        assert_eq!(stats.flattened, 2);
        let c = calls(&m, 0);
        let rights: Vec<u32> = c.iter().map(|x| x.2[1]).collect();
        assert_eq!(rights, vec![1, 2, 3, 4]);
        assert!(c.iter().all(|x| x.0 == STR_APPEND));
    }
}
