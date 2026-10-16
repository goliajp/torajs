//! Index checks a string's known length already answers.
//!
//! `s.charCodeAt(i & 15)` reads code unit `i & 15` only after
//! comparing it with the length, and answers NaN past the end:
//!
//! ```text
//! %k = and %i, 15
//! %l = load i32, %s +8
//! %z = zext_i32 %l
//! %c = icmp ult %k, %z
//! cond_br %c, read, nan
//! ```
//!
//! When `%s` is a literal, or a concat whose left side is one, its
//! length is at least the literal's: a concat joins the two code unit
//! sequences (ECMA-262 §13.15.3), a NULL side reads as the text
//! "null", and past the longest string the kernel raises a RangeError
//! and answers its left operand. An index whose largest value is
//! below that length is always inside, so the branch goes one way and
//! the NaN arm is gone. That arm is what kept the loop's sum from
//! being an integer: with it, float demotion sees a NaN flowing into
//! the accumulator.
//!
//! Runs before int_entry and the interval analysis, so both see the
//! loop without the arm. `TORAJS_STR_LEN_FOLD_OFF=1` skips,
//! `TORAJS_STR_LEN_FOLD_STATS=1` counts.

#[cfg(test)]
mod tests;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, FuncId, Function, IPred, InstKind, Module, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use crate::branch_fold::sweep_unreachable_blocks;

/// Offset of the u32 length in a Str header.
const STR_LEN_OFF: u64 = 8;
/// How many copies and concats deep a length is followed.
const MAX_DEPTH: u32 = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StrLenFoldStats {
    /// Index compares the length decided.
    pub folded: u32,
    /// Blocks only the folded-away arms reached.
    pub blocks_removed: u32,
}

pub fn fold_checks(module: &mut Module) -> StrLenFoldStats {
    let mut stats = StrLenFoldStats::default();
    let concat = module
        .funcs
        .iter()
        .position(|f| f.name == "__torajs_str_concat")
        .map(|i| FuncId(i as u32));
    let lens: Vec<u32> = module.strings.iter().map(|s| s.length).collect();
    for func in module.funcs.iter_mut() {
        if !func.is_declaration() {
            fold_fn(func, &lens, concat, &mut stats);
        }
    }
    stats
}

struct Defs<'a> {
    /// Per value id, its def when it has exactly one.
    single: Vec<Option<&'a InstKind>>,
    lens: &'a [u32],
    concat: Option<FuncId>,
}

