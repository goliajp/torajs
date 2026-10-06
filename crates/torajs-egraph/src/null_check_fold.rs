//! Nullish-check folding — drop the `__torajs_str_null_check` calls
//! whose value an earlier check on every path already passed.
//!
//! Lowering guards each member read off a string that may be holding
//! a nullish spelling:
//!
//! ```text
//! call __torajs_str_null_check(%s)
//! <probe of the pending-exception flag> → <propagate>, <continue>
//! ```
//!
//! The check reads nothing but the pointer's bits and writes nothing
//! when it passes, so on `<continue>` the value is known live and
//! stays so: a second check of the same value answers the same. A
//! scanner loop checked its source twice per character, once for
//! `.length` and once for `charCodeAt`.
//!
//! Forward must-analysis over the set of passed values: a check's
//! `<continue>` edge adds its value, a join keeps what every
//! predecessor has, an instruction defining the value again (a merged
//! loop cell) removes it, and the entry has nothing. A check reached
//! with its value in the set is deleted; its probe stays for
//! `throw_check_fold`, which runs next.
//!
//! A check that opens a loop header, on a value the loop does not
//! define, is not covered on the way in. It gets a copy on every edge
//! into the loop, branching to the header check's own propagate path,
//! and the header check then folds like any other. The loop ran the
//! header check before anything else on its first iteration, so the
//! copy raises the same error at the same point.

mod loop_guard;
mod str_reads;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};
use torajs_core::ssa::{BlockId, FuncId, Function, Inst, InstKind, Module, Operand, ValueId};

use crate::throw_check_fold::{
    insert_entry_probe, is_clobber, probe_of, propagate_path_reads, successors,
};

const STR_NULL_CHECK: &str = "__torajs_str_null_check";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NullCheckFoldStats {
    pub checks: u32,
    pub folded: u32,
    pub loop_entry_checks: u32,
}

/// The inline guards a loop repeats, then the call-form checks.
/// `TORAJS_LOOP_GUARD_OFF=1` / `TORAJS_NULL_CHECK_FOLD_OFF=1` skip
/// either (bisect gates).
pub(crate) fn run(module: &mut Module) {
    crate::gated_pass("LOOP_GUARD", module, loop_guard::hoist_loop_guards);
    crate::gated_pass("NULL_CHECK_FOLD", module, fold_null_checks);
}

pub fn fold_null_checks(module: &mut Module) -> NullCheckFoldStats {
    let mut stats = NullCheckFoldStats::default();
    let Some(check) = module.funcs.iter().position(|f| f.name == STR_NULL_CHECK) else {
        return stats;
    };
    let check = FuncId(check as u32);
    for func in module.funcs.iter_mut() {
        fold_in_function(func, check, &mut stats);
    }
    stats
}

/// A block whose check of `value` at `call` is answered by the probe
/// ending it: `passed` is entered only when the check passed.
struct Site {
    call: usize,
    value: ValueId,
    raised: BlockId,
    passed: BlockId,
}

fn fold_in_function(func: &mut Function, check: FuncId, stats: &mut NullCheckFoldStats) {
    let mut first_round = true;
    let mut guarded: HashSet<BlockId> = HashSet::default();
    loop {
        let sites = sites_of(func, check);
        if first_round {
            stats.checks += sites.len() as u32;
            first_round = false;
        }
        if sites.is_empty() {
            return;
        }
        let passed = passed_on_entry(func, &sites);
        for (&b, site) in &sites {
            let insts = &mut func.blocks[b].insts;
            if passed[b].contains(&site.value) && !defines(&insts[..site.call], site.value) {
                insts.remove(site.call);
                stats.folded += 1;
            }
        }
        let sites = sites_of(func, check);
        let Some((lp, b)) = loop_to_guard(func, &sites, &guarded) else {
            return;
        };
        let site = &sites[&b];
        let (value, raised) = (site.value, site.raised);
        let entry = insert_entry_probe(func, &lp, raised);
        func.blocks[entry.0 as usize].insts.insert(
            0,
            Inst {
                result: None,
                kind: InstKind::Call(check, vec![Operand::Value(value)]),
                origin: None,
            },
        );
        guarded.insert(lp.header);
        stats.loop_entry_checks += 1;
    }
}

fn defines(insts: &[Inst], v: ValueId) -> bool {
    insts.iter().any(|i| i.result == Some(v))
}

fn sites_of(func: &Function, check: FuncId) -> HashMap<usize, Site> {
    (0..func.blocks.len())
        .filter_map(|b| site_of(func, b, check).map(|s| (b, s)))
        .collect()
}

