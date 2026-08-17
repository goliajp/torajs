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

/// Census + rename over one freshly-parsed lib section. Returns the
/// mangled→original map for the namespace accumulator's field
/// spellings. `prior` is this path's mangle memory from an earlier
/// request (the same file re-parses per request; the same decl must
/// mangle to the same spelling so the injected-name ledger keeps
/// deduplicating). Errs when a NAMED request asks for a spelling an
/// earlier visit already mangled — the importer-visible binding
/// would collide with whatever forced the mangle, and a loud reject
/// beats a silently split identity.
///
/// 423-01 knife C — a name in `hidden` mangles UNCONDITIONALLY (it
/// must land, but never importer-visible), and its memory goes into
/// `hidden_prior`, NOT `prior`: a later plain `import { util }` of a
/// hidden-mangled EXPORT must keep today's bare re-injection path,
/// not trip the requested-collision reject above. `hidden_inject`
/// collects the spellings the walk should inject for the hidden set
/// (the mangle, or the original name when the census declined on a
/// rebind with no collision — no rewrite happens, so the references
/// already match).
#[allow(clippy::too_many_arguments)]
pub(super) fn deconflict_lib_section(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    current_path: &std::path::Path,
    seed: &SeedNames,
    requested: &HashSet<&str>,
    hidden: &HashSet<String>,
    bare_exports: &mut HashMap<String, String>,
    prior: &mut HashMap<String, String>,
    hidden_prior: &mut HashMap<String, String>,
    mangle_seq: &mut usize,
    hidden_inject: &mut HashSet<String>,
    delta: &class_rename::LibTableDelta,
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
        // Parser synthetics that bake a class name into their OWN
        // decl name (`__ccmk_<C>_<n>` computed-key hoists,
        // `__cm_gen_<C>__<m>` generator forwarders) are not census
        // candidates — they follow their class's rename inside
        // `rename_class_artifacts` (which also re-keys the entry
        // this inserts), and mangling one independently would break
        // the name-derived consumers. A hidden one still injects.
        if name.starts_with("__ccmk_") || name.starts_with("__cm_gen_") {
            if hidden.contains(&name) {
                hidden_inject.insert(name);
            }
            continue;
        }
        let is_class = is_class_decl(&lib_section[i]);
        // A hidden dep that happens to share the requested spelling is
        // an import of a NON-export (the request can't be satisfied
        // either way, staying loud) — the dep's mangle wins.
        let is_hidden = hidden.contains(&name);
        // A bare-exported decl (`const a = …; export { a as b }`)
        // injects under its FACE (P13-S4b renames it), so the face is
        // its collision surface — the decl's own spelling never lands.
        let bare_face = bare_exports.get(&name).cloned();
        let surface = bare_face.as_deref().unwrap_or(name.as_str());
        if requested.contains(surface) && !is_hidden {
            if prior.contains_key(&name) {
                return Err(format!(
                    "import name `{surface}` collides with a same-named export of another \
                     module; import it through a namespace (`import * as ns`) instead"
                ));
            }
            hoist_face_rename(
                ast,
                lib_section,
                lib_expr_offset,
                i,
                &name,
                &bare_face,
                bare_exports,
                &mut mangles,
                delta,
                hidden,
                hidden_inject,
            );
            continue;
        }
        let prior_mangle = prior
            .get(&name)
            .or_else(|| hidden_prior.get(&name))
            .cloned();
        let collides = seed.hard.contains(surface)
            || seed
                .reserved
                .get(surface)
                .is_some_and(|p| p != current_path);
        if prior_mangle.is_none() && !collides && !is_hidden {
            hoist_face_rename(
                ast,
                lib_section,
                lib_expr_offset,
                i,
                &name,
                &bare_face,
                bare_exports,
                &mut mangles,
                delta,
                hidden,
                hidden_inject,
            );
            continue;
        }
        if rebinds_elsewhere(ast, lib_section, lib_expr_offset, i, &name)
            || (is_class && class_rename::type_param_shadows(lib_section, &name))
        {
            // decline — keeps the loud redeclaration reject. A hidden
            // dep still injects under its own spelling when nothing
            // collides; a colliding one keeps the loud unknown.
            if is_hidden && !collides && prior_mangle.is_none() {
                hidden_inject.insert(name);
            }
            continue;
        }
        let mangled = prior_mangle.unwrap_or_else(|| {
            *mangle_seq += 1;
            format!("__m{}_{name}", *mangle_seq)
        });
        rename_top_decl(&mut lib_section[i], &mangled);
        copy_fn_name_tables(ast, &name, &mangled);
        if is_class {
            class_rename::rename_class_artifacts(
                ast,
                lib_section,
                lib_expr_offset,
                i,
                &name,
                &mangled,
                delta,
                hidden,
                hidden_inject,
            );
        }
        if is_hidden {
            hidden_inject.insert(mangled.clone());
        }
        if collides || prior.contains_key(&name) {
            prior.insert(name.clone(), mangled.clone());
        } else {
            hidden_prior.insert(name.clone(), mangled.clone());
        }
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
    // binding follows it (self-references included). `New` names its
    // class in a String field the Ident arm can't see (knife D — and
    // the parser also mints `Expr::New` for `new f()` over a plain
    // fn, so the arm applies to every mangle, not just classes).
    for e in ast.exprs[lib_expr_offset..].iter_mut() {
        match e {
            Expr::Ident(n) => {
                if let Some(m) = mangles.get(n) {
                    *n = m.clone();
                }
            }
            Expr::New { class_name, .. } => {
                if let Some(m) = mangles.get(class_name) {
                    *class_name = m.clone();
                }
            }
            _ => {}
        }
    }
    Ok(demangle)
}

