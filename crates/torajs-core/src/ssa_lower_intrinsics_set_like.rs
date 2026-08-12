//! Pass 0 `declare_intrinsic` group: the §24.2.1.2 GetSetRecord
//! protocol kernels behind the typed tier's Set methods when the
//! argument is NOT statically a Set (a Map, a user set-like struct,
//! an `any`) — the lowering boxes the argument and routes here; a
//! statically-Set argument keeps the Set×Set fast kernels declared
//! in the map_set group. Own group per the map_set declare's
//! no-growth ledger entry (rotation 266).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct SetLikeIds {
    /// `(this_set, other_any, op)` — op 0 isSubsetOf / 1 isSupersetOf
    /// / 2 isDisjointFrom; answers 1/0, 0 under a pending throw.
    pub relation_setlike: FuncId,
    /// `(this_set, other_any, op)` — op 0 union / 1 intersection /
    /// 2 difference / 3 symmetricDifference; answers a fresh rc-1
    /// Set, NULL under a pending throw.
    pub setop_setlike: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> SetLikeIds {
    SetLikeIds {
        relation_setlike: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_relation_setlike",
            &[Type::Set, Type::Any, Type::I64][..],
            Type::I64,
        ),
        setop_setlike: declare_intrinsic(
            module,
            fn_table,
            "__torajs_set_setop_setlike",
            &[Type::Set, Type::Any, Type::I64][..],
            Type::Set,
        ),
    }
}
