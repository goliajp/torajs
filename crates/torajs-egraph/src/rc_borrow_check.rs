//! The use audit and hazard check behind `rc_borrow`: which
//! instructions may touch a web's holders, and that no holder is live
//! across anything that could free the element it borrows.
//!
//! *Holders* are the web's members plus every value derived from one
//! by a bit-preserving op (`copy`, `ptrtoint`, `inttoptr`, masking
//! arithmetic, `select`) or read through one as a pointer or a box (a
//! `load ptr|any` past the count word, a `load_dyn ptr|any` off it):
//! such a value points into memory the element owns. A holder's
//! *needing* uses are the ones that read the element — a load through
//! it, a pure op on its bits, the wrapped length call. Retains,
//! releases and the bit derivations are not needing uses: a
//! derivation needs its source only where its own result is needed.
//! A read into a holder needs its source at the read, and from then
//! on its result is a holder of its own.
//!
//! Liveness is per value, backward, with that transfer. An
//! instruction is a hazard when a holder is live across it (live
//! after it and not defined by it) and the instruction is not
//! window-transparent: a non-readonly call, a heap store, a release
//! of anything. The origin that defines a holder is never "across"
//! its own value, so a `call` origin passes unless an older holder of
//! the same web is still needed past it.

use crate::id_hash::HashSet;

use torajs_core::ssa::{
    BinOp, Function, InstKind, Operand, Terminator, Type, ValueId, visit_value_operands,
};

use crate::optimize::classify_pure;
use crate::rc_borrow::Site;

pub(crate) struct Env {
    pub(crate) slots: HashSet<ValueId>,
    pub(crate) readonly: HashSet<u32>,
    pub(crate) length_get: Option<u32>,
}

pub(crate) struct Uses {
    /// `__torajs_any_length_get` calls on a member: wrapped.
    pub(crate) wraps: Vec<(Site, ValueId)>,
    /// `rc_dec_any` of a member.
    pub(crate) releases: Vec<(Site, ValueId)>,
}

/// What an instruction does to the holders it mentions.
#[derive(Clone)]
enum Use {
    /// Reads the element: needs every holder it mentions.
    Need(Vec<ValueId>),
    /// Defines a holder from these holders: needs them iff it is needed.
    Derive(Vec<ValueId>),
    /// Mentions no holder, or only retains / releases one.
    None,
}

/// Audit every use of the web's holders and prove no hazard sits
/// inside a borrowed lifetime; answers the wrapped calls and the
/// releases.
pub(crate) fn check_uses(func: &Function, members: &HashSet<ValueId>, env: &Env) -> Option<Uses> {
    let holders = holders_of(func, members);
    let mut uses = Uses {
        wraps: Vec::new(),
        releases: Vec::new(),
    };
    let mut classes: Vec<Vec<Use>> = Vec::with_capacity(func.blocks.len());
    for (b, block) in func.blocks.iter().enumerate() {
        let mut row = Vec::with_capacity(block.insts.len());
        for (i, inst) in block.insts.iter().enumerate() {
            let mut touched = Vec::new();
            visit_value_operands(&inst.kind, |v| {
                if holders.contains(&v) {
                    touched.push(v);
                }
            });
            row.push(if touched.is_empty() {
                Use::None
            } else {
                classify(func, (b, i), touched, members, &holders, env, &mut uses)?
            });
        }
        if let Terminator::Ret(Some(Operand::Value(v))) = &block.term
            && holders.contains(v)
        {
            return None;
        }
        classes.push(row);
    }
    no_hazard(func, &classes, &holders, env).then_some(uses)
}

/// Members and everything bit-derived from them, to a fixpoint.
fn holders_of(func: &Function, members: &HashSet<ValueId>) -> HashSet<ValueId> {
    let mut holders = members.clone();
    loop {
        let mut grew = false;
        for block in &func.blocks {
            for inst in &block.insts {
                if let Some(r) = inst.result
                    && !holders.contains(&r)
                    && (derives(&inst.kind, &holders) || reads_into(&inst.kind, &holders))
                {
                    holders.insert(r);
                    grew = true;
                }
            }
        }
        if !grew {
            return holders;
        }
    }
}

/// Does `kind` carry a holder's bits into its result?
fn derives(kind: &InstKind, holders: &HashSet<ValueId>) -> bool {
    let h = |op: &Operand| matches!(op, Operand::Value(v) if holders.contains(v));
    match kind {
        InstKind::Copy(_, s) | InstKind::PtrToInt(s) | InstKind::IntToPtr(s) => h(s),
        InstKind::BinOp(BinOp::And | BinOp::Or | BinOp::Add | BinOp::Sub | BinOp::Xor, a, b) => {
            h(a) || h(b)
        }
        InstKind::Select(_, _, a, b) => h(a) || h(b),
        _ => false,
    }
}

/// Does `kind` read a pointer or a box out of a holder's memory?
fn reads_into(kind: &InstKind, holders: &HashSet<ValueId>) -> bool {
    let h = |op: &Operand| matches!(op, Operand::Value(v) if holders.contains(v));
    match kind {
        InstKind::Load(Type::Ptr | Type::Any, a, off) => *off != 0 && h(a),
        InstKind::LoadDyn(Type::Ptr | Type::Any, a, _)
        | InstKind::LoadDynScaled8(Type::Ptr | Type::Any, a, _) => h(a),
        _ => false,
    }
}

