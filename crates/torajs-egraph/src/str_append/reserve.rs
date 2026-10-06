//! A loop accumulator that starts as `''` and never leaves its frame
//! starts as an empty cell with room instead.
//!
//! `let acc = ''; for (...) acc = acc + piece` reaches its final
//! length through the doubling ladder: the first append copies the
//! static `''` into a cell of its exact size, and every append that
//! outgrows the cell after that reallocates and copies again. The
//! early rungs are the costly ones — each holds only a few appends.
//! When the accumulator is seeded with `__torajs_str_alloc_capacity`
//! room for [`SEED_UNITS`] code units, the appends of a short string
//! never reallocate and a long one skips the rungs below that size.
//!
//! The price of the room is memory, and it is only free while the
//! cell dies with the loop's use of it. So the set of values the
//! accumulator flows through — the seed copies, every append result
//! and everything joined to them by `copy` — must have only those as
//! defs, and every use must be one of:
//!
//! * a `copy` into another value of the set;
//! * the left operand of a consuming append, whose result joins it;
//! * the right operand of an append, which copies its bytes;
//! * a `__torajs_str_drop`;
//! * a load through it, or an argument slot of a runtime reader that
//!   keeps nothing of the string.
//!
//! A store, a return, a call that may retain it (a push, a Map key, a
//! concat that may answer its operand) leaves the static seed alone:
//! a kept string would own the room however short it is. And some
//! append of the set must sit in a loop the seed does not, or there is
//! no ladder to skip.
//!
//! When every append of the set writes a UTF-16 literal the seed is
//! UTF-16 (`__torajs_str_alloc_capacity_utf16`): a Latin-1 one would
//! reallocate on the first append only to widen nothing.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{
    FuncId, Function, Inst, InstKind, Module, Operand, Type, ValueId, ValueInfo,
};

use crate::concat_num_fuse::stack::READERS;
use web::{Index, Web};

mod web;

const ALLOC_CAPACITY: &str = "__torajs_str_alloc_capacity";
const ALLOC_CAPACITY_UTF16: &str = "__torajs_str_alloc_capacity_utf16";
const APPEND_LIT: &str = "__torajs_str_append_lit";
const STR_DROP: &str = "__torajs_str_drop";
const APPENDS: [&str; 3] = [
    "__torajs_str_append",
    "__torajs_str_append_substr",
    APPEND_LIT,
];
/// Code units the seed holds before its first reallocation. At either
/// width that is a pool class (32 or 64 bytes), so the seed recycles
/// through the pool like the cells it replaces.
const SEED_UNITS: i64 = 32;

struct Kernels {
    drop: FuncId,
    appends: Vec<FuncId>,
    append_lit: Option<FuncId>,
    readers: HashMap<FuncId, &'static [usize]>,
}

