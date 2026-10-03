//! Number strings that only live to be read go in the caller's frame.
//!
//! `const s = 'lit' + i` followed by `s.match(re)` / `s.length` and
//! the scope-end drop allocates a cell, fills it, reads it once and
//! frees it. When nothing can keep the cell, the bytes can live in a
//! fixed frame slot instead: the mint becomes
//! `__torajs_str_concat_i64_into(lit, n, buf)` /
//! `__torajs_i64_to_str_into(n, buf)` over an `AllocaBytes` slot sized
//! for the longest result, the cell's header is static (so any rc op
//! or release that still reaches it is a no-op), and every drop of it
//! is deleted.
//!
//! What makes that sound is the set of values the cell's address can
//! reach — the mint results plus everything joined to them by `copy`
//! (the loop-carried and versioned-loop forms). Every value in it must
//! have only mints and copies of each other as defs, and every use
//! must be one of:
//!
//! * a `copy` into another value of the set;
//! * a `__torajs_str_drop` of it, which is deleted;
//! * a load through it, or an argument slot of a runtime reader that
//!   is known not to keep the string (no cache, no view, no result
//!   that points into it), of a value whose bytes are known to be its
//!   own: either its one def is the mint (the slot then always holds
//!   that def's latest result, which is the value the name denotes),
//!   or its latest def before the read in the same block is a mint.
//!   A value that took its address by `copy` is never read: the slot
//!   may have been minted again since.
//!
//! Anything else — a store, a return, a branch on it, an rc op, a
//! call that may retain it (`match` / `exec` results hold views into
//! their subject, a Map keeps its key) — leaves the whole set on the
//! heap. The prefix must be a static literal so the slot's size is
//! known at compile time; there is no heap fallback inside the kernel.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    FuncId, Function, Inst, InstKind, Module, Operand, Type, ValueId, ValueInfo,
};

use web::{Index, Role, Use, mint_of, role, web};

mod web;

const I64_TO_STR: &str = "__torajs_i64_to_str";
const CONCAT_I64: &str = "__torajs_str_concat_i64";
const STR_DROP: &str = "__torajs_str_drop";
const I64_TO_STR_INTO: &str = "__torajs_i64_to_str_into";
const CONCAT_I64_INTO: &str = "__torajs_str_concat_i64_into";

const HDR_BYTES: u64 = 16;
/// `-9223372036854775808`
const I64_DIGITS_MAX: u64 = 20;
/// The most frame one temp may take.
const CELL_CAP: u64 = 256;

