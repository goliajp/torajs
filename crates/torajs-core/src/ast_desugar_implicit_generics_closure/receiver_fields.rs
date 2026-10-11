//! The class field rows a `__this`-shaped method's return sniff reads.

use crate::ast::{Param, PropKey, Stmt};

/// r502 — every class the class desugar flattened into a struct
/// decl, by name: a `__this`-shaped method's return sniff resolves
/// `this.<field>` against its receiver's row.
pub(crate) fn receiver_field_rows(
    stmts: &[Stmt],
) -> std::collections::HashMap<String, Vec<(PropKey, String)>> {
    stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::TypeDecl { name, fields, .. } => Some((name.clone(), fields.clone())),
            _ => None,
        })
        .collect()
}

/// The receiver's field rows for a `__this`-first fn whose receiver
/// ann is a plain class name (`C`, not `C<T>` / `any`).
pub(super) fn receiver_fields_of<'a>(
    params: &[Param],
    receiver_fields: &'a std::collections::HashMap<String, Vec<(PropKey, String)>>,
) -> Option<&'a Vec<(PropKey, String)>> {
    let ann = params.first()?.type_ann.as_deref()?;
    if ann.contains('<') {
        return None;
    }
    receiver_fields.get(ann)
}
