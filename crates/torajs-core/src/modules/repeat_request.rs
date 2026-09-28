//! What a repeat visit of an already-walked path remembers — split
//! from `resolve_imports` when the 426 knives pushed it past the
//! 200-line limit. The parent drives the BFS; this file answers how
//! a request aligns itself to the walks that came before it (the
//! dyn-namespace field shortcut, the mangle-memory realignment) and
//! what a namespace walk leaves behind for the next one.

use crate::ast::{Ast, Stmt};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::resolve_helpers::{NsAccum, filter_request};
use super::{NamedImport, default_binding};

/// The whole pre-walk alignment for one dequeued request: the
/// repeat-visit shortcuts, the per-path injected-state filter, and
/// the mangle-memory realignment of the named list. `None` = the
/// request filtered to nothing (everything it wants already landed)
/// — or a side-effect request for the ENTRY itself (`import
/// "./self"`, a lib's `export { x } from "./entry"`), which
/// evaluates nothing extra: the entry is the program, and a walk
/// would splice a second copy of its whole body in front of it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn align_and_filter(
    ast: &mut Ast,
    target_path: &Path,
    entry_path: Option<&Path>,
    named: Vec<NamedImport>,
    mut default_alias: Option<String>,
    mut namespace_alias: Option<String>,
    side_effect_only: bool,
    ns_fields_by_path: &HashMap<PathBuf, Vec<(String, String)>>,
    dyn_ns_repeat: &mut Vec<(String, Vec<(String, String)>)>,
    default_bound_as: &HashMap<PathBuf, String>,
    injections_by_path: &mut HashMap<PathBuf, Vec<Stmt>>,
    injected_names: &mut HashMap<PathBuf, HashSet<String>>,
    mangled_by_path: &HashMap<PathBuf, HashMap<String, String>>,
) -> Option<(Vec<NamedImport>, Option<String>, Option<String>, bool)> {
    if side_effect_only && entry_path == Some(target_path) {
        return None;
    }
    align_repeat_request(
        ast,
        target_path,
        &mut default_alias,
        &mut namespace_alias,
        ns_fields_by_path,
        dyn_ns_repeat,
        default_bound_as,
        injections_by_path,
    );
    let entry = injected_names.entry(target_path.to_path_buf()).or_default();
    let (mut named, default_alias, namespace_alias, side_effect_only) = filter_request(
        entry,
        named,
        default_alias,
        namespace_alias,
        side_effect_only,
    )?;
    realign_named_to_mangles(&mut named, mangled_by_path.get(target_path));
    Some((named, default_alias, namespace_alias, side_effect_only))
}

/// The pre-filter half: a dyn-import namespace revisiting a path
/// whose fields are already known binds them without re-walking,
/// and a repeat default request binds by reference (see
/// `default_binding::shortcut_repeat_default`).
#[allow(clippy::too_many_arguments)]
pub(super) fn align_repeat_request(
    ast: &mut Ast,
    target_path: &Path,
    default_alias: &mut Option<String>,
    namespace_alias: &mut Option<String>,
    ns_fields_by_path: &HashMap<PathBuf, Vec<(String, String)>>,
    dyn_ns_repeat: &mut Vec<(String, Vec<(String, String)>)>,
    default_bound_as: &HashMap<PathBuf, String>,
    injections_by_path: &mut HashMap<PathBuf, Vec<Stmt>>,
) {
    if let Some(ns) = namespace_alias
        .as_ref()
        .filter(|n| n.starts_with("__dyn_ns_"))
        && let Some(fields) = ns_fields_by_path.get(target_path)
    {
        dyn_ns_repeat.push((ns.clone(), fields.clone()));
        *namespace_alias = None;
    }
    default_binding::shortcut_repeat_default(
        ast,
        target_path,
        default_alias,
        default_bound_as,
        injections_by_path,
    );
}

/// A named request that ALIASES its binding follows this path's
/// mangle memory: the source-side decl an earlier walk renamed
/// re-mangles to the same spelling, and the alias is what the
/// importer sees anyway (the `__reex_` fetches ride this). A
/// plain-spelling request keeps the orig — the importer wants the
/// very name the mangle took away, and the census's loud reject
/// stays right for it.
pub(super) fn realign_named_to_mangles(
    named: &mut [NamedImport],
    prior: Option<&HashMap<String, String>>,
) {
    let Some(prior) = prior else { return };
    for (orig, alias) in named.iter_mut() {
        if let Some(m) = prior.get(orig)
            && alias.as_ref().is_some_and(|a| a != orig)
        {
            *orig = m.clone();
        }
    }
}

/// For P13-S2 namespace import: make sure the alias owns an
/// accumulator (the collector of every value-export injected) and
/// answer where THIS lib's own contribution starts, so the dyn-ns
/// revisit shortcut still records per-path fields.
pub(super) fn open_ns_walk(
    namespace_alias: &Option<String>,
    ns_accums: &mut HashMap<String, NsAccum>,
    ns_order: &mut Vec<String>,
) -> usize {
    let Some(alias) = namespace_alias else {
        return 0;
    };
    if !ns_accums.contains_key(alias) {
        ns_order.push(alias.clone());
        ns_accums.insert(alias.clone(), NsAccum::new(alias.clone()));
    }
    ns_accums[alias].fields().len()
}

/// The post-walk half: settle the repeat side of the ns `default`
/// field (the binding already exists, only the claim is owed) and
/// snapshot this walk's fields so a dyn revisit can inline them —
/// claim before snapshot, so the snapshot carries the field.
pub(super) fn record_ns_walk(
    namespace_alias: &Option<String>,
    repeat_default_field: Option<String>,
    ns_accums: &mut HashMap<String, NsAccum>,
    first_field: usize,
    target_path: &Path,
    ns_fields_by_path: &mut HashMap<PathBuf, Vec<(String, String)>>,
) {
    let Some(alias) = namespace_alias else { return };
    if let Some(binding) = repeat_default_field
        && let Some(accum) = ns_accums.get_mut(alias.as_str())
    {
        accum.claim("default", &binding);
    }
    let fields = ns_accums[alias].fields()[first_field..].to_vec();
    ns_fields_by_path.insert(target_path.to_path_buf(), fields);
}

