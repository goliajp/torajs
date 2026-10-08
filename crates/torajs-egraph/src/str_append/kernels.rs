//! The binary string kernels the append rewrites reason about: each
//! is `f(left, right) -> Str` answering a fresh owned cell, and the
//! table records which side is a view and whether the kernel takes
//! the caller's reference to `left` (the append family) or only
//! borrows it (the concat family).

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand};

/// The static shape of a string operand: an owned or borrowed Str
/// cell, or a Substr view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Str,
    Substr,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Kernel {
    pub fid: FuncId,
    pub left: Kind,
    pub right: Kind,
    /// The kernel releases the caller's reference to `left`.
    pub consumes_left: bool,
}

const TABLE: [(&str, Kind, Kind, bool); 6] = [
    ("__torajs_str_concat", Kind::Str, Kind::Str, false),
    (
        "__torajs_substr_concat_substr_str",
        Kind::Substr,
        Kind::Str,
        false,
    ),
    (
        "__torajs_substr_concat_str_substr",
        Kind::Str,
        Kind::Substr,
        false,
    ),
    (
        "__torajs_substr_concat_substr_substr",
        Kind::Substr,
        Kind::Substr,
        false,
    ),
    ("__torajs_str_append", Kind::Str, Kind::Str, true),
    ("__torajs_str_append_substr", Kind::Str, Kind::Substr, true),
];
const STR_DROP: &str = "__torajs_str_drop";
const SUBSTR_DROP: &str = "__torajs_substr_drop";

/// The kernels a module declares.
pub(crate) struct Kernels {
    binary: Vec<Kernel>,
    pub str_drop: FuncId,
    substr_drop: Option<FuncId>,
}

impl Kernels {
    /// `None` when the module concatenates no strings at all, or
    /// lacks the drop the rewrites pair with.
    pub fn find(module: &Module) -> Option<Self> {
        let find = |name: &str| {
            module
                .funcs
                .iter()
                .position(|f| f.name == name)
                .map(|i| FuncId(i as u32))
        };
        let binary: Vec<Kernel> = TABLE
            .iter()
            .filter_map(|&(name, left, right, consumes_left)| {
                Some(Kernel {
                    fid: find(name)?,
                    left,
                    right,
                    consumes_left,
                })
            })
            .collect();
        if binary.is_empty() {
            return None;
        }
        Some(Self {
            binary,
            str_drop: find(STR_DROP)?,
            substr_drop: find(SUBSTR_DROP),
        })
    }

    /// The kernels that only borrow both operands.
    pub fn borrowing(&self) -> impl Iterator<Item = FuncId> + '_ {
        self.binary
            .iter()
            .filter(|k| !k.consumes_left)
            .map(|k| k.fid)
    }

    pub fn get(&self, fid: FuncId) -> Option<&Kernel> {
        self.binary.iter().find(|k| k.fid == fid)
    }

    /// The kernel behind `inst` when it is a two-operand call to one.
    pub fn call<'a>(&self, inst: &'a Inst) -> Option<(&Kernel, &'a [Operand])> {
        match &inst.kind {
            InstKind::Call(fid, args) if args.len() == 2 => Some((self.get(*fid)?, args)),
            _ => None,
        }
    }

    pub fn is_drop(&self, fid: FuncId) -> bool {
        fid == self.str_drop || Some(fid) == self.substr_drop
    }

    /// A call whose only effect is on its own operands' cells: one
    /// of the binary kernels or a drop.
    pub fn is_kernel(&self, fid: FuncId) -> bool {
        self.is_drop(fid) || self.get(fid).is_some()
    }

    /// The kernel for `left ++ right` under the given shapes, when
    /// the module declares it.
    pub fn flavor(&self, left: Kind, consumes_left: bool, right: Kind) -> Option<FuncId> {
        self.binary
            .iter()
            .find(|k| k.left == left && k.right == right && k.consumes_left == consumes_left)
            .map(|k| k.fid)
    }

    /// The value a result-less drop call releases.
    pub fn dropped<'a>(&self, inst: &'a Inst) -> Option<&'a Operand> {
        match &inst.kind {
            InstKind::Call(fid, args)
                if inst.result.is_none() && args.len() == 1 && self.is_drop(*fid) =>
            {
                Some(&args[0])
            }
            _ => None,
        }
    }
}
