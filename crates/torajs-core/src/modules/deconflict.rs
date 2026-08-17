//! 423-01 — per-module deconflict (RFC 20260817-per-module-scope,
//! knife A).
//!
//! The resolver injects lib decls into the entry's flat top level
//! under their user spelling, so two modules exporting the same name
//! collide (`redeclaration` reject). This census renames the
//! COLLIDING decls only — scope-hoisting bundlers' deconflict, not a
//! blanket prefix — to `__m<k>_<name>`: `.name` reflection strips a
//! known mangle shape, diagnostics on a missed reference stay hard
//! (`__` reserve), and every non-colliding program keeps its exact
//! current names.
//!
//! Only a decl the importer did NOT request by name is eligible: a
//! requested spelling lands in the importer's scope on purpose, so a
//! collision there is the program's own duplicate binding and stays
//! loud. Namespace / side-effect lanes are where unrequested decls
//! flood the top level; their importer face goes through the
//! namespace object's FIELDS, which keep the export spelling (the
//! accumulator stores field/local pairs).
//!
//! Reference rewrite is an arena-slice scan: `parse_into` appends the
//! lib's expressions after `lib_expr_offset`, entry and lib never
//! share ExprIds, and every reference at any nesting depth is an
//! `Expr::Ident` in that slice. Shadowing is handled by DECLINE, not
//! scope tracking: a lib that rebinds the name anywhere (a fn param,
//! a body `let`, an arrow) keeps today's loud redeclaration reject —
//! never a silently mis-scoped rewrite (the
//! `desugar_generators_alpha` posture).
//!
//! Class / type decls are out of this knife's scope (knife C/D —
//! `__priv_<C>__` strings bake the class name at parse time); their
//! collisions keep the loud reject.

use crate::ast::{Ast, Expr, Stmt};
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
pub(super) struct SeedNames {
    pub(super) hard: HashSet<String>,
    pub(super) reserved: HashMap<String, std::path::PathBuf>,
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

pub(super) fn seed_seen_names(ast: &Ast, base_dir: &std::path::Path) -> SeedNames {
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
                let path = super::resolve_path(base_dir, source).ok();
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
                if let Some(n) = super::decl_name(inner) {
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
pub(super) fn extend_seen_with_lib(
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
        if let Some(n) = super::decl_name(inner) {
            let injected = bare_exports.get(&n).cloned().unwrap_or(n);
            seed.reserved.remove(&injected);
            seed.hard.insert(injected);
        }
    }
}

/// Census + rename over one freshly-parsed lib section. Returns the
/// mangled→original map for the namespace accumulator's field
/// spellings. `prior` is this path's mangle memory from an earlier
/// request (the same file re-parses per request; the same decl must
/// mangle to the same spelling so the injected-name ledger keeps
/// deduplicating). Errs when a NAMED request asks for a spelling an
/// earlier visit already mangled — the importer-visible binding
/// would collide with whatever forced the mangle, and a loud reject
/// beats a silently split identity.
pub(super) fn deconflict_lib_section(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    current_path: &std::path::Path,
    seed: &SeedNames,
    requested: &HashSet<&str>,
    bare_exports: &mut HashMap<String, String>,
    prior: &mut HashMap<String, String>,
    mangle_seq: &mut usize,
) -> Result<HashMap<String, String>, String> {
    // orig → mangled (drives the arena rewrite) and mangled → FIELD
    // spelling (what the namespace accumulator shows the importer —
    // the export FACE for a bare export, the decl name otherwise).
    let mut mangles: HashMap<String, String> = HashMap::new();
    let mut demangle: HashMap<String, String> = HashMap::new();
    for i in 0..lib_section.len() {
        let Some(name) = top_value_decl_name(&lib_section[i]) else {
            continue;
        };
        // A bare-exported decl (`const a = …; export { a as b }`)
        // injects under its FACE (P13-S4b renames it), so the face is
        // its collision surface — the decl's own spelling never lands.
        let bare_face = bare_exports.get(&name).cloned();
        let surface = bare_face.as_deref().unwrap_or(name.as_str());
        if requested.contains(surface) {
            if prior.contains_key(&name) {
                return Err(format!(
                    "import name `{surface}` collides with a same-named export of another \
                     module; import it through a namespace (`import * as ns`) instead"
                ));
            }
            continue;
        }
        let prior_mangle = prior.get(&name).cloned();
        let collides = seed.hard.contains(surface)
            || seed
                .reserved
                .get(surface)
                .is_some_and(|p| p != current_path);
        if prior_mangle.is_none() && !collides {
            continue;
        }
        if rebinds_elsewhere(ast, lib_section, lib_expr_offset, i, &name) {
            continue; // decline — keeps the loud redeclaration reject
        }
        let mangled = prior_mangle.unwrap_or_else(|| {
            *mangle_seq += 1;
            format!("__m{}_{name}", *mangle_seq)
        });
        rename_top_decl(&mut lib_section[i], &mangled);
        copy_fn_name_tables(ast, &name, &mangled);
        prior.insert(name.clone(), mangled.clone());
        if let Some(face) = bare_face {
            // Re-key so the injection recognizes the mangled decl as
            // bare-exported but keeps its spelling (the map's value ==
            // decl name skips the face rename); the FIELD still shows
            // the face.
            bare_exports.remove(&name);
            bare_exports.insert(mangled.clone(), mangled.clone());
            demangle.insert(mangled.clone(), face);
        } else {
            demangle.insert(mangled.clone(), name.clone());
        }
        mangles.insert(name, mangled);
    }
    if mangles.is_empty() {
        return Ok(HashMap::new());
    }
    // Reference rewrite — every lib expression lives in the appended
    // arena slice; a bare Ident there referencing a mangled top-level
    // binding follows it (self-references included).
    for e in ast.exprs[lib_expr_offset..].iter_mut() {
        if let Expr::Ident(n) = e
            && let Some(m) = mangles.get(n)
        {
            *e = Expr::Ident(m.clone());
        }
    }
    Ok(demangle)
}

/// The FnDecl / LetDecl name a top-level lib statement declares
/// (through the `export` wrapper). Class / type decls answer None —
/// out of knife A's scope.
pub(super) fn top_value_decl_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => top_value_decl_name(inner),
        Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Point the declaration at its mangled name (through the `export`
/// wrapper).
fn rename_top_decl(s: &mut Stmt, mangled: &str) {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => rename_top_decl(inner, mangled),
        Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } => *name = mangled.to_string(),
        _ => {}
    }
}

/// Does any OTHER statement of the lib — or any arrow value in the
/// lib's arena slice — rebind `name`? The blind arena rewrite is only
/// sound when nothing shadows the top-level binding; a rebinding
/// declines the mangle (module doc).
pub(super) fn rebinds_elsewhere(
    ast: &Ast,
    lib_section: &[Stmt],
    lib_expr_offset: usize,
    decl_idx: usize,
    name: &str,
) -> bool {
    let stmt_rebinds = lib_section.iter().enumerate().any(|(j, s)| {
        if j == decl_idx {
            return false;
        }
        let inner = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner,
            other => other,
        };
        crate::ast::rebinds_in_stmt(inner, name)
    });
    if stmt_rebinds {
        return true;
    }
    ast.exprs[lib_expr_offset..].iter().any(|e| {
        matches!(e, Expr::ArrowFn { params, body, .. }
            if params.iter().any(|p| p.name == name)
                || body.iter().any(|s| crate::ast::rebinds_in_stmt(s, name)))
    })
}