impl<'a> Defs<'a> {
    fn def(&self, v: ValueId) -> Option<&'a InstKind> {
        self.single.get(v.0 as usize).copied().flatten()
    }

    /// The fewest code units the Str value `v` can hold.
    fn min_len(&self, v: ValueId, depth: u32) -> u32 {
        if depth > MAX_DEPTH {
            return 0;
        }
        match self.def(v) {
            Some(InstKind::StaticStrRef(id)) => self.lens.get(id.0 as usize).copied().unwrap_or(0),
            Some(InstKind::Copy(Type::Str, Operand::Value(x))) => self.min_len(*x, depth + 1),
            Some(InstKind::Call(f, args)) if Some(*f) == self.concat => match args.first() {
                Some(Operand::Value(a)) => self.min_len(*a, depth + 1),
                _ => 0,
            },
            _ => 0,
        }
    }

    /// The fewest code units a `zext_i32 (load i32, %s +8)` reads.
    fn len_floor(&self, op: &Operand) -> u32 {
        let Operand::Value(z) = op else { return 0 };
        let Some(InstKind::ZExtI32ToI64(Operand::Value(l))) = self.def(*z) else {
            return 0;
        };
        match self.def(*l) {
            Some(InstKind::Load(Type::I32, Operand::Value(s), STR_LEN_OFF)) => self.min_len(*s, 0),
            _ => 0,
        }
    }

    /// The widening and the length read a decided compare reads, in
    /// that order.
    fn len_chain(&self, compare: &InstKind) -> Vec<ValueId> {
        let InstKind::ICmp(_, _, Operand::Value(z)) = compare else {
            return Vec::new();
        };
        match self.def(*z) {
            Some(InstKind::ZExtI32ToI64(Operand::Value(l))) => vec![*z, *l],
            _ => Vec::new(),
        }
    }

    /// The largest value an index can take, when it is never negative.
    fn index_max(&self, op: &Operand) -> Option<i64> {
        let mask = |o: &Operand| match o {
            Operand::ConstI64(c) if *c >= 0 => Some(*c),
            _ => None,
        };
        match op {
            Operand::ConstI64(_) => mask(op),
            Operand::Value(v) => match self.def(*v) {
                Some(InstKind::BinOp(BinOp::And, a, b)) => mask(a).or_else(|| mask(b)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The answer of an unsigned index-against-length compare, when
    /// the length decides it.
    fn decide(&self, kind: &InstKind) -> Option<bool> {
        let InstKind::ICmp(pred, l, r) = kind else {
            return None;
        };
        let (index, len, inside) = match pred {
            IPred::Ult => (l, r, true),
            IPred::Uge => (l, r, false),
            _ => return None,
        };
        let max = self.index_max(index)?;
        (max < i64::from(self.len_floor(len))).then_some(inside)
    }
}

/// A Str length read and an unsigned compare, the two halves of a
/// code unit read's index check; most functions have neither.
fn has_index_check(func: &Function) -> bool {
    let (mut len_read, mut compare) = (false, false);
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        match inst.kind {
            InstKind::Load(Type::I32, _, STR_LEN_OFF) => len_read = true,
            InstKind::ICmp(IPred::Ult | IPred::Uge, ..) => compare = true,
            _ => continue,
        }
        if len_read && compare {
            return true;
        }
    }
    false
}

fn fold_fn(func: &mut Function, lens: &[u32], concat: Option<FuncId>, stats: &mut StrLenFoldStats) {
    if !has_index_check(func) {
        return;
    }
    let mut decided: HashMap<ValueId, bool> = HashMap::default();
    let mut feeds: Vec<ValueId> = Vec::new();
    {
        // a def count per value: 0 none, 1 single, 2 more
        let mut count: Vec<u8> = vec![0; func.values.len()];
        let mut single: Vec<Option<&InstKind>> = vec![None; func.values.len()];
        let mut compares: Vec<(ValueId, &InstKind)> = Vec::new();
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            if let Some(r) = inst.result {
                let i = r.0 as usize;
                count[i] = count[i].saturating_add(1);
                single[i] = (count[i] == 1).then_some(&inst.kind);
                if matches!(inst.kind, InstKind::ICmp(IPred::Ult | IPred::Uge, ..)) {
                    compares.push((r, &inst.kind));
                }
            }
        }
        let defs = Defs {
            single,
            lens,
            concat,
        };
        for (v, kind) in compares {
            if defs.def(v).is_none() {
                continue;
            }
            if let Some(answer) = defs.decide(kind) {
                decided.insert(v, answer);
                feeds.extend(defs.len_chain(kind));
            }
        }
    }
    if decided.is_empty() {
        return;
    }
    // only a compare nothing but branches read goes with its branches
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        visit_value_operands(&inst.kind, |v| {
            decided.remove(&v);
        });
    }
    for block in &func.blocks {
        if let Terminator::Ret(Some(Operand::Value(v))) = &block.term {
            decided.remove(v);
        }
    }
    let mut folded = 0;
    for block in func.blocks.iter_mut() {
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = &block.term
            && let Some(&inside) = decided.get(c)
        {
            block.term = Terminator::Br(if inside { *then_blk } else { *else_blk });
            folded += 1;
        }
    }
    if folded == 0 {
        return;
    }
    stats.folded += folded;
    stats.blocks_removed += sweep_unreachable_blocks(func);
    drop_unread(func, decided.into_keys().collect(), feeds);
}

/// The folded compares, then the length reads and widenings that fed
/// them, each once nothing else reads it (`feeds` lists a widening
/// before the read under it).
fn drop_unread(func: &mut Function, mut dead: HashSet<ValueId>, feeds: Vec<ValueId>) {
    let mut uses: HashMap<ValueId, u32> = feeds.iter().map(|v| (*v, 0)).collect();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if !inst.result.is_some_and(|r| dead.contains(&r)) {
            visit_value_operands(&inst.kind, |v| {
                if let Some(n) = uses.get_mut(&v) {
                    *n += 1;
                }
            });
        }
    }
    for block in &func.blocks {
        if let Terminator::Ret(Some(Operand::Value(v)))
        | Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } = &block.term
            && let Some(n) = uses.get_mut(v)
        {
            *n += 1;
        }
    }
    // a widening that goes stops counting as a read of the load under it
    let mut gone: Vec<ValueId> = Vec::new();
    for v in feeds {
        if uses[&v] > 0 || dead.contains(&v) {
            continue;
        }
        dead.insert(v);
        gone.push(v);
    }
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if inst.result.is_some_and(|r| gone.contains(&r)) {
            visit_value_operands(&inst.kind, |v| {
                if let Some(n) = uses.get_mut(&v) {
                    *n -= 1;
                    if *n == 0 {
                        dead.insert(v);
                    }
                }
            });
        }
    }
    for block in func.blocks.iter_mut() {
        block
            .insts
            .retain(|inst| !inst.result.is_some_and(|r| dead.contains(&r)));
    }
}
