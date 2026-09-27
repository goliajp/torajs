//! One request on the resolver's worklist: which module, and what the
//! requester wants from it. The constructors name the request shapes a
//! walk queues; the fields are what [`super::resolve_imports`] reads
//! back when it pops the request.

use std::path::PathBuf;

use super::NamedImport;

pub(super) struct WorkItem {
    /// Canonicalized absolute path of the module to load.
    pub(super) path: PathBuf,
    /// The named imports that drove the request.
    pub(super) named: Vec<NamedImport>,
    /// `Some(x)` when the importer wrote `import x from "./y"`; the
    /// lib's `export default <expr>` resolves to a synthetic
    /// `let x = <expr>`.
    pub(super) default_alias: Option<String>,
    /// Bare `import "./y"`: the lib's full top-level statement list
    /// injects in source order regardless of `export` wrapping.
    pub(super) side_effect_only: bool,
    /// `Some(M)` when the importer wrote `import * as M from "./y"`;
    /// the lib's full export list materializes as a synthetic
    /// `let M = { name1: name1, … }` after the named decls inject.
    pub(super) namespace_alias: Option<String>,
    /// An `export * from "m"` pour into an existing namespace
    /// accumulator: §16.2.3 star forwarding excludes `default`, so the
    /// ns `default`-field synthesis must skip these walks (a DIRECT
    /// namespace request and `export * as ns` both carry the module's
    /// own default and stay false).
    pub(super) ns_star_feed: bool,
    /// Reached only through `import type` / `export type … from`: the
    /// module is not requested for evaluation, but its exported type
    /// declarations must reach the checker (see `types_lane`).
    pub(super) types_only: bool,
}

impl WorkItem {
    /// An import clause as written: side-effect-only when it binds
    /// nothing.
    pub(super) fn clause(
        path: PathBuf,
        named: Vec<NamedImport>,
        default_alias: Option<String>,
        namespace_alias: Option<String>,
    ) -> Self {
        let side_effect_only =
            named.is_empty() && default_alias.is_none() && namespace_alias.is_none();
        WorkItem {
            path,
            named,
            default_alias,
            side_effect_only,
            namespace_alias,
            ns_star_feed: false,
            types_only: false,
        }
    }

    pub(super) fn named(path: PathBuf, named: Vec<NamedImport>) -> Self {
        Self::clause(path, named, None, None)
    }

    pub(super) fn default(path: PathBuf, alias: String) -> Self {
        Self::clause(path, Vec::new(), Some(alias), None)
    }

    pub(super) fn namespace(path: PathBuf, alias: String) -> Self {
        Self::clause(path, Vec::new(), None, Some(alias))
    }

    pub(super) fn side_effect(path: PathBuf) -> Self {
        Self::clause(path, Vec::new(), None, None)
    }

    pub(super) fn types(path: PathBuf) -> Self {
        WorkItem {
            side_effect_only: false,
            types_only: true,
            ..Self::side_effect(path)
        }
    }
}
