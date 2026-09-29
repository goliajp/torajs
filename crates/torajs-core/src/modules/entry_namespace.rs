//! The entry importing itself — `import * as ns from "./self"`, its
//! namespace, and `import { x as y } from "./self"`, its own named
//! exports and default. Split from `repeat_request` at the file-size
//! line.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::ast::{Ast, Expr, Stmt};

use super::resolve_helpers::{NsAccum, queue_reexport, queue_star_reexport};
use super::static_resolution::LedgerState;
use super::{NamedImport, WorkItem, is_builtin_module_source, resolve_path};

/// The entry's own export faces, in source order: exported name → the
/// entry's own local binding. Its `export default <expr>` has no
/// binding of its own, so it becomes one here (`__nsdefault_entry`,
/// evaluated in place). An `export … from "./self"` clause naming one
/// of these faces is one more face of the same binding; a clause
/// forwarding from elsewhere is not the entry's own and is left out.
fn entry_own_faces(ast: &mut Ast, entry_path: &Path) -> Result<Vec<(String, String)>, String> {
    let mut own: Vec<(String, String)> = Vec::new();
    for s in ast.stmts.iter_mut() {
        match s {
            Stmt::ExportDecl { inner: Some(d), .. } => {
                if let Some(n) = super::inject_export::decl_name(d) {
                    own.push((n.clone(), n));
                }
            }
            Stmt::ExportDecl {
                inner: None,
                default_expr: Some(eid),
                ..
            } => {
                own.push(("default".into(), ENTRY_DEFAULT.into()));
                // `any`: the namespace literal at the program's head
                // reads it as a global cell whatever the initializer
                // (`export default null` has no slot shape to infer)
                *s = Stmt::LetDecl {
                    mutable: false,
                    name: ENTRY_DEFAULT.to_string(),
                    type_ann: Some("any".to_string()),
                    init: *eid,
                    is_var: false,
                };
            }
            Stmt::LetDecl { name, .. } if name == ENTRY_DEFAULT => {
                own.push(("default".into(), ENTRY_DEFAULT.into()));
            }
            Stmt::ExportDecl {
                inner: None,
                named,
                source: None,
                type_only: false,
                ..
            } => {
                for (local, face) in named.iter() {
                    let face = face.as_deref().unwrap_or(local);
                    own.push((face.to_string(), local.clone()));
                }
            }
            _ => {}
        }
    }
    let entry_dir = entry_path.parent().unwrap_or(entry_path);
    let mut at_self: Vec<(String, String)> = Vec::new();
    for s in &ast.stmts {
        let Stmt::ExportDecl {
            inner: None,
            named,
            star: None,
            source: Some(source),
            ..
        } = s
        else {
            continue;
        };
        if is_builtin_module_source(source) || resolve_path(entry_dir, source)? != entry_path {
            continue;
        }
        for (orig, face) in named {
            if let Some((_, local)) = own.iter().find(|(f, _)| f == orig) {
                at_self.push((face.as_deref().unwrap_or(orig).to_string(), local.clone()));
            }
        }
    }
    own.extend(at_self);
    Ok(own)
}

/// `import { x as y } from "./self"` / `import d from "./self"` — the
/// import binding IS the entry's own binding (§16.2.1 an import
/// binding is indirect), so nothing is walked or copied: every
/// top-level reference to the import spelling follows the local
/// (`follow_aliases`), and a `const y = x` right after the local's
/// declaration serves the frames the follow leaves alone. A face the
/// entry forwards from another module is not its own and stays on
/// the ordinary request path; the remaining requests come back.
pub(super) fn bind_entry_self_imports(
    ast: &mut Ast,
    entry_path: &Path,
    named: Vec<NamedImport>,
    default_alias: Option<String>,
    st: &mut super::resolve_state::ResolveState,
) -> Result<(Vec<NamedImport>, Option<String>), String> {
    let own = entry_own_faces(ast, entry_path)?;
    let local_of = |face: &str| own.iter().find(|(f, _)| f == face).map(|(_, l)| l.clone());
    let mut rest: Vec<NamedImport> = Vec::new();
    for (orig, alias) in named {
        match local_of(&orig) {
            Some(local) => bind_self(ast, st, alias.as_deref().unwrap_or(&orig), &local),
            None => rest.push((orig, alias)),
        }
    }
    let rest_default = match default_alias {
        Some(d) => match local_of("default") {
            Some(local) => {
                bind_self(ast, st, &d, &local);
                None
            }
            None => Some(d),
        },
        None => None,
    };
    Ok((rest, rest_default))
}