/// The check the probe ending block `b` answers: the last one before
/// the flag load, with nothing between them that could change the
/// flag or the value.
fn site_of(func: &Function, b: usize, check: FuncId) -> Option<Site> {
    let probe = probe_of(func, b)?;
    let insts = &func.blocks[b].insts[..probe.load];
    let (call, value) =
        insts
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, inst)| match (&inst.kind, inst.result) {
                (InstKind::Call(f, args), None) if *f == check => match args.as_slice() {
                    [Operand::Value(v)] => Some((i, *v)),
                    _ => None,
                },
                _ => None,
            })?;
    let between = &insts[call + 1..];
    if between.iter().any(|i| is_clobber(&i.kind)) || defines(&func.blocks[b].insts[call..], value)
    {
        return None;
    }
    Some(Site {
        call,
        value,
        raised: probe.then_blk,
        passed: probe.else_blk,
    })
}

/// Per block: the values whose check has passed on every path into it.
/// Optimistic start (every block but the entry holds every checked
/// value), lowered to the greatest fixpoint.
fn passed_on_entry(func: &Function, sites: &HashMap<usize, Site>) -> Vec<HashSet<ValueId>> {
    let n = func.blocks.len();
    let universe: HashSet<ValueId> = sites.values().map(|s| s.value).collect();
    let mut passed = vec![universe.clone(); n];
    passed[0].clear();
    loop {
        let mut changed = false;
        for b in 0..n {
            let block = &func.blocks[b];
            let mut at_end = passed[b].clone();
            for inst in &block.insts {
                if let Some(r) = inst.result {
                    at_end.remove(&r);
                }
            }
            let site = sites.get(&b);
            for succ in successors(&block.term) {
                let s = succ.0 as usize;
                let gen_here = site
                    .filter(|site| site.passed == succ)
                    .map(|site| site.value);
                let before = passed[s].len();
                passed[s].retain(|v| at_end.contains(v) || gen_here == Some(*v));
                changed |= passed[s].len() != before;
            }
        }
        if !changed {
            return passed;
        }
    }
}

