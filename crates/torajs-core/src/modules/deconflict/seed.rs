//! 423-01 — the committed-name seed state, split from `deconflict.rs`
//! when knife D pushed it past the 500-line limit. The parent answers
//! "which decl gets which new name"; this file answers the question
//! that precedes it: which spellings are already taken, and by whom.

use crate::ast::Stmt;
use std::collections::{HashMap, HashSet};

/// The seeded name state: `hard` names collide with any injection
/// (the entry's own top-level decls); `reserved` maps an entry-level
/// import BINDING to the path it resolves from. Bindings reserve
/// their spellings up front — a namespace-injected decl of the same
/// name must deconflict even when its module happens to walk first
/// (BFS order must not decide which of the two keeps the name) — but
/// an injection FROM THE RESERVING PATH is that binding's own decl
/// (§16.2.1.6 one-set-of-bindings: `import "./se"` then
/// `import { S1 } from "./se"` shares S1), so same-path names pass.
/// An unresolvable source or two paths claiming one spelling turn
/// the name hard. Nested libs' own named requests are not visible
/// here; a late collision there lands on the census's loud reject
/// instead of a silent split.
pub(in crate::modules) struct SeedNames {
    pub(in crate::modules) hard: HashSet<String>,
    pub(in crate::modules) reserved: HashMap<String, std::path::PathBuf>,
}

impl SeedNames {
    fn reserve(&mut self, name: String, path: Option<&std::path::Path>) {
        match path {
            Some(p) if !self.hard.contains(&name) => match self.reserved.entry(name.clone()) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    if e.get() != p {
                        e.remove();
                        self.hard.insert(name);
                    }
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(p.to_path_buf());
                }
            },
            _ => {
                self.reserved.remove(&name);
                self.hard.insert(name);
            }
        }
    }
}

pub(in crate::modules) fn seed_seen_names(
    ast: &crate::ast::Ast,
    base_dir: &std::path::Path,
) -> SeedNames {
    let mut seed = SeedNames {
        hard: HashSet::new(),
        reserved: HashMap::new(),
    };
    for s in &ast.stmts {
        match s {
            Stmt::ImportDecl {
                source,
                named,
                default,
                namespace,
            } => {
                let path = crate::modules::resolve_path(base_dir, source).ok();
                for (orig, alias) in named {
                    let n = alias.clone().unwrap_or_else(|| orig.clone());
                    seed.reserve(n, path.as_deref());
                }
                if let Some(d) = default {
                    seed.reserve(d.clone(), path.as_deref());
                }
                if let Some(n) = namespace {
                    // The namespace ALIAS binding is entry-scope, not
                    // any lib's own decl — always hard.
                    seed.reserve(n.clone(), None);
                }
            }
            other => {
                let inner = match other {
                    Stmt::ExportDecl {
                        inner: Some(inner), ..
                    } => inner,
                    o => o,
                };
                if let Some(n) = crate::modules::decl_name(inner) {
                    seed.reserve(n, None);
                }
            }
        }
    }
    seed
}

/// Commit one walked lib's (post-census) top-level INJECTED spellings
/// — hard: whoever comes later collides with them. A bare-exported
/// decl injects under its face (P13-S4b renames it), so the face is
/// what later walks must avoid; a census-mangled bare decl's map
/// entry is the identity, which lands here unchanged.
pub(in crate::modules) fn extend_seen_with_lib(
    lib_section: &[Stmt],
    bare_exports: &HashMap<String, String>,
    seed: &mut SeedNames,
) {
    for s in lib_section {
        let inner = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner,
            other => other,
        };
        if let Some(n) = crate::modules::decl_name(inner) {
            let injected = bare_exports.get(&n).cloned().unwrap_or(n);
            seed.reserved.remove(&injected);
            seed.hard.insert(injected);
        }
    }
}
