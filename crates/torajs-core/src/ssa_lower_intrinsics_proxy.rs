//! Pass 0 `declare_intrinsic` group: Proxy substrate
//! (RFC 20260823-proxy-substrate).
//!
//! 刀 1 face is one kernel — §10.5.14 ProxyCreate. Everything a
//! proxy DOES reaches it through the any-lane kernels that already
//! exist, so the trap dispatch needs no intrinsic of its own.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ProxyIds {
    pub proxy_create: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ProxyIds {
    ProxyIds {
        // (target, handler — both borrowed Any) → owned Proxy Any;
        // a non-object argument records the §10.5.14 TypeError and
        // answers undefined for the caller's throw check.
        proxy_create: declare_intrinsic(
            module,
            fn_table,
            "__torajs_proxy_create",
            &[Type::Any, Type::Any],
            Type::Any,
        ),
    }
}