fn bind_self(
    ast: &mut Ast,
    st: &mut super::resolve_state::ResolveState,
    visible: &str,
    local: &str,
) {
    st.entry_self_bindings.insert(visible.to_string());
    if visible == local {
        return;
    }
    st.repeat_aliases
        .push((visible.to_string(), local.to_string()));
    let declares = |s: &Stmt| match s {
        Stmt::ExportDecl { inner: Some(d), .. } => {
            super::inject_export::decl_name(d).as_deref() == Some(local)
        }
        other => super::inject_export::decl_name(other).as_deref() == Some(local),
    };
    let Some(at) = ast.stmts.iter().position(declares) else {
        return;
    };
    let init = ast.add_expr(Expr::Ident(local.to_string()));
    ast.stmts.insert(
        at + 1,
        Stmt::LetDecl {
            mutable: false,
            name: visible.to_string(),
            type_ann: None,
            init,
            is_var: false,
        },
    );
}

/// The entry's own namespace: its fields are the entry's own
/// top-level bindings, claimed straight from its export clauses. A
/// walk of the entry would inject a second copy of every declaration
/// (mangled past the collision with the original) and point the
/// fields at the copies, so `ns.x` after `x++` still read the copy's
/// initial value. The entry's `export default <expr>` has no binding
/// of its own, so it becomes one here (`__nsdefault_entry`, evaluated
/// in place) and the `default` field names it.
///
/// An `export … from` clause of the entry names a binding of the
/// module it points at. Pointed back at the entry itself, the clause
/// is one more face of the entry's own binding (`export { local1 as
/// indirect } from "./self"`), and a star at itself adds nothing
/// (§16.2.1.6.3 — a module's own exports are already in). Pointed
/// anywhere else, the clause queues the same nested request a lib's
/// would, into this accumulator: the named faces claim now and bind
/// to what the nested load materializes; the stars queue after every
/// own claim, since a local export shadows a star's (§16.2.3).
pub(super) fn claim_entry_namespace(
    ast: &mut Ast,
    alias: &str,
    entry_path: &Path,
    tables: &mut super::ns_tables::NsTables,
    work: &mut VecDeque<WorkItem>,
    ledgers: &mut LedgerState,
) -> Result<(), String> {
    if !tables.accums.contains_key(alias) {
        tables.order.push(alias.to_string());
        tables
            .accums
            .insert(alias.to_string(), NsAccum::new(alias.to_string()));
    }
    // exported name → the entry's own local binding (an own face an
    // `export … from "./self"` clause re-spells is claimed here too)
    let faces = entry_own_faces(ast, entry_path)?;
    let own: HashMap<String, String> = faces.iter().cloned().collect();
    let acc = tables.accums.get_mut(alias).expect("inserted above");
    for (face, local) in &faces {
        acc.claim(face, local);
    }
    let entry_dir = entry_path.parent().unwrap_or(entry_path);
    let no_want: HashSet<&str> = HashSet::new();
    let no_rename: HashMap<&str, Vec<&str>> = HashMap::new();
    let no_default: Option<String> = None;
    let no_own: HashSet<String> = HashSet::new();
    let mut stars = Vec::new();
    for s in &ast.stmts {
        let Stmt::ExportDecl {
            inner: None,
            named,
            star,
            source: Some(source),
            ..
        } = s
        else {
            continue;
        };
        if is_builtin_module_source(source) {
            continue;
        }
        let at_self = resolve_path(entry_dir, source)? == entry_path;
        match star {
            Some(star) => {
                if !at_self {
                    stars.push((star, source));
                }
            }
            None if at_self => {
                for (orig, face) in named {
                    // an own face is already claimed; otherwise a face
                    // an earlier clause of the entry forwarded from
                    // elsewhere
                    if own.contains_key(orig) {
                        continue;
                    }
                    let Some((_, local)) = acc.fields().iter().find(|(f, _)| f == orig) else {
                        return Err(format!(
                            "module resolution failure (SyntaxError): `{orig}` re-exported \
                             from the entry module is not one of its exports"
                        ));
                    };
                    let local = local.clone();
                    acc.claim(face.as_deref().unwrap_or(orig), &local);
                }
            }
            None => queue_reexport(
                work,
                entry_dir,
                named,
                source,
                &no_want,
                &no_rename,
                &no_default,
                Some(&mut *acc),
                &mut ledgers.ledger(entry_path),
            )?,
        }
    }
    for (star, source) in stars {
        queue_star_reexport(
            work,
            entry_dir,
            star,
            source,
            &no_want,
            &no_rename,
            &no_default,
            &no_own,
            Some(&mut *acc),
        )?;
    }
    Ok(())
}

/// The binding the entry's `export default <expr>` becomes when the
/// entry imports its own namespace (`claim_entry_namespace`).
pub(super) const ENTRY_DEFAULT: &str = "__nsdefault_entry";
