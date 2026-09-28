//! `import * as ns from "./self"` — the entry importing its own
//! namespace. Split from `repeat_request` at the file-size line.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::ast::{Ast, Stmt};

use super::resolve_helpers::{NsAccum, queue_reexport, queue_star_reexport};
use super::static_resolution::LedgerState;
use super::{WorkItem, is_builtin_module_source, resolve_path};

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
    let acc = tables.accums.get_mut(alias).expect("inserted above");
    // exported name → the entry's own local binding
    let mut own: HashMap<String, String> = HashMap::new();
    for s in ast.stmts.iter_mut() {
        match s {
            Stmt::ExportDecl { inner: Some(d), .. } => {
                if let Some(n) = super::inject_export::decl_name(d) {
                    acc.claim(&n, &n);
                    own.insert(n.clone(), n);
                }
            }
            Stmt::ExportDecl {
                inner: None,
                default_expr: Some(eid),
                ..
            } => {
                acc.claim("default", ENTRY_DEFAULT);
                own.insert("default".into(), ENTRY_DEFAULT.into());
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
                acc.claim("default", ENTRY_DEFAULT);
                own.insert("default".into(), ENTRY_DEFAULT.into());
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
                    acc.claim(face, local);
                    own.insert(face.to_string(), local.clone());
                }
            }
            _ => {}
        }
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
                    // an own binding, or a face an earlier clause of
                    // the entry already forwarded from elsewhere
                    let local = match own.get(orig) {
                        Some(l) => l.clone(),
                        None => match acc.fields().iter().find(|(f, _)| f == orig) {
                            Some((_, l)) => l.clone(),
                            None => {
                                return Err(format!(
                                    "module resolution failure (SyntaxError): `{orig}` re-exported \
                                     from the entry module is not one of its exports"
                                ));
                            }
                        },
                    };
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
