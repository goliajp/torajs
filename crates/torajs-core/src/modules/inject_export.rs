//! Decl injection for the BFS resolver — the "with which spellings
//! does one lib decl land in the importer's scope" half, split from
//! `resolve_helpers.rs` when the 421-04 multi-alias fan-out pushed
//! that file past the 500-line limit. The parent keeps request
//! filtering and the re-export / star translations; this file owns
//! the per-decl injection (spelling fan-out, deep clones, reference
//! bindings, K.2 self-reference follows).

use crate::ast::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::resolve_helpers::NsAccum;

/// Where each importer-visible spelling's declaration landed — the
/// §16.2.1.6.3 ambiguity detector's ground truth. Two `export * from`
/// clauses forwarding the same requested name from DIFFERENT modules
/// each inject a decl under the same alias (per-path ledgers don't
/// dedupe across paths), which is exactly ResolveExport's "ambiguous"
/// verdict; a transitive diamond converges on ONE final module whose
/// per-path ledger absorbs the second request, so it never lands
/// twice and never false-positives here. Consumers: the dyn-import
/// dispatcher poisons candidates whose namespace touches an
/// ambiguous spelling (§16.2.1.5 promise-reject).
pub(super) struct Landings<'a> {
    /// The module path this walk is injecting from.
    pub(super) path: &'a Path,
    /// alias → the first path that landed a decl under it.
    pub(super) first: &'a mut HashMap<String, PathBuf>,
    /// aliases a second, different path landed again.
    pub(super) ambiguous: &'a mut HashSet<String>,
}

impl Landings<'_> {
    fn record(&mut self, alias: &str) {
        match self.first.get(alias) {
            Some(p) if p != self.path => {
                self.ambiguous.insert(alias.to_string());
            }
            Some(_) => {}
            None => {
                self.first
                    .insert(alias.to_string(), self.path.to_path_buf());
            }
        }
    }
}

