//! Where a cell value of a function comes from, read through the stack
//! slots lowering parks every binding in before `mem2reg` runs: the
//! function's receiver (its first parameter), a cell the function
//! allocated itself, or anything else.

use crate::ssa::id_hash::{HashMap, HashSet};
use crate::ssa::{FuncId, Function, InstKind, Module, Operand, ValueId, visit_value_operands};

/// The [`Roots`] of each function of a module asked about, built once.
pub(super) struct RootsByFn<'m> {
    module: &'m Module,
    alloc: Option<FuncId>,
    by_fn: HashMap<FuncId, Roots<'m>>,
}

impl<'m> RootsByFn<'m> {
    pub(super) fn new(module: &'m Module, alloc: Option<FuncId>) -> Self {
        RootsByFn {
            module,
            alloc,
            by_fn: HashMap::default(),
        }
    }

    pub(super) fn root(&mut self, fid: FuncId, op: &Operand) -> Root {
        let (module, alloc) = (self.module, self.alloc);
        (self.by_fn.entry(fid))
            .or_insert_with(|| Roots::of(&module.funcs[fid.0 as usize], alloc))
            .root(op)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Root {
    Receiver,
    Fresh,
    Other,
}

/// Slots this deep in a chain of loads are not followed.
const DEPTH: usize = 16;

pub(super) struct Roots<'a> {
    f: &'a Function,
    alloc: Option<FuncId>,
    defs: HashMap<ValueId, &'a InstKind>,
    /// What each slot holds, for the slots only loaded and stored
    /// whole: a slot whose address goes anywhere else is not one.
    slots: HashMap<ValueId, Vec<&'a Operand>>,
}

impl<'a> Roots<'a> {
    pub(super) fn of(f: &'a Function, alloc: Option<FuncId>) -> Self {
        let insts = || f.blocks.iter().flat_map(|b| &b.insts);
        let mut defs = HashMap::default();
        let mut slots: HashMap<ValueId, Vec<&Operand>> = HashMap::default();
        for inst in insts() {
            if let Some(v) = inst.result {
                defs.insert(v, &inst.kind);
                if matches!(inst.kind, InstKind::Alloca(_)) {
                    slots.insert(v, Vec::new());
                }
            }
        }
        let mut escaped = HashSet::default();
        for inst in insts() {
            match &inst.kind {
                InstKind::Store(v, Operand::Value(p), 0) if slots.contains_key(p) => {
                    slots.get_mut(p).expect("a slot").push(v);
                    if let Operand::Value(v) = v {
                        escaped.insert(*v);
                    }
                }
                InstKind::Load(_, Operand::Value(_), 0) => {}
                kind => visit_value_operands(kind, |v| {
                    escaped.insert(v);
                }),
            }
        }
        slots.retain(|s, _| !escaped.contains(s));
        Roots {
            f,
            alloc,
            defs,
            slots,
        }
    }

    pub(super) fn root(&self, op: &Operand) -> Root {
        match op {
            Operand::Value(v) => self.walk(*v, &mut Vec::new()),
            _ => Root::Other,
        }
    }

    fn walk(&self, v: ValueId, seen: &mut Vec<ValueId>) -> Root {
        if self.f.params.first() == Some(&v) {
            return Root::Receiver;
        }
        if seen.contains(&v) || seen.len() > DEPTH {
            return Root::Other;
        }
        seen.push(v);
        let r = match self.defs.get(&v) {
            Some(InstKind::Call(t, _)) if Some(*t) == self.alloc => Root::Fresh,
            Some(InstKind::Copy(_, Operand::Value(x)) | InstKind::Identity(Operand::Value(x))) => {
                self.walk(*x, seen)
            }
            Some(InstKind::Load(_, Operand::Value(p), 0)) => match self.slots.get(p) {
                Some(stored) => self.held(stored, seen),
                None => Root::Other,
            },
            _ => Root::Other,
        };
        seen.pop();
        r
    }

    /// The root every value stored into a slot shares; a slot's null
    /// initializer names no cell.
    fn held(&self, stored: &[&Operand], seen: &mut Vec<ValueId>) -> Root {
        let mut held = stored
            .iter()
            .filter(|s| !matches!(s, Operand::ConstPtrNull));
        let Some(first) = held.next() else {
            return Root::Other;
        };
        let r = match first {
            Operand::Value(v) => self.walk(*v, seen),
            _ => Root::Other,
        };
        let same = held.all(|s| match s {
            Operand::Value(v) => self.walk(*v, seen) == r,
            _ => false,
        });
        if same { r } else { Root::Other }
    }
}
