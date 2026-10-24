//! Where lowering left the references a call holds on a receiver or
//! argument it read out of a place, so the module-wide pass that
//! proves a hold needless can remove it.
//!
//! Each piece of a hold, its retain and every release, sits behind its
//! own branch: the block `at` ends in `cond_br true, body, join`, the
//! blocks `body` run the piece and end at `join`, and nothing else in
//! the function enters them. Taking the branch keeps the piece; jumping
//! straight to `join` removes it without touching anything around it.

use super::{BlockId, FuncId, StructId};

/// One guarded piece of a hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldGuard {
    /// The block whose terminator picks the piece or skips it.
    pub at: BlockId,
    /// The blocks the piece runs in, `body.0 .. body.1`; the first is
    /// the branch's target.
    pub body: (u32, u32),
    /// Where the piece and the skip meet again.
    pub join: BlockId,
}

/// One call's reference on a cell of class sid `sid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHold {
    pub sid: StructId,
    /// The retain, taken right after the value is read.
    pub take: HoldGuard,
    /// The releases: on the normal path once the call is done, and on
    /// the throw path of every check in between.
    pub drops: Vec<HoldGuard>,
    /// Which of `drops` is the normal path's; `None` while the call is
    /// still being lowered.
    pub done: Option<usize>,
}

/// The holds of every function the module keeps as it was lowered.
pub type CallHolds = Vec<(FuncId, Vec<CallHold>)>;
