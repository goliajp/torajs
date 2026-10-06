//! The set of values one `''`-seeded accumulator flows through, with
//! every def and use checked against the rules in the parent module's
//! doc.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    Function, Inst, InstKind, Operand, Terminator, ValueId, visit_value_operands,
};

use super::Kernels;
use crate::str_append::literal::STR_FLAG_IS_LATIN1;

/// Defs and uses of every value in one function, and which values
/// are the static `''`.
pub(super) struct Index<'f> {
    pub(super) f: &'f Function,
    defs: HashMap<ValueId, Vec<(usize, usize)>>,
    /// `None` is a terminator use
    uses: HashMap<ValueId, Vec<Option<(usize, usize)>>>,
    pub(super) empty_lits: HashSet<ValueId>,
}

impl<'f> Index<'f> {
    pub(super) fn of(f: &'f Function, empty: &[bool]) -> Self {
        let mut defs: HashMap<ValueId, Vec<(usize, usize)>> = HashMap::default();
        let mut uses: HashMap<ValueId, Vec<Option<(usize, usize)>>> = HashMap::default();
        let mut lits = HashSet::default();
        for (b, block) in f.blocks.iter().enumerate() {
            for (i, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    defs.entry(r).or_default().push((b, i));
                    if let InstKind::StaticStrRef(sid) = inst.kind
                        && empty.get(sid.0 as usize) == Some(&true)
                    {
                        lits.insert(r);
                    }
                }
                visit_value_operands(&inst.kind, |v| {
                    uses.entry(v).or_default().push(Some((b, i)))
                });
            }
            match &block.term {
                Terminator::Ret(Some(Operand::Value(v)))
                | Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                } => uses.entry(*v).or_default().push(None),
                _ => {}
            }
        }
        // only a value whose one def is the literal names it
        lits.retain(|v| defs.get(v).is_some_and(|d| d.len() == 1));
        Index {
            f,
            defs,
            uses,
            empty_lits: lits,
        }
    }

    fn inst(&self, (b, i): (usize, usize)) -> &'f Inst {
        &self.f.blocks[b].insts[i]
    }
}

/// The values one accumulator flows through.
pub(super) struct Web {
    pub(super) set: HashSet<ValueId>,
    /// the `copy`-of-`''` defs
    pub(super) seeds: Vec<(usize, usize)>,
    /// blocks of the appends whose left operand is in the set
    pub(super) appends: Vec<usize>,
    /// every def and use is one the module doc allows
    pub(super) ok: bool,
    /// every append onto the set writes a UTF-16 literal
    pub(super) utf16: bool,
}

impl Web {
    pub(super) fn grow(r: ValueId, ix: &Index<'_>, k: &Kernels) -> Self {
        let mut web = Web {
            set: HashSet::from_iter([r]),
            seeds: Vec::new(),
            appends: Vec::new(),
            ok: true,
            utf16: true,
        };
        let mut queue = vec![r];
        while let Some(v) = queue.pop() {
            if ix.f.params.contains(&v) {
                web.ok = false;
            }
            let mut joined = Vec::new();
            for &at in ix.defs.get(&v).map_or(&[][..], |d| d.as_slice()) {
                web.def(at, ix, k, &mut joined);
            }
            for &u in ix.uses.get(&v).map_or(&[][..], |u| u.as_slice()) {
                match u {
                    Some(at) => web.use_of(v, at, ix, k, &mut joined),
                    None => web.ok = false,
                }
            }
            for w in joined {
                if web.set.insert(w) {
                    queue.push(w);
                }
            }
        }
        web
    }

    fn def(&mut self, at: (usize, usize), ix: &Index<'_>, k: &Kernels, joined: &mut Vec<ValueId>) {
        match &ix.inst(at).kind {
            InstKind::Copy(_, Operand::Value(w)) if ix.empty_lits.contains(w) => {
                if !self.seeds.contains(&at) {
                    self.seeds.push(at);
                }
            }
            InstKind::Copy(_, Operand::Value(w)) => joined.push(*w),
            InstKind::Call(fid, args) if k.appends.contains(fid) => match args.first() {
                Some(Operand::Value(w)) => joined.push(*w),
                _ => self.ok = false,
            },
            _ => self.ok = false,
        }
    }

    fn use_of(
        &mut self,
        v: ValueId,
        at: (usize, usize),
        ix: &Index<'_>,
        k: &Kernels,
        joined: &mut Vec<ValueId>,
    ) {
        let is_v = |op: &Operand| *op == Operand::Value(v);
        let inst = ix.inst(at);
        let ok = match &inst.kind {
            InstKind::Copy(_, op) if is_v(op) => inst.result.map(|w| joined.push(w)).is_some(),
            // the right operand is only copied from
            InstKind::Call(fid, args) if k.appends.contains(fid) && !is_v(&args[0]) => true,
            InstKind::Call(fid, args) if k.appends.contains(fid) => {
                self.appends.push(at.0);
                self.utf16 &= Some(*fid) == k.append_lit
                    && matches!(args.get(2), Some(Operand::ConstI64(meta))
                        if (*meta as u64) & STR_FLAG_IS_LATIN1 == 0);
                inst.result.map(|w| joined.push(w)).is_some()
            }
            InstKind::Call(fid, args) if *fid == k.drop => inst.result.is_none() && args.len() == 1,
            InstKind::Call(fid, args) => k.readers.get(fid).is_some_and(|slots| {
                args.iter()
                    .enumerate()
                    .all(|(p, a)| !is_v(a) || slots.contains(&p))
            }),
            InstKind::Load(_, base, _) => is_v(base),
            InstKind::LoadDyn(_, base, idx)
            | InstKind::LoadU8Dyn(base, idx)
            | InstKind::LoadU16Dyn(base, idx) => is_v(base) && !is_v(idx),
            _ => false,
        };
        self.ok &= ok;
    }
}