/// COPY the parser-filled name-keyed fn tables onto the mangle. Not
/// a move: the original spelling may belong to the ENTRY's same-named
/// fn (the very collision that forced this mangle) or re-appear when
/// the lib re-parses for a later request; a leftover entry with no
/// matching decl is a no-op for every consumer.
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

/// The per-request prep between "lib section drained" and "walk":
/// the importer's want / spelling maps, the lib's export faces (user
/// spellings — collected before any rename), then the 423-01
/// deconflict census over the colliding unrequested decl names. The
/// demangle map lets the namespace accumulator claim fields by the
/// export spelling while referencing the mangled binding.
#[allow(clippy::type_complexity)]
pub(super) fn prep_lib_request<'a>(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    current_path: &std::path::Path,
    named: &'a [super::NamedImport],
    seed: &mut SeedNames,
    prior_mangles: &mut HashMap<String, String>,
    mangle_seq: &mut usize,
) -> Result<
    (
        HashSet<&'a str>,
        HashMap<&'a str, Vec<&'a str>>,
        HashMap<String, String>,
        HashSet<String>,
        HashMap<String, String>,
    ),
    String,
> {
    let want: HashSet<&str> = named.iter().map(|(n, _)| n.as_str()).collect();
    // orig → every importer-visible spelling, in clause order
    // (421-04: `import { fa, fa as renamed }` binds BOTH names —
    // a single-alias map collapsed them to the last one).
    let mut rename: HashMap<&str, Vec<&str>> = HashMap::new();
    for (orig, alias) in named {
        let visible = alias.as_deref().unwrap_or(orig.as_str());
        let entry = rename.entry(orig.as_str()).or_default();
        if !entry.contains(&visible) {
            entry.push(visible);
        }
    }
    let mut bare_exports = super::resolve_helpers::collect_bare_exports(lib_section);
    let own_exports = super::resolve_helpers::collect_own_export_names(lib_section);
    // The census compares each decl's injected SURFACE (a bare
    // export's face, the decl name otherwise) against this set — an
    // importer-requested spelling lands importer-visible on purpose.
    let requested: HashSet<&str> = want.iter().copied().collect();
    let demangle = deconflict_lib_section(
        ast,
        lib_section,
        lib_expr_offset,
        current_path,
        seed,
        &requested,
        &mut bare_exports,
        prior_mangles,
        mangle_seq,
    )?;
    extend_seen_with_lib(lib_section, &bare_exports, seed);
    Ok((want, rename, bare_exports, own_exports, demangle))
}
