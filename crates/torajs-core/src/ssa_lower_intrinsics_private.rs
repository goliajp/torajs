//! Pass 0 `declare_intrinsic` group: private-name (`#x`) brand
//! substrate.
//!
//! A `#x` member READ lowers its tag channel to
//! `__torajs_any_member_get_priv_tag` (statically selected by the
//! `__priv_` prefix in `emit_member_fallback`) — same walk as the
//! base tag channel, but an own-face miss throws TypeError per
//! §7.3.31 PrivateGet / PrivateBrandCheck instead of answering
//! `undefined`. The paired value channel stays the base intrinsic.
//! Grows with future brand-check kernels (`#x in o`, private write
//! miss).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct PrivateIds {
    pub any_member_get_priv_tag: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> PrivateIds {
    PrivateIds {
        any_member_get_priv_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_member_get_priv_tag",
            &[Type::Any, Type::Ptr],
            Type::I64,
        ),
    }
}