fn classify(
    func: &Function,
    site: Site,
    touched: Vec<ValueId>,
    members: &HashSet<ValueId>,
    holders: &HashSet<ValueId>,
    env: &Env,
    uses: &mut Uses,
) -> Option<Use> {
    let inst = &func.blocks[site.0].insts[site.1];
    let member = |op: &Operand| match op {
        Operand::Value(m) if members.contains(m) => Some(*m),
        _ => None,
    };
    match &inst.kind {
        InstKind::RcIncAny(m) if member(m).is_some() => Some(Use::None),
        InstKind::RcDecAny(m) => {
            uses.releases.push((site, member(m)?));
            Some(Use::None)
        }
        InstKind::Load(_, Operand::Value(a), off) if holders.contains(a) => {
            // offset 0 is the count word
            (*off != 0).then_some(Use::Need(touched))
        }
        InstKind::Load(..)
        | InstKind::LoadDyn(..)
        | InstKind::LoadDynScaled8(..)
        | InstKind::LoadU8Dyn(..)
        | InstKind::LoadU16Dyn(..) => Some(Use::Need(touched)),
        InstKind::Call(f, args) if Some(f.0) == env.length_get => {
            let [m] = args.as_slice() else {
                return None;
            };
            uses.wraps.push((site, member(m)?));
            Some(Use::Need(touched))
        }
        InstKind::Copy(Type::Any, _) if inst.result.is_some_and(|r| members.contains(&r)) => {
            Some(Use::Derive(touched))
        }
        kind if derives(kind, holders) => Some(Use::Derive(touched)),
        InstKind::BoxAny(..) | InstKind::Identity(_) => None,
        kind if pure_op(kind) => Some(Use::Need(touched)),
        _ => None,
    }
}

/// `classify_pure` plus the bit casts it leaves out of GVN: no memory,
/// no count, no trap.
fn pure_op(kind: &InstKind) -> bool {
    classify_pure(kind)
        || matches!(
            kind,
            InstKind::PtrToInt(_)
                | InstKind::IntToPtr(_)
                | InstKind::BitCastF64ToI64(_)
                | InstKind::BitCastI64ToF64(_)
                | InstKind::TruncI64ToBool(_)
        )
}

/// Can `kind` run while a borrowed element must stay alive? True for
/// what provably neither writes a heap slot nor lowers a count.
fn window_ok(kind: &InstKind, env: &Env) -> bool {
    if pure_op(kind) {
        return true;
    }
    match kind {
        InstKind::Copy(..)
        | InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::Load(..)
        | InstKind::LoadDyn(..)
        | InstKind::LoadDynScaled8(..)
        | InstKind::LoadU8Dyn(..)
        | InstKind::LoadU16Dyn(..)
        | InstKind::GlobalRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::StringRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_)
        | InstKind::RcInc(_)
        | InstKind::RcIncAny(_)
        | InstKind::Pinned(_)
        | InstKind::ThrowSignal => true,
        InstKind::Store(_, Operand::Value(p), _) => env.slots.contains(p),
        InstKind::Call(f, _) => env.readonly.contains(&f.0),
        // the function parks: any code may run before it resumes
        InstKind::Suspend(..) => false,
        _ => false,
    }
}

/// Backward transfer of one instruction over the live holder set.
fn transfer(result: Option<ValueId>, class: &Use, live: &mut HashSet<ValueId>) {
    let needed = match result {
        Some(r) => live.remove(&r),
        None => false,
    };
    match class {
        Use::Need(srcs) => live.extend(srcs.iter().copied()),
        Use::Derive(srcs) if needed => live.extend(srcs.iter().copied()),
        Use::Derive(_) | Use::None => {}
    }
}

/// Per-value liveness of the holders to a fixpoint, then the sweep:
/// no holder live across a non-transparent instruction.
fn no_hazard(func: &Function, classes: &[Vec<Use>], holders: &HashSet<ValueId>, env: &Env) -> bool {
    let n = func.blocks.len();
    let mut live_in: Vec<HashSet<ValueId>> = vec![HashSet::default(); n];
    let live_out = |b: usize, live_in: &[HashSet<ValueId>]| {
        let mut out: HashSet<ValueId> = HashSet::default();
        for s in crate::throw_check_fold::successors(&func.blocks[b].term) {
            out.extend(live_in[s.0 as usize].iter().copied());
        }
        out
    };
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            let mut live = live_out(b, &live_in);
            for (inst, class) in func.blocks[b].insts.iter().zip(&classes[b]).rev() {
                transfer(inst.result, class, &mut live);
            }
            if live != live_in[b] {
                live_in[b] = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    (0..n).all(|b| {
        let mut live = live_out(b, &live_in);
        func.blocks[b]
            .insts
            .iter()
            .zip(&classes[b])
            .rev()
            .all(|(inst, class)| {
                let across = live
                    .iter()
                    .any(|v| holders.contains(v) && inst.result != Some(*v));
                let ok = !across || window_ok(&inst.kind, env);
                transfer(inst.result, class, &mut live);
                ok
            })
    })
}
