//! The annotations a type alias stands for: an array alias's element,
//! and the annotation an alias chain ends at.

use crate::ast::{Ast, Stmt};
use crate::check_type_ann::strip_nullable;

/// The element annotation of the array annotation `ann` (`T[]`,
/// `Array<T>` or `ReadonlyArray<T>`, either one `| undefined`, or a
/// type alias of one).
pub(crate) fn elem_ann(ast: &Ast, ann: &str) -> Option<String> {
    let ann = unalias(ast, ann);
    let elem = match ann.strip_suffix("[]") {
        Some(e) => e,
        None => (ann.strip_prefix("Array<"))
            .or_else(|| ann.strip_prefix("ReadonlyArray<"))?
            .strip_suffix('>')?,
    };
    Some(elem.trim().to_string())
}

/// The annotation `ann` spells, `| null` / `| undefined` stripped and
/// every non-generic type alias it is followed to (`type M = L`).
pub(crate) fn unalias<'a>(ast: &'a Ast, ann: &'a str) -> &'a str {
    let mut ann = strip_nullable(ann).trim();
    let mut seen: Vec<&str> = Vec::new();
    while let Some(target) = alias_target(ast, ann)
        && !seen.contains(&ann)
    {
        seen.push(ann);
        ann = strip_nullable(target).trim();
    }
    ann
}

/// The annotation the non-generic type alias `name` stands for
/// (`type L = T[]`).
fn alias_target<'a>(ast: &'a Ast, name: &str) -> Option<&'a str> {
    if !(name.bytes()).all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$') {
        return None;
    }
    ast.stmts.iter().find_map(|s| match s {
        Stmt::TypeDecl {
            name: n,
            type_params,
            fields,
        } if n == name && type_params.is_empty() => match fields.as_slice() {
            [(k, target)] if *k == "__alias__" => Some(target.as_str()),
            _ => None,
        },
        _ => None,
    })
}
