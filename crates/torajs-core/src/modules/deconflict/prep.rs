//! The per-request prep of a lib section: the importer's want and
//! spelling maps, the lib's export faces, and the deconflict census
//! over them, in the order the walk needs them.

use super::{
    LaneShape, LibTableDelta, SeedNames, alias_rename, deconflict_lib_section,
    extend_seen_with_lib, hidden,
};
use crate::ast::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

/// The per-request prep between "lib section drained" and "walk":
/// the importer's want / spelling maps, the lib's export faces (user
/// spellings — collected before any rename), then the 423-01
/// deconflict census over the colliding unrequested decl names. The
/// demangle map lets the namespace accumulator claim fields by the
/// export spelling while referencing the mangled binding. The last two
/// tuple slots are the hidden-dependency injection set (knife C) and
/// the other faces of each bare export, keyed by its primary face.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(in crate::modules) fn prep_lib_request<'a>(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    current_path: &std::path::Path,
    named: &'a [super::super::NamedImport],
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
        HashMap<String, Vec<String>>,
    ),
    String,
> {
    let mut want: HashSet<&str> = named.iter().map(|(n, _)| n.as_str()).collect();
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
    let (mut bare_exports, other_faces) = super::super::export_faces::collect_bare_exports(
        lib_section,
        &want,
        lane.has_default_alias,
    );
    // Every face of one bare-exported decl names the same binding: the
    // decl lands once under its primary face, which is the first one
    // the importer names, so the spellings asked for through the
    // others join that face's.
    for (primary, others) in &other_faces {
        let Some(&face) = want.get(primary.as_str()) else {
            continue;
        };
        for other in others {
            if want.remove(other.as_str())
                && let Some(visibles) = rename.remove(other.as_str())
            {
                let entry = rename.entry(face).or_default();
                for v in visibles {
                    if !entry.contains(&v) {
                        entry.push(v);
                    }
                }
            }
        }
    }
    let own_exports = super::super::export_faces::collect_own_export_names(lib_section);
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
    let (demangle, mut requested_renames) = deconflict_lib_section(
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
        &rename,
    )?;
    requested_renames.extend(alias_rename::rename_single_aliases(
        ast,
        lib_section,
        lib_expr_offset,
        &rename,
        &mut bare_exports,
        delta,
        &hidden,
        &mut hidden_inject,
    ));
    // 427-02 — a single-aliased class was renamed IN the census, so
    // the walk must recognize the decl under its alias: re-point the
    // want / visibles faces at the alias spelling (borrowed from the
    // same `named` clause the maps were built from).
    for (orig, alias) in &requested_renames {
        want.remove(orig.as_str());
        rename.remove(orig.as_str());
        if let Some((_, Some(a))) = named
            .iter()
            .find(|(o, a)| o == orig && a.as_deref() == Some(alias.as_str()))
        {
            want.insert(a.as_str());
            rename.insert(a.as_str(), vec![a.as_str()]);
        }
    }
    extend_seen_with_lib(lib_section, &bare_exports, seed);
    Ok((
        want,
        rename,
        bare_exports,
        own_exports,
        demangle,
        hidden_inject,
        other_faces,
    ))
}