/// The name a top-level statement declares, if any — the resolver's
/// decl-identity face (moved from `modules.rs` when its file budget
/// ran out; this file is the main consumer).
pub(super) fn decl_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::FnDecl { name, .. }
        | Stmt::LetDecl { name, .. }
        | Stmt::TypeDecl { name, .. }
        | Stmt::ClassDecl { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Point a declaration at a new name (references are the walkers'
/// business).
pub(super) fn rename_decl(s: &mut Stmt, new_name: String) {
    match s {
        Stmt::FnDecl { name, .. }
        | Stmt::LetDecl { name, .. }
        | Stmt::TypeDecl { name, .. }
        | Stmt::ClassDecl { name, .. } => *name = new_name,
        _ => {}
    }
}

/// `export <decl>` (inner Some) injection — type decls always inject;
/// namespace imports inject every value decl under its original name
/// (accumulating `namespace_fields` for the synthetic object literal);
/// named imports filter by `want` and inject one decl per
/// importer-visible spelling (421-04: `import { fa, fa as renamed }`
/// binds BOTH names; a single-alias `rename` map collapsed them).
#[allow(clippy::too_many_arguments)]
pub(super) fn inject_export_inner(
    ast: &mut Ast,
    injections: &mut Vec<Stmt>,
    mut inner: Stmt,
    want: &HashSet<&str>,
    rename: &HashMap<&str, Vec<&str>>,
    ns: Option<&mut NsAccum>,
    injected: &mut HashSet<String>,
    demangle: &HashMap<String, String>,
    hidden: &HashSet<String>,
    landings: &mut Landings<'_>,
) {
    // Type decls always inject — TS doesn't require type
    // names in the value-import list, and downstream
    // check.rs needs them to resolve fn return-type
    // annotations on imported value decls.
    if matches!(inner, Stmt::TypeDecl { .. }) {
        injections.push(inner);
        return;
    }
    // P13-S2 namespace import path: inject every value
    // decl with its original name (no `want` filter, no
    // alias rename) so the synthetic namespace object
    // can reference them. The struct literal builds
    // after the BFS drains. A name an earlier request
    // already injected under this path still claims its
    // FIELD — the object references the existing binding
    // — but must not inject a second decl of it.
    if let Some(ns) = ns
        && let Some(name) = decl_name(&inner)
    {
        // The FIELD keeps the export spelling even when the census
        // mangled the local decl (423-01 deconflict).
        let field = demangle.get(&name).cloned().unwrap_or_else(|| name.clone());
        if ns.claim(&field, &name) && injected.insert(name) {
            injections.push(inner);
        }
        return;
    }
    // 423-01 knife C — an un-wanted decl whose spelling the hidden
    // census marked still injects (mangled, or bare when the census
    // declined without a collision): some injected decl's free
    // variables reach it. Never importer-visible — no want / rename /
    // ns face applies. Checked before the want arm: a hidden spelling
    // is by construction not a wanted one.
    if let Some(name) = decl_name(&inner)
        && hidden.contains(&name)
    {
        if injected.insert(name) {
            injections.push(inner);
        }
        return;
    }
    if let Some(name) = decl_name(&inner)
        && want.contains(name.as_str())
    {
        // `want` and the visibles map are built from the same named
        // list, so a wanted name always has an entry; the bare
        // inject stays as the defensive fallback.
        let Some(visibles) = rename.get(name.as_str()) else {
            injections.push(inner);
            return;
        };
        // The original spelling, when the importer listed it plainly
        // alongside aliases (`{ fa, fa as renamed }`), keeps the
        // UN-CLONED decl: recursive self-references inside a decl
        // bind the original name (the K.2 corner), and the spelling
        // that preserves them should own the original body. With no
        // plain spelling the first alias takes it (rename in place —
        // today's single-alias behavior).
        for v in visibles {
            landings.record(v);
        }
        let first = visibles
            .iter()
            .find(|v| **v == name)
            .or_else(|| visibles.first())
            .copied()
            .unwrap_or(name.as_str())
            .to_string();
        let extras: Vec<&str> = visibles.iter().filter(|v| **v != first).copied().collect();
        // Extra spellings inject as their own decls AFTER the first.
        // A FnDecl deep-clones (a shared body would hand one ExprId
        // tree to two decls, and every stmts-walking desugar pass
        // would rewrite it twice); a TypeDecl is ExprId-free and
        // clones plainly; a LetDecl aliases by REFERENCE (`const
        // renamed = fa`) so its initializer evaluates once, per
        // §16.2.1's one-binding semantics. A ClassDecl aliases by
        // reference too (423-01 knife D1): `const K2 = K` shares the
        // ONE class identity — `new K2()` rides the non-class-new
        // NewDynamic route off the class value, instanceof included —
        // where a deep clone would silently split statics and brand.
        let mut extra_decls: Vec<Stmt> = Vec::new();
        for alias in extras {
            match &inner {
                Stmt::FnDecl { .. } => {
                    let mut copy = crate::ast::clone_toplevel_stmt(ast, &inner);
                    rename_decl(&mut copy, alias.to_string());
                    crate::ast::rename_fn_self_refs(ast, &mut copy, &name, alias);
                    copy_fn_name_tables(ast, &name, alias);
                    extra_decls.push(copy);
                }
                Stmt::TypeDecl { .. } => {
                    let mut copy = inner.clone();
                    rename_decl(&mut copy, alias.to_string());
                    extra_decls.push(copy);
                }
                Stmt::LetDecl { .. } | Stmt::ClassDecl { .. } => {
                    let init = ast.add_expr(Expr::Ident(first.clone()));
                    extra_decls.push(Stmt::LetDecl {
                        mutable: false,
                        name: alias.to_string(),
                        type_ann: None,
                        init,
                        is_var: false,
                    });
                }
                _ => {}
            }
        }
        if first != *name {
            // K.2 closure: a renamed fn's free self-references follow
            // the new spelling — `import { fact as f1 }` used to leave
            // the recursive `fact(n - 1)` reading a name not in scope.
            crate::ast::rename_fn_self_refs(ast, &mut inner, &name, &first);
            copy_fn_name_tables(ast, &name, &first);
            rename_decl(&mut inner, first);
        }
        injections.push(inner);
        injections.extend(extra_decls);
    }
}

/// P13-S4b — a top-level decl a bare named export lists injects
/// under its EXPORTED name (then the importer's own alias applies
/// inside `inject_export_inner`). Everything else is a lib-level
/// non-export statement — dropped, unless the hidden-dependency
/// census marked its spelling (423-01 knife C), in which case it
/// injects as a plain top-level decl.
#[allow(clippy::too_many_arguments)]
pub(super) fn inject_bare_exported_decl(
    ast: &mut Ast,
    injections: &mut Vec<Stmt>,
    other: Stmt,
    bare_exports: &HashMap<String, String>,
    want: &HashSet<&str>,
    rename: &HashMap<&str, Vec<&str>>,
    ns: Option<&mut NsAccum>,
    injected: &mut HashSet<String>,
    demangle: &HashMap<String, String>,
    hidden: &HashSet<String>,
    landings: &mut Landings<'_>,
) {
    if let Some(dname) = decl_name(&other)
        && let Some(exported) = bare_exports.get(&dname)
    {
        let mut inner = other;
        if *exported != dname {
            // K.2 closure — same self-reference follow as the alias
            // rename inside `inject_export_inner`.
            crate::ast::rename_fn_self_refs(ast, &mut inner, &dname, exported);
            copy_fn_name_tables(ast, &dname, exported);
            rename_decl(&mut inner, exported.clone());
        }
        inject_export_inner(
            ast, injections, inner, want, rename, ns, injected, demangle, hidden, landings,
        );
        return;
    }
    if let Some(dname) = decl_name(&other)
        && hidden.contains(&dname)
        && injected.insert(dname)
    {
        injections.push(other);
    }
}

/// r425(423-01 勘察薄刀)— the parser-filled name-keyed fn tables
/// follow a rename: `desugar_async` gates on `async_fns` by NAME, so
/// an aliased async import otherwise checks its body as a plain fn
/// (`import { af as g }` — loud ret-type mismatch where bun runs);
/// same face for the async-generator set and the generator
/// param-destr prefix count. COPY, not move: the original spelling
/// may still be injected by another lane of the same lib, and a
/// same-named entry decl is a redeclaration reject before any table
/// is consulted.
fn copy_fn_name_tables(ast: &mut Ast, old: &str, new: &str) {
    if ast.async_fns.contains(old) {
        ast.async_fns.insert(new.to_string());
    }
    if ast.async_generator_fns.contains(old) {
        ast.async_generator_fns.insert(new.to_string());
    }
    if let Some(&n) = ast.gen_param_destr_prefix.get(old) {
        ast.gen_param_destr_prefix.insert(new.to_string(), n);
    }
}
