//! Parked owned-temp scratch threaded through `LowerCtx` — operands
//! whose mint site and release site straddle other emission (a runtime
//! helper call, a may-throw lower of a sibling subexpression), moved
//! out of `ssa_lower_ctx_struct.rs` as their own state family.

use crate::ssa::{Operand, Type};

#[derive(Default)]
pub(crate) struct TempScratch {
    /// RFC 20260712 chunk B — fresh-owned operands parked in a str
    /// method's argv (ToString-coerced searchValue/replaceValue,
    /// fresh temp args) that must drop AFTER the runtime helper
    /// call consumes them. `populate_argv` pushes; `dispatch_
    /// intrinsic` drains right after the emit. Pre-B these leaked
    /// (300k `replace(n.slice(0,1), ..)` churned 16MB).
    pub(crate) argv_owned: Vec<(Operand, Type)>,
}
