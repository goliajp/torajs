//! r502 (RFC 20260824-s2-5 刀 4 A8) — the judged columns of the
//! class-method rows the link bakes into `class_layouts`.
//!
//! A row is `{ name, flags, adapter, twin }`. The name and flags are
//! read by the inspect walker; the two fn-address columns each root
//! a user fn (user-gc) whose per-parameter unbox / any-lane member
//! reads root the any world. Each column has a small, fixed set of
//! runtime readers — structmeta finder entries — so its liveness is
//! link-judged exactly like a seam ([`crate::dead_strip_elide`]
//! runs the fix-point; this module names the readers and rewrites
//! the table). The readers are a runtime-substrate fact, not
//! per-program policy, which is why the symbols live in the link
//! crate (as `force_emit_derive` names the table globals).

use crate::dead_strip_elide::Guard;
use crate::exec::UserClassLayoutEntry;

/// The one runtime reader of a class-method row's twin slot.
const TWIN_READER: &str = "___torajs_struct_method_twin_at";
/// The runtime entries that hand out a class-method row's adapter to
/// be invoked (the any-receiver method call, the class accessor
/// faces, the register kernel's reification). The inspect walker
/// enumerates rows through `__torajs_struct_method_name_at` and is
/// deliberately not one of them.
const ADAPTER_READERS: [&str; 5] = [
    "___torajs_struct_method_find",
    "___torajs_struct_method_find_flags",
    "___torajs_struct_accessor_method_find",
    "___torajs_struct_accessor_method_find_flags",
    "___torajs_struct_method_at",
];

/// The two judged columns of the class-method rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Columns {
    pub(crate) twin: bool,
    pub(crate) adapter: bool,
}

impl Columns {
    #[cfg(test)]
    pub(crate) const NONE: Columns = Columns {
        twin: false,
        adapter: false,
    };
    pub(crate) fn any(self) -> bool {
        self.twin || self.adapter
    }
    /// The twin column's evidence.
    pub(crate) fn twin_guard() -> Guard {
        Guard::Symbols(vec![TWIN_READER.to_string()])
    }
    /// The adapter column's evidence.
    pub(crate) fn adapter_guard() -> Guard {
        Guard::Symbols(ADAPTER_READERS.iter().map(|s| (*s).to_string()).collect())
    }
}

/// Which columns any method row populates at all.
pub(crate) fn columns_present(layouts: &[UserClassLayoutEntry]) -> Columns {
    let rows = || layouts.iter().flat_map(|cl| cl.methods.iter());
    Columns {
        twin: rows().any(|m| m.twin_fn_id.is_some()),
        adapter: rows().any(|m| m.adapter_fn_id.is_some()),
    }
}

/// The same table with the dropped columns' slots baking 0 (the
/// shape a method that minted no twin already has).
pub(crate) fn without_columns(
    layouts: &[UserClassLayoutEntry],
    drop: Columns,
) -> Vec<UserClassLayoutEntry> {
    layouts
        .iter()
        .map(|cl| {
            let mut cl = cl.clone();
            for m in &mut cl.methods {
                if drop.twin {
                    m.twin_fn_id = None;
                }
                if drop.adapter {
                    m.adapter_fn_id = None;
                }
            }
            cl
        })
        .collect()
}
