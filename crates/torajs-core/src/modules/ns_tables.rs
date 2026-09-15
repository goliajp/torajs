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
    /// Dynamic-import namespace aliases (`__dyn_ns_c<k>`) whose module
    /// was already walked under an earlier alias: no BFS accum exists
    /// for them, so the dispatcher builds their eval fn's namespace
    /// literal from the fields remembered per path.
    pub(super) dyn_ns_repeat: Vec<(String, Vec<(String, String)>)>,
    /// Field lists remembered per path so a second `import("...")` of
    /// an already-visited module (its request is sentinel-deduped to
    /// nothing) still gets its binding.
    pub(super) fields_by_path: HashMap<PathBuf, Vec<(String, String)>>,
    /// Namespace object fields, keyed by the ALIAS rather than the
    /// module — a lib's `export * from "m"` pours m's exports into the
    /// same namespace, and m is a work item popped later.
    pub(super) accums: HashMap<String, NsAccum>,
    /// Discovery order of the aliases, for
    /// `materialize_pending_namespaces`.
    pub(super) order: Vec<String>,
}
