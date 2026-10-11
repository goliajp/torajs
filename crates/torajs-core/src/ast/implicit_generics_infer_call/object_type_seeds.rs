//! The named object types the return sniff reads a field off, spelled
//! as the inline object types they name.

use std::collections::HashMap;

use crate::ast::scalar_alias_inline::Claims;
use crate::ast::{Ast, Stmt};

/// Each top-level non-generic object type (`type X = { … }`,
/// `interface X { … }`) whose name means that one type everywhere,
/// spelled as the inline object type it names (`__inlobj(f:ann|…)`)
/// and keyed by [`object_type_key`]: the return sniff reads a field
/// off a parameter of that type through it. A class is not one: its
/// instances are not the object literals the spelling describes. An
/// interface with heritage rows or a non-string key is left out.
pub(crate) fn object_type_seeds(ast: &Ast) -> HashMap<String, String> {
    let claims = Claims::collect(ast);
    let mut out = HashMap::new();
    for s in &ast.stmts {
        let s = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner.as_ref(),
            s => s,
        };
        let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
        else {
            continue;
        };
        if !type_params.is_empty()
            || ast.class_parents.contains_key(name)
            || claims.type_decls.get(name).copied().unwrap_or(0) != 1
            || claims.type_params.contains(name)
            || claims.values.contains(name)
        {
            continue;
        }
        let rows: Option<Vec<String>> = fields
            .iter()
            .map(|(k, ann)| {
                let k = k.as_str()?;
                (k != "__alias__" && k != crate::ast::interface_extends::EXTENDS_FIELD)
                    .then(|| format!("{k}:{ann}"))
            })
            .collect();
        if let Some(rows) = rows
            && !rows.is_empty()
        {
            out.insert(
                object_type_key(name),
                format!("__inlobj({})", rows.join("|")),
            );
        }
    }
    out
}

/// The seed key a named object type's inline spelling rides under in
/// the return sniff's binds; no binding or `this.<field>` key has a
/// space in it.
pub(crate) fn object_type_key(name: &str) -> String {
    format!("type {name}")
}