/// 427-01 — the P13-S4b face rename (`const a = …; export { a as b }`
/// injects under `b`), hoisted from walk time into the census: the
/// walk-time rename only followed a fn's self-references, so a
/// SIBLING decl reading the original spelling (`export const c =
/// a * 2`) broke. Routing the rename through the census's arena
/// rewrite follows every reference; the re-keyed map (value == decl
/// name) makes the walk skip its shallow rename. Declines — keeping
/// the walk-time behavior — when the lib rebinds either spelling
/// anywhere (blind-rewrite soundness on the old name; a rebound FACE
/// would capture the rewritten references).
#[allow(clippy::too_many_arguments)]
fn hoist_face_rename(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    i: usize,
    name: &str,
    bare_face: &Option<String>,
    bare_exports: &mut HashMap<String, String>,
    mangles: &mut HashMap<String, String>,
    delta: &class_rename::LibTableDelta,
    hidden: &HashSet<String>,
    hidden_inject: &mut HashSet<String>,
) {
    let Some(face) = bare_face else { return };
    if face == name
        || rebinds_elsewhere(ast, lib_section, lib_expr_offset, i, name)
        || rebinds_elsewhere(ast, lib_section, lib_expr_offset, i, face)
        || (is_class_decl(&lib_section[i]) && class_rename::type_param_shadows(lib_section, name))
    {
        return;
    }
    rename_top_decl(&mut lib_section[i], face);
    copy_fn_name_tables(ast, name, face);
    if is_class_decl(&lib_section[i]) {
        // A face rename IS a rename — the baked artifacts move the
        // same way a mangle moves them (knife D).
        class_rename::rename_class_artifacts(
            ast,
            lib_section,
            lib_expr_offset,
            i,
            name,
            face,
            delta,
            hidden,
            hidden_inject,
        );
    }
    bare_exports.remove(name);
    bare_exports.insert(face.clone(), face.clone());
    mangles.insert(name.to_string(), face.clone());
}

/// The FnDecl / LetDecl / ClassDecl name a top-level lib statement
/// declares (through the `export` wrapper). Type decls answer None.
/// Knife D admits ClassDecl: the census renames a class through
/// `class_rename::rename_class_artifacts`, which moves every baked
/// artifact with the decl name.
pub(super) fn top_value_decl_name(s: &Stmt) -> Option<String> {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => top_value_decl_name(inner),
        Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
            Some(name.clone())
        }
        _ => None,
    }
}

fn is_class_decl(s: &Stmt) -> bool {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => is_class_decl(inner),
        Stmt::ClassDecl { .. } => true,
        _ => false,
    }
}

/// Point the declaration at its mangled name (through the `export`
/// wrapper). For a ClassDecl this is only the NAME FIELD — the
/// caller must follow with `rename_class_artifacts`, which owns the
/// baked-artifact move (never rename a class through this alone).
fn rename_top_decl(s: &mut Stmt, mangled: &str) {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => rename_top_decl(inner, mangled),
        Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
            *name = mangled.to_string()
        }
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

mod seed;
pub(super) use seed::{SeedNames, extend_seen_with_lib, seed_seen_names};
mod class_rename;
pub(super) use class_rename::{LibTableDelta, diff_class_tables, snapshot_class_tables};
mod hidden;
pub(super) use hidden::LaneShape;

/// The per-request prep between "lib section drained" and "walk":
/// the importer's want / spelling maps, the lib's export faces (user
/// spellings — collected before any rename), then the 423-01
/// deconflict census over the colliding unrequested decl names. The
/// demangle map lets the namespace accumulator claim fields by the
/// export spelling while referencing the mangled binding. The last
/// tuple slot is the hidden-dependency injection set (knife C).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn prep_lib_request<'a>(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    current_path: &std::path::Path,
    named: &'a [super::NamedImport],
    seed: &mut SeedNames,
    prior_mangles: &mut HashMap<String, String>,
    hidden_mangles: &mut HashMap<String, String>,
    mangle_seq: &mut usize,
    lane: LaneShape,
    delta: &LibTableDelta,
) -> Result<
    (
        HashSet<&'a str>,
        HashMap<&'a str, Vec<&'a str>>,
        HashMap<String, String>,
        HashSet<String>,
        HashMap<String, String>,
        HashSet<String>,
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
    let hidden = if lane.side_effect_only {
        HashSet::new()
    } else {
        hidden::hidden_injection_closure(ast, lib_section, &want, &bare_exports, &lane)
    };
    // The census compares each decl's injected SURFACE (a bare
    // export's face, the decl name otherwise) against this set — an
    // importer-requested spelling lands importer-visible on purpose.
    let requested: HashSet<&str> = want.iter().copied().collect();
    let mut hidden_inject: HashSet<String> = HashSet::new();
    let demangle = deconflict_lib_section(
        ast,
        lib_section,
        lib_expr_offset,
        current_path,
        seed,
        &requested,
        &hidden,
        &mut bare_exports,
        prior_mangles,
        hidden_mangles,
        mangle_seq,
        &mut hidden_inject,
        delta,
    )?;
    extend_seen_with_lib(lib_section, &bare_exports, seed);
    Ok((
        want,
        rename,
        bare_exports,
        own_exports,
        demangle,
        hidden_inject,
    ))
}