/// §16.2.1.5 — which walk runs a module's body. The first request of
/// the entry's static closure to reach it does (`__eval` joins the
/// path's ledger, and a later side-effect request filters to
/// nothing); a side-effect walk already ran it (`__se`); the entry is
/// the program itself. A dynamic-import candidate defers its body to
/// the `import()` — unless a static walk already ran it, when the
/// second answer is `false` and the candidate collects nothing
/// (§16.2.1.5.3: an evaluated module is not evaluated again).
pub(super) fn evaluation_role(
    injected_names: &mut HashMap<PathBuf, HashSet<String>>,
    target_path: &Path,
    entry_path: Option<&Path>,
    side_effect_only: bool,
    static_phase: bool,
    dyn_candidate: bool,
) -> (bool, bool) {
    let ledger = injected_names.entry(target_path.to_path_buf()).or_default();
    if dyn_candidate {
        return (false, !ledger.contains("__eval"));
    }
    let evaluate = static_phase
        && !side_effect_only
        && entry_path != Some(target_path)
        && !ledger.contains("__se")
        && ledger.insert("__eval".into());
    (evaluate, false)
}

/// §16.2.1 — a module's declaration is ONE binding however many
/// requests reach it. Each request re-parses the module and its census
/// may land the same declaration under another spelling (a mangle for a
/// namespace walk, an alias for a named one), so a later walk would
/// inject a second copy: `a === m.a` answered false, and an object
/// literal was evaluated twice. `pushed` is what one source statement
/// injected in this walk, `orig` its declaration's own name in the
/// module; the first spelling it ever landed under is remembered in
/// `landed`, and every later one binds to it by reference. That also
/// covers a function asked for under two spellings in one request.
/// Each `(later, first)` pair goes to `aliases` for
/// [`follow_aliases`].
pub(super) fn bind_repeat_decls(
    ast: &mut Ast,
    pushed: &mut [Stmt],
    orig: &str,
    landed: &mut HashMap<String, String>,
    aliases: &mut Vec<(String, String)>,
) {
    for s in pushed.iter_mut() {
        let name = match s {
            Stmt::FnDecl { name, .. }
            | Stmt::ClassDecl { name, .. }
            | Stmt::LetDecl {
                name,
                is_var: false,
                ..
            } => name.clone(),
            _ => continue,
        };
        let Some(first) = landed.get(orig) else {
            landed.insert(orig.to_string(), name);
            continue;
        };
        if *first == name {
            continue;
        }
        let init = ast.add_expr(crate::ast::Expr::Ident(first.clone()));
        aliases.push((name.clone(), first.clone()));
        *s = Stmt::LetDecl {
            mutable: false,
            name,
            type_ann: None,
            init,
            is_var: false,
        };
    }
}

/// §16.2.1 — an import binding is an indirect binding to the export's
/// own binding, not a copy of its value. The `const later = first`
/// that [`bind_repeat_decls`] leaves reads the export once: a `let`
/// the module later reassigns stays stale through it, and a requester
/// that runs before its provider (an import cycle) cannot call a
/// function through it — the `const` is not hoisted, the function
/// declaration is. Every program-wide reference to `later` therefore
/// follows `first` itself. The flattened program gives each top-level
/// name one binding, so the blind arena rewrite is sound unless some
/// inner scope rebinds either spelling; such a pair keeps the `const`.
/// A class keeps it too: its binding is not hoisted either, and a
/// class spelling also lives in type annotations this rewrite does not
/// reach.
pub(super) fn follow_aliases(ast: &mut Ast, aliases: &[(String, String)]) {
    let map: HashMap<&str, &str> = aliases
        .iter()
        .filter(|(later, first)| !rebound(ast, later) && !rebound(ast, first))
        .map(|(later, first)| (later.as_str(), first.as_str()))
        .collect();
    if map.is_empty() {
        return;
    }
    for e in ast.exprs.iter_mut() {
        match e {
            crate::ast::Expr::Ident(n) => {
                if let Some(first) = map.get(n.as_str()) {
                    *n = first.to_string();
                }
            }
            crate::ast::Expr::New { class_name, .. } => {
                if let Some(first) = map.get(class_name.as_str()) {
                    *class_name = first.to_string();
                }
            }
            _ => {}
        }
    }
}

/// Does anything but `name`'s own top-level function or lexical
/// declaration bind it — a nested scope, a class, an arrow's
/// parameter?
pub(super) fn rebound(ast: &Ast, name: &str) -> bool {
    let in_stmts = ast.stmts.iter().any(|s| {
        let s = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner,
            other => other,
        };
        match s {
            Stmt::FnDecl {
                name: n,
                params,
                body,
                ..
            } if n == name => {
                params.iter().any(|p| p.name == name)
                    || body.iter().any(|b| crate::ast::rebinds_in_stmt(b, name))
            }
            Stmt::LetDecl {
                name: n,
                is_var: false,
                ..
            } if n == name => false,
            other => crate::ast::rebinds_in_stmt(other, name),
        }
    });
    in_stmts
        || ast.exprs.iter().any(|e| {
            matches!(e, crate::ast::Expr::ArrowFn { params, body, .. }
                if params.iter().any(|p| p.name == name)
                    || body.iter().any(|s| crate::ast::rebinds_in_stmt(s, name)))
        })
}
