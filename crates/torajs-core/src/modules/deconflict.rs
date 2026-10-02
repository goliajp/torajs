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
//! `Expr::Ident` in that slice. A lib that rebinds the name anywhere
//! (a fn param, a body `let`, an arrow) takes the site rewrite in
//! `refs.rs` instead — only references the free-variable walk
//! resolves to the top level move; a class keeps the decline there.
//!
//! Class / type decls are out of this knife's scope (knife C/D —
//! `__priv_<C>__` strings bake the class name at parse time); their
//! collisions keep the loud reject.

use crate::ast::{Ast, Stmt};
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
/// `(orig → mangled, [(mangled, field spelling)])`.
type RenamePlan = (HashMap<String, String>, Vec<(String, String)>);

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
    landed: &HashMap<String, String>,
    mangle_seq: &mut usize,
    hidden_inject: &mut HashSet<String>,
    delta: &class_rename::LibTableDelta,
    visibles: &HashMap<&str, Vec<&str>>,
) -> Result<RenamePlan, String> {
    // orig → mangled (drives the arena rewrite) and mangled → FIELD
    // spelling (what the namespace accumulator shows the importer —
    // the export FACE for a bare export, the decl name otherwise).
    let mut mangles: HashMap<String, String> = HashMap::new();
    let mut demangle: HashMap<String, String> = HashMap::new();
    // orig → mangled for spellings the lib rebinds somewhere: those
    // move by resolved site, not by arena scan
    let mut site_mangles: HashMap<String, String> = HashMap::new();
    let mut requested_renames: Vec<(String, String)> = Vec::new();
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
            settle_requested_landing(prior, landed, seed, &name, surface)?;
            if is_class
                && bare_face.is_none()
                && class_rename::try_alias_rename(
                    ast,
                    lib_section,
                    lib_expr_offset,
                    i,
                    &name,
                    visibles,
                    delta,
                    hidden,
                    hidden_inject,
                    &mut mangles,
                    &mut requested_renames,
                )
            {
                continue;
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
        // A rebound spelling takes the site rewrite (`refs.rs`); a
        // class declines there, since its spelling is baked into
        // artifacts the site walk does not reach.
        let rebinds = rebinds_elsewhere(ast, lib_section, lib_expr_offset, i, &name);
        if (is_class && (rebinds || class_rename::type_param_shadows(lib_section, &name)))
            || redeclared_at_top(lib_section, i, &name)
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
        if rebinds {
            site_mangles.insert(name, mangled);
        } else {
            mangles.insert(name, mangled);
        }
    }
    if mangles.is_empty() && site_mangles.is_empty() {
        return Ok((HashMap::new(), requested_renames));
    }
    rewrite_lib_refs(ast, lib_expr_offset, &mangles);
    rewrite_lib_ref_sites(ast, lib_section, lib_expr_offset, &site_mangles);
    Ok((demangle, requested_renames))
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

mod decl_shape;
pub(super) use decl_shape::{is_class_decl, rename_top_decl, top_value_decl_name};
mod seed;
pub(super) use seed::{SeedNames, extend_seen_with_lib, seed_seen_names};
mod alias_rename;
mod class_rename;
pub(super) use class_rename::{
    LibTableDelta, diff_class_tables, snapshot_class_tables, type_param_shadows,
};
mod hidden;
pub(super) use hidden::LaneShape;
mod refs;
pub(super) use refs::rebinds_elsewhere;
use refs::{redeclared_at_top, rewrite_lib_ref_sites, rewrite_lib_refs};
mod requested;
use requested::settle_requested_landing;
mod prep;
pub(super) use prep::prep_lib_request;