/// Seed every qualifying accumulator; answers how many seed copies
/// now take a reserved cell.
pub(super) fn reserve_accumulators(module: &mut Module) -> u32 {
    let find = |m: &Module, name: &str| {
        let at = m.funcs.iter().position(|f| f.name == name)?;
        Some(FuncId(at as u32))
    };
    let Some(drop) = find(module, STR_DROP) else {
        return 0;
    };
    let appends: Vec<FuncId> = APPENDS.iter().filter_map(|n| find(module, n)).collect();
    if appends.is_empty() {
        return 0;
    }
    let readers = READERS
        .iter()
        .filter_map(|&(name, slots)| Some((find(module, name)?, slots)))
        .collect();
    let k = Kernels {
        drop,
        appends,
        append_lit: find(module, APPEND_LIT),
        readers,
    };
    let empty: Vec<bool> = module.strings.iter().map(|s| s.length == 0).collect();
    let plans: Vec<(usize, Vec<Seed>)> = module
        .funcs
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_declaration())
        .map(|(i, f)| (i, seeds(f, &k, &empty)))
        .filter(|(_, s)| !s.is_empty())
        .collect();
    if plans.is_empty() {
        return 0;
    }
    let mut declare = |name: &str| {
        find(module, name).unwrap_or_else(|| {
            let mut f = Function::new(name, Type::Str);
            f.add_param(Type::I64, "cap");
            module.funcs.push(f);
            FuncId(module.funcs.len() as u32 - 1)
        })
    };
    let wide = plans.iter().any(|(_, s)| s.iter().any(|s| s.utf16));
    let narrow = plans.iter().any(|(_, s)| s.iter().any(|s| !s.utf16));
    let alloc = narrow.then(|| declare(ALLOC_CAPACITY));
    let alloc_utf16 = wide.then(|| declare(ALLOC_CAPACITY_UTF16));
    let mut seeded = 0;
    for (fi, mut sites) in plans {
        let f = &mut module.funcs[fi];
        // back to front, so an insertion leaves the sites before it put
        sites.sort_unstable_by_key(|s| std::cmp::Reverse(s.at));
        for Seed { at: (b, i), utf16 } in sites {
            let (fid, bytes) = if utf16 {
                (alloc_utf16, SEED_UNITS * 2)
            } else {
                (alloc, SEED_UNITS)
            };
            let fid = fid.expect("declared for every seed kind planned");
            f.values.push(ValueInfo {
                ty: Type::Str,
                name: None,
            });
            let cell = ValueId(f.values.len() as u32 - 1);
            let insts = &mut f.blocks[b].insts;
            if let InstKind::Copy(_, src) = &mut insts[i].kind {
                *src = Operand::Value(cell);
            }
            let origin = insts[i].origin;
            insts.insert(
                i,
                Inst {
                    result: Some(cell),
                    kind: InstKind::Call(fid, vec![Operand::ConstI64(bytes)]),
                    origin,
                },
            );
            seeded += 1;
        }
    }
    seeded
}

/// A `copy` of a static `''` to seed with a reserved cell.
struct Seed {
    at: (usize, usize),
    utf16: bool,
}

