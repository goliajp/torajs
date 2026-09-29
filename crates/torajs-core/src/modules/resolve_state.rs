//! Everything [`super::resolve_imports`] carries across worklist
//! iterations. The parent drives the BFS; this file answers what the
//! drive remembers between one request and the next, and why each
//! piece is kept the way it is.

use crate::ast::{Ast, Stmt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::graph::ModuleGraph;
use super::{WorkItem, deconflict, ns_tables, static_resolution};

pub(super) struct ResolveState {
    pub(super) closure_files: Vec<(PathBuf, Vec<u8>)>,
    /// Per-path "already injected" tracking. The set holds importer-
    /// visible names (= alias if Some, else orig) plus the sentinels
    /// `__default` / `__namespace` / `__se` for the non-named import
    /// forms. A path may be re-visited via a different request shape
    /// (e.g., a transitive re-export's nested load asking for a name
    /// not in the first visit's named list); the per-name filter
    /// de-duplicates injections without forcing first-encounter wins.
    pub(super) injected_names: HashMap<PathBuf, HashSet<String>>,
    pub(super) closure_paths: HashSet<PathBuf>,
    pub(super) work: VecDeque<WorkItem>,
    /// §16.2.1.5 — a module body runs AFTER every module it requests.
    /// The worklist is breadth-first, so pop order is the opposite;
    /// the graph is recorded here instead and the injected statements
    /// are spliced back in dependency post-order once the queue
    /// drains. An edge is whatever a walk pushed onto the worklist, so
    /// it needs no threading: [`ModuleGraph::edges_since`] reads the
    /// tail.
    pub(super) graph: ModuleGraph,
    /// 423-01 deconflict state — the committed-name set (entry decls +
    /// entry import bindings, see `deconflict::seed_seen_names`) plus
    /// each path's mangle memory (the same file re-parses per request,
    /// and the same decl must keep the same mangle).
    pub(super) seen_names: deconflict::SeedNames,
    pub(super) mangled_by_path: HashMap<PathBuf, HashMap<String, String>>,
    /// Knife C hidden-dep mangles keep their own per-path memory —
    /// NOT `mangled_by_path`: a later plain named request for a
    /// hidden-mangled export must re-inject bare, not trip the
    /// requested-collision reject (see `deconflict_lib_section`).
    pub(super) hidden_by_path: HashMap<PathBuf, HashMap<String, String>>,
    pub(super) mangle_seq: usize,
    /// Per path, each declaration's own name → the spelling it first
    /// landed under (see `repeat_request::bind_repeat_decls`).
    pub(super) landed_by_path: HashMap<PathBuf, HashMap<String, String>>,
    /// Every `(later, first)` spelling pair a repeat landing bound
    /// (see `repeat_request::follow_aliases`).
    pub(super) repeat_aliases: Vec<(String, String)>,
    /// Import spellings bound to the entry's own bindings without a
    /// walk (`entry_namespace::bind_entry_self_imports`) — resolved
    /// requests with no injected declaration behind them.
    pub(super) entry_self_bindings: HashSet<String>,
    /// Statements injected on behalf of one module, keyed by its path
    /// so the dependency-order splice can reassemble them. A path may
    /// be walked more than once (a later request asking for a name the
    /// first visit did not want); the later visit appends.
    pub(super) injections_by_path: HashMap<PathBuf, Vec<Stmt>>,
    /// The namespace tables — see `ns_tables::NsTables`.
    pub(super) ns: ns_tables::NsTables,
    /// §16.2.1.6.3 ambiguity ledger — see `inject_export::Landings`.
    pub(super) landing_first: HashMap<String, PathBuf>,
    pub(super) ambiguous_locals: HashSet<String>,
    /// The alias a path's default export materialized under. A
    /// module's default is ONE export but nothing stops several
    /// requests wanting it under different names (`export { default
    /// as X } from "m"` beside `export { default } from "m"`); the
    /// first request wins the `let <alias> = <expr>` and every later
    /// alias binds to the first — re-walking would re-lower the same
    /// ExprId.
    pub(super) default_bound_as: HashMap<PathBuf, String>,
    pub(super) ledger: static_resolution::LedgerState,
    /// Module-evaluation statements per dynamic-import candidate,
    /// keyed by its `__dyn_ns_c<k>` alias — see `LibRequest::deferred`.
    pub(super) dyn_eval_bodies: HashMap<String, Vec<Stmt>>,
    /// Type imports seen in any walked module, as (source path,
    /// exported name, local spelling) — see
    /// `types_lane::note_type_aliases`.
    pub(super) type_aliases: Vec<(PathBuf, String, String)>,
    /// Every class declaration a types-only walk read, with its module
    /// and the export spelling it answers to (`types_lane::walk_types`);
    /// the ones a type-only import names and no value walk landed are
    /// injected for their shape (`types_lane::inject_ambient_classes`).
    pub(super) ambient_classes: Vec<(PathBuf, String, Stmt)>,
}

impl ResolveState {
    pub(super) fn new(ast: &Ast, base_dir: &Path) -> Self {
        ResolveState {
            closure_files: Vec::new(),
            injected_names: HashMap::new(),
            closure_paths: HashSet::new(),
            work: VecDeque::new(),
            graph: ModuleGraph::default(),
            seen_names: deconflict::seed_seen_names(ast, base_dir),
            mangled_by_path: HashMap::new(),
            hidden_by_path: HashMap::new(),
            mangle_seq: 0,
            landed_by_path: HashMap::new(),
            ambient_classes: Vec::new(),
            repeat_aliases: Vec::new(),
            entry_self_bindings: HashSet::new(),
            injections_by_path: HashMap::new(),
            ns: ns_tables::NsTables::default(),
            landing_first: HashMap::new(),
            ambiguous_locals: HashSet::new(),
            default_bound_as: HashMap::new(),
            ledger: static_resolution::LedgerState::default(),
            dyn_eval_bodies: HashMap::new(),
            type_aliases: entry_type_aliases(ast, base_dir),
        }
    }
}

fn entry_type_aliases(ast: &Ast, base_dir: &Path) -> Vec<(PathBuf, String, String)> {
    let mut out = Vec::new();
    for s in &ast.stmts {
        super::types_lane::note_type_aliases(&mut out, base_dir, s);
    }
    out
}