/// Runtime readers and the argument slots in which a string is only
/// read: each scans the bytes and answers a number or a bool.
pub(crate) const READERS: &[(&str, &[usize])] = &[
    ("__torajs_str_match_len", &[0]),
    ("__torajs_regex_test", &[1]),
    ("__torajs_str_eq", &[0, 1]),
    ("__torajs_str_char_code_at", &[0]),
    ("__torajs_str_starts_with", &[0, 1]),
    ("__torajs_str_ends_with", &[0, 1]),
    ("__torajs_str_includes", &[0, 1]),
    ("__torajs_str_index_of", &[0, 1]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mint {
    ToStr,
    Concat,
}

struct Kernels {
    drop: FuncId,
    to_str: Option<FuncId>,
    concat: Option<FuncId>,
    readers: HashMap<FuncId, &'static [usize]>,
    /// Frame bytes for a `concat_i64` cell over each static literal,
    /// `None` past [`CELL_CAP`].
    lit_cells: Vec<Option<u64>>,
}

#[derive(Default)]
struct Plan {
    /// `(block, inst, slot bytes, kind)`
    mints: Vec<(usize, usize, u64, Mint)>,
    /// `(block, inst)`
    drops: Vec<(usize, usize)>,
}

/// Move every qualifying temp into its frame; answers how many mint
/// sites moved.
pub(crate) fn stack_temps(module: &mut Module) -> u32 {
    let Some(k) = kernels(module) else {
        return 0;
    };
    let plans: Vec<(usize, Plan)> = module
        .funcs
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_declaration())
        .map(|(i, f)| (i, plan(f, &k)))
        .filter(|(_, p)| !p.mints.is_empty())
        .collect();
    if plans.is_empty() {
        return 0;
    }
    let to_str_into = declare(module, I64_TO_STR_INTO, &[Type::I64, Type::Ptr]);
    let concat_into = declare(module, CONCAT_I64_INTO, &[Type::Str, Type::I64, Type::Ptr]);
    let mut moved = 0;
    for (i, p) in plans {
        moved += p.mints.len() as u32;
        apply(&mut module.funcs[i], p, |m| match m {
            Mint::ToStr => to_str_into,
            Mint::Concat => concat_into,
        });
    }
    moved
}

fn find(module: &Module, name: &str) -> Option<FuncId> {
    let at = module.funcs.iter().position(|f| f.name == name)?;
    Some(FuncId(at as u32))
}

fn kernels(module: &Module) -> Option<Kernels> {
    let drop = find(module, STR_DROP)?;
    let to_str = find(module, I64_TO_STR);
    let concat = find(module, CONCAT_I64);
    if to_str.is_none() && concat.is_none() {
        return None;
    }
    let readers = READERS
        .iter()
        .filter_map(|&(name, slots)| Some((find(module, name)?, slots)))
        .collect();
    let lit_cells = module
        .strings
        .iter()
        .map(|lit| {
            let unit = if lit.is_latin1 { 1 } else { 2 };
            let bytes = HDR_BYTES + (u64::from(lit.length) + I64_DIGITS_MAX) * unit;
            let bytes = bytes.next_multiple_of(8);
            (bytes <= CELL_CAP).then_some(bytes)
        })
        .collect();
    Some(Kernels {
        drop,
        to_str,
        concat,
        readers,
        lit_cells,
    })
}

fn declare(module: &mut Module, name: &str, params: &[Type]) -> FuncId {
    if let Some(fid) = find(module, name) {
        return fid;
    }
    let mut f = Function::new(name, Type::Str);
    for (i, &t) in params.iter().enumerate() {
        f.add_param(t, &format!("a{i}"));
    }
    module.funcs.push(f);
    FuncId(module.funcs.len() as u32 - 1)
}

/// Frame bytes one function may give its number strings: a large
/// function with many read-only sites keeps the rest on the heap
/// rather than growing every frame of a deep recursion.
const FRAME_BUDGET: u64 = 2048;

fn plan(f: &Function, k: &Kernels) -> Plan {
    let ix = Index::of(f);
    let mut seen: HashSet<ValueId> = HashSet::new();
    let mut p = Plan::default();
    let mut frame_bytes = 0u64;
    for block in &f.blocks {
        for inst in &block.insts {
            let Some(r) = inst.result else { continue };
            if seen.contains(&r) || mint_of(inst, k, &ix).is_none() {
                continue;
            }
            let (set, ok) = web(r, k, &ix);
            seen.extend(set.iter().copied());
            if !ok {
                continue;
            }
            let mut mints = Vec::new();
            let mut drops = Vec::new();
            for v in &set {
                for &(b, i) in ix.defs.get(v).map_or(&[][..], |d| d.as_slice()) {
                    if let Some((bytes, m)) = mint_of(ix.inst(b, i), k, &ix) {
                        mints.push((b, i, bytes, m));
                    }
                }
                for &u in ix.uses.get(v).map_or(&[][..], |u| u.as_slice()) {
                    if let Use::Inst(b, j) = u
                        && matches!(role(ix.inst(b, j), *v, k), Role::Drop)
                    {
                        drops.push((b, j));
                    }
                }
            }
            let bytes: u64 = mints.iter().map(|m| m.2).sum();
            if frame_bytes + bytes > FRAME_BUDGET {
                continue;
            }
            frame_bytes += bytes;
            p.mints.extend(mints);
            p.drops.extend(drops);
        }
    }
    p
}

/// `(block, inst, the slot for a mint or None for a drop)`
type Edit = (usize, usize, Option<(u64, Mint)>);

fn apply(f: &mut Function, p: Plan, into: impl Fn(Mint) -> FuncId) {
    // per block, edits from the highest index down so earlier ones
    // keep their positions
    let mut edits: Vec<Edit> = p
        .mints
        .into_iter()
        .map(|(b, i, bytes, m)| (b, i, Some((bytes, m))))
        .chain(p.drops.into_iter().map(|(b, i)| (b, i, None)))
        .collect();
    edits.sort_by_key(|e| std::cmp::Reverse((e.0, e.1)));
    edits.dedup_by_key(|e| (e.0, e.1));
    for (b, i, edit) in edits {
        let Some((bytes, m)) = edit else {
            f.blocks[b].insts.remove(i);
            continue;
        };
        let buf = ValueId(f.values.len() as u32);
        f.values.push(ValueInfo {
            ty: Type::Ptr,
            name: None,
        });
        let call = &mut f.blocks[b].insts[i];
        let origin = call.origin;
        let InstKind::Call(fid, args) = &mut call.kind else {
            unreachable!("a planned mint is a call");
        };
        *fid = into(m);
        args.push(Operand::Value(buf));
        f.blocks[b].insts.insert(
            i,
            Inst {
                result: Some(buf),
                kind: InstKind::AllocaBytes(bytes),
                origin,
            },
        );
    }
}

#[cfg(test)]
mod tests;