/// Every `copy` of a static `''` that seeds a qualifying accumulator
/// in `f`.
fn seeds(f: &Function, k: &Kernels, empty: &[bool]) -> Vec<Seed> {
    let ix = Index::of(f, empty);
    let mut seen: HashSet<ValueId> = HashSet::default();
    let mut loops: Option<LoopAnalysis> = None;
    let mut out = Vec::new();
    for seed in f.blocks.iter().flat_map(|b| b.insts.iter()) {
        let (Some(r), InstKind::Copy(_, Operand::Value(src))) = (seed.result, &seed.kind) else {
            continue;
        };
        if !ix.empty_lits.contains(src) || seen.contains(&r) {
            continue;
        }
        let web = Web::grow(r, &ix, k);
        seen.extend(web.set.iter().copied());
        if !web.ok || web.appends.is_empty() {
            continue;
        }
        let la = loops.get_or_insert_with(|| LoopAnalysis::compute(f, &DominatorTree::compute(f)));
        let ladder = la.loops().iter().any(|l| {
            let inside = |blk: usize| l.contains(&f.blocks[blk].id);
            web.appends.iter().any(|&blk| inside(blk)) && web.seeds.iter().all(|&(b, _)| !inside(b))
        });
        if ladder {
            let utf16 = web.utf16;
            out.extend(web.seeds.into_iter().map(|at| Seed { at, utf16 }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use torajs_core::ssa::{BlockId, StringLiteral, Terminator};

    fn copy_of(result: u32, src: u32) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind: InstKind::Copy(Type::Str, v(src)),
            origin: None,
        }
    }

    /// `x`, Latin-1 and ASCII
    const NARROW: i64 = 0x1012;
    /// one UTF-16 code unit
    const WIDE: i64 = 0x10;

    fn append_lit(result: u32, acc: u32, meta: i64) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind: InstKind::Call(
                STR_APPEND_LIT,
                vec![v(acc), Operand::ConstI64(120), Operand::ConstI64(meta)],
            ),
            origin: None,
        }
    }

    fn length_of(result: u32, s: u32) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind: InstKind::Load(Type::I32, v(s), 8),
            origin: None,
        }
    }

    /// `let acc = ''; while (..) acc = acc + 'x'; <exit>` where the
    /// exit block holds `exit` and ends in `last`: bb0 seeds, bb1 is
    /// the loop header, bb2 the body, bb3 the exit.
    fn accumulator(exit: Vec<Inst>, last: Terminator) -> Module {
        accumulator_of(NARROW, exit, last)
    }

    fn accumulator_of(meta: i64, exit: Vec<Inst>, last: Terminator) -> Module {
        let mut m = module_with_blocks(
            vec![
                vec![static_str(1), copy_of(2, 1)],
                vec![],
                vec![append_lit(3, 2, meta), copy_of(2, 3)],
                exit,
            ],
            last,
        );
        let blocks = &mut m.funcs[BODY].blocks;
        blocks[1].term = Terminator::CondBr {
            cond: Operand::ConstBool(true),
            then_blk: BlockId(2),
            else_blk: BlockId(3),
        };
        blocks[2].term = Terminator::Br(BlockId(1));
        m.strings = vec![empty()];
        m
    }

    fn empty() -> StringLiteral {
        StringLiteral {
            bytes: vec![],
            is_latin1: true,
            length: 0,
        }
    }

    fn seeded(m: &Module) -> Option<FuncId> {
        match &m.funcs[BODY].blocks[0].insts[1].kind {
            InstKind::Call(fid, args)
                if args == &[Operand::ConstI64(SEED_UNITS)]
                    || args == &[Operand::ConstI64(SEED_UNITS * 2)] =>
            {
                Some(*fid)
            }
            _ => None,
        }
    }

    #[test]
    fn a_loop_accumulator_read_and_dropped_takes_a_reserved_seed() {
        let mut m = accumulator(vec![length_of(4, 2), drop_of(2)], Terminator::Ret(None));
        assert_eq!(reserve_accumulators(&mut m), 1);
        let alloc = seeded(&m).expect("the seed is a reserved cell");
        assert_eq!(m.funcs[alloc.0 as usize].name, ALLOC_CAPACITY);
        let seed = &m.funcs[BODY].blocks[0].insts[2];
        let cell = m.funcs[BODY].blocks[0].insts[1].result.unwrap();
        assert!(matches!(seed.kind, InstKind::Copy(_, Operand::Value(c)) if c == cell));
    }

    #[test]
    fn an_accumulator_of_utf16_literals_takes_a_utf16_seed() {
        let exit = vec![length_of(4, 2), drop_of(2)];
        let mut m = accumulator_of(WIDE, exit, Terminator::Ret(None));
        assert_eq!(reserve_accumulators(&mut m), 1);
        let alloc = seeded(&m).expect("the seed is a reserved cell");
        assert_eq!(m.funcs[alloc.0 as usize].name, ALLOC_CAPACITY_UTF16);
        assert!(
            m.funcs.iter().all(|f| f.name != ALLOC_CAPACITY),
            "only the kind used is declared"
        );
    }

    #[test]
    fn a_returned_accumulator_keeps_the_static_seed() {
        let mut m = accumulator(vec![], Terminator::Ret(Some(v(2))));
        assert_eq!(reserve_accumulators(&mut m), 0);
        assert!(seeded(&m).is_none());
    }

    #[test]
    fn an_accumulator_handed_to_an_unknown_call_keeps_the_static_seed() {
        // a push / store / any call not known to keep nothing
        let mut m = accumulator(
            vec![call(STR_CONCAT, 4, &[2, 1]), drop_of(2)],
            Terminator::Ret(None),
        );
        assert_eq!(reserve_accumulators(&mut m), 0);
    }

    #[test]
    fn an_append_outside_any_loop_keeps_the_static_seed() {
        let mut m = module_with_blocks(
            vec![vec![
                static_str(1),
                copy_of(2, 1),
                append_lit(3, 2, NARROW),
                drop_of(3),
            ]],
            Terminator::Ret(None),
        );
        m.strings = vec![empty()];
        assert_eq!(reserve_accumulators(&mut m), 0);
    }
}
