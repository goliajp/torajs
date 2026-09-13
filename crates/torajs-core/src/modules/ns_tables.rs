//! The resolver's namespace tables — the four per-BFS maps a
//! namespace request (`import * as ns`, a dynamic-import candidate,
//! an `export * as ns from`) writes as its walk goes and the final
//! splice reads. Lifted out of `resolve_imports`'s locals when that
//! function reached the size limit (rotation 645); the docs are the
//! ones the locals carried.

use std::collections::HashMap;
use std::path::PathBuf;

use super::NsAccum;

#[derive(Default)]
pub(super) struct NsTables {
    /// Dynamic-import namespaces (`__dyn_ns_<n>`) don't materialize a
    /// top-level `let` — a lifted async-fn body can't see main-local
    /// bindings, so the resolver rewrites each `Ident(__dyn_ns_<n>)`
    /// use site into the namespace object literal directly (the field
    /// Idents resolve against the injected lib decls from any fn
    /// body).
    pub(super) dyn_ns_inline: Vec<(String, Vec<(String, String)>)>,
    /// Field lists remembered per path so a second `import("...")` of
    /// an already-visited module (its request is sentinel-deduped to
    /// nothing) still gets its rewrite.
    pub(super) fields_by_path: HashMap<PathBuf, Vec<(String, String)>>,
    /// Namespace object fields, keyed by the ALIAS rather than the
    /// module — a lib's `export * from "m"` pours m's exports into the
    /// same namespace, and m is a work item popped later.
    pub(super) accums: HashMap<String, NsAccum>,
    /// Discovery order of the aliases, for
    /// `materialize_pending_namespaces`.
    pub(super) order: Vec<String>,
}