/// A loop whose header opens with a check of a value the loop does not
/// define, and whose propagate path runs the same from outside it.
fn loop_to_guard(
    func: &Function,
    sites: &HashMap<usize, Site>,
    guarded: &HashSet<BlockId>,
) -> Option<(NaturalLoop, usize)> {
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    loops.loops().iter().find_map(|lp| {
        let h = lp.header.0 as usize;
        if h == 0 || guarded.contains(&lp.header) {
            return None;
        }
        let site = sites.get(&h).filter(|s| s.call == 0)?;
        let body: HashSet<usize> = lp.body.iter().map(|b| b.0 as usize).collect();
        let redefined = body
            .iter()
            .any(|&b| defines(&func.blocks[b].insts, site.value));
        (!redefined
            && propagate_path_reads(func, &body, site.raised).is_some_and(|read| read.is_empty()))
        .then(|| (lp.clone(), h))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{IPred, THROW_ACTIVE_SYM, Terminator, Type};

    const CHECK: FuncId = FuncId(0);

    /// Append `check(v)` and its probe to `b`.
    fn check(f: &mut Function, b: BlockId, v: ValueId, raised: BlockId, passed: BlockId) {
        f.append_void(b, InstKind::Call(CHECK, vec![Operand::Value(v)]));
        let p = f.append_inst(
            b,
            InstKind::GlobalRef(THROW_ACTIVE_SYM.into()),
            Type::Ptr,
            None,
        );
        let l = f.append_inst(
            b,
            InstKind::Load(Type::I64, Operand::Value(p), 0),
            Type::I64,
            None,
        );
        let c = f.append_inst(
            b,
            InstKind::ICmp(IPred::Ne, Operand::Value(l), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        f.set_term(
            b,
            Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk: raised,
                else_blk: passed,
            },
        );
    }

    fn run(f: &mut Function) -> NullCheckFoldStats {
        let mut stats = NullCheckFoldStats::default();
        fold_in_function(f, CHECK, &mut stats);
        stats
    }

    fn checks_left(f: &Function) -> usize {
        f.blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| matches!(&i.kind, InstKind::Call(c, _) if *c == CHECK))
            .count()
    }

    fn ret(f: &mut Function, b: BlockId, k: i64) {
        f.set_term(b, Terminator::Ret(Some(Operand::ConstI64(k))));
    }

    /// entry: check(a) → (throw0, mid) · mid: check(second) → (throw1, exit)
    fn two_checks(same_value: bool) -> Function {
        let mut f = Function::new("t", Type::I64);
        let a = f.add_param(Type::Str, "a");
        let b = f.add_param(Type::Str, "b");
        let entry = f.add_block();
        let throw0 = f.add_block();
        let mid = f.add_block();
        let throw1 = f.add_block();
        let exit = f.add_block();
        check(&mut f, entry, a, throw0, mid);
        check(&mut f, mid, if same_value { a } else { b }, throw1, exit);
        ret(&mut f, throw0, 0);
        ret(&mut f, throw1, 0);
        ret(&mut f, exit, 1);
        f
    }

    #[test]
    fn a_second_check_of_a_passed_value_folds() {
        let mut f = two_checks(true);
        let stats = run(&mut f);
        assert_eq!((stats.checks, stats.folded), (2, 1));
        assert_eq!(checks_left(&f), 1);
    }

    #[test]
    fn a_check_of_another_value_stays() {
        let mut f = two_checks(false);
        assert_eq!(run(&mut f).folded, 0);
        assert_eq!(checks_left(&f), 2);
    }

    #[test]
    fn a_check_on_the_raised_edge_stays() {
        // entry: check(a) → (again, exit) · again: check(a) → (throw, exit)
        let mut f = Function::new("t", Type::I64);
        let a = f.add_param(Type::Str, "a");
        let entry = f.add_block();
        let again = f.add_block();
        let throw = f.add_block();
        let exit = f.add_block();
        check(&mut f, entry, a, again, exit);
        check(&mut f, again, a, throw, exit);
        ret(&mut f, throw, 0);
        ret(&mut f, exit, 1);
        assert_eq!(run(&mut f).folded, 0);
    }

    #[test]
    fn a_check_passed_on_one_arm_only_stays() {
        // entry → (left, join) · left: check(a) → (throw0, join)
        // join: check(a) → (throw1, exit)
        let mut f = Function::new("t", Type::I64);
        let a = f.add_param(Type::Str, "a");
        let entry = f.add_block();
        let left = f.add_block();
        let throw0 = f.add_block();
        let join = f.add_block();
        let throw1 = f.add_block();
        let exit = f.add_block();
        f.set_term(
            entry,
            Terminator::CondBr {
                cond: Operand::ConstBool(true),
                then_blk: left,
                else_blk: join,
            },
        );
        check(&mut f, left, a, throw0, join);
        check(&mut f, join, a, throw1, exit);
        ret(&mut f, throw0, 0);
        ret(&mut f, throw1, 0);
        ret(&mut f, exit, 1);
        assert_eq!(run(&mut f).folded, 0);
    }

    #[test]
    fn a_value_defined_again_is_checked_again() {
        // entry: v = copy a; check(v) → (throw0, mid)
        // mid: v = copy b; check(v) → (throw1, exit)
        let mut f = Function::new("t", Type::I64);
        let a = f.add_param(Type::Str, "a");
        let b = f.add_param(Type::Str, "b");
        let entry = f.add_block();
        let throw0 = f.add_block();
        let mid = f.add_block();
        let throw1 = f.add_block();
        let exit = f.add_block();
        let v = f.append_inst(
            entry,
            InstKind::Copy(Type::Str, Operand::Value(a)),
            Type::Str,
            None,
        );
        check(&mut f, entry, v, throw0, mid);
        f.blocks[mid.0 as usize].insts.push(Inst {
            result: Some(v),
            kind: InstKind::Copy(Type::Str, Operand::Value(b)),
            origin: None,
        });
        check(&mut f, mid, v, throw1, exit);
        ret(&mut f, throw0, 0);
        ret(&mut f, throw1, 0);
        ret(&mut f, exit, 1);
        assert_eq!(run(&mut f).folded, 0);
        assert_eq!(checks_left(&f), 2);
    }

    /// entry → head · head: check(a) → (throw, body) · body → (head, exit)
    /// with `body` optionally checking `a` again before it branches.
    fn scanner_loop(throw_reads_loop_value: bool) -> Function {
        let mut f = Function::new("t", Type::I64);
        let a = f.add_param(Type::Str, "a");
        let entry = f.add_block();
        let head = f.add_block();
        let throw = f.add_block();
        let body = f.add_block();
        let throw1 = f.add_block();
        let latch = f.add_block();
        let exit = f.add_block();
        f.set_term(entry, Terminator::Br(head));
        check(&mut f, head, a, throw, body);
        check(&mut f, body, a, throw1, latch);
        let n = f.append_inst(
            latch,
            InstKind::Load(Type::I64, Operand::Value(a), 8),
            Type::I64,
            None,
        );
        f.set_term(
            latch,
            Terminator::CondBr {
                cond: Operand::ConstBool(true),
                then_blk: head,
                else_blk: exit,
            },
        );
        if throw_reads_loop_value {
            f.set_term(throw, Terminator::Ret(Some(Operand::Value(n))));
        } else {
            ret(&mut f, throw, 0);
        }
        ret(&mut f, throw1, 0);
        ret(&mut f, exit, 1);
        f
    }

    #[test]
    fn a_loop_checks_its_source_once_on_the_way_in() {
        let mut f = scanner_loop(false);
        let stats = run(&mut f);
        assert_eq!((stats.loop_entry_checks, stats.folded), (1, 2));
        assert_eq!(checks_left(&f), 1);
        let head = &f.blocks[1];
        assert!(
            !head
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::Call(..)))
        );
    }

    #[test]
    fn a_propagate_path_reading_a_loop_value_keeps_the_header_check() {
        let mut f = scanner_loop(true);
        let stats = run(&mut f);
        assert_eq!((stats.loop_entry_checks, stats.folded), (0, 1));
        assert_eq!(checks_left(&f), 1);
        assert!(matches!(f.blocks[1].insts[0].kind, InstKind::Call(..)));
    }
}
