//! The names of the optional `number` members whose slot is an f64
//! (`ssa_lower_parse_type::parse_struct_field_type`): a literal that
//! leaves one out, or spells it `undefined`, puts the sentinel in the
//! slot, so a read of the member may answer it. The names seed the
//! sentinel field table (`sentinel_tables`).

use super::Analysis;
use crate::ast::{PropKey, Stmt};
use crate::check::Type;
use std::collections::HashSet;

/// By name, like the rest of the sentinel tables: a same-named member
/// of another type costs one predictable compare at the consumer.
///
/// Two sources, because the two spellings of an object type keep the
/// optional flag in two places: a declaration records it on the AST,
/// an inline object type (`p: { a?: number }`) only in the checker's
/// struct type, which is where every struct type that reaches an
/// expression ends up anyway.
pub(super) fn collect(a: &Analysis<'_>) -> HashSet<PropKey> {
    let mut out = HashSet::new();
    for stmt in &a.ast.stmts {
        let Stmt::TypeDecl { name, fields, .. } = stmt else {
            continue;
        };
        for (k, ann) in fields {
            if ann == crate::ssa_lower_parse_type::OPTIONAL_NUMBER_ANN
                && a.ast
                    .optional_plain_members
                    .contains(&(name.clone(), k.clone()))
            {
                out.insert(k.clone());
            }
        }
    }
    for t in a.expr_types.values() {
        walk(t, &mut out);
    }
    out
}

fn walk(t: &Type, out: &mut HashSet<PropKey>) {
    match t {
        Type::Struct(fs) => {
            for (k, ft) in fs.iter() {
                if fs.is_optional(k) && matches!(ft, Type::Nullable(n) if **n == Type::Number) {
                    out.insert(k.clone());
                }
                walk(ft, out);
            }
        }
        Type::Array(t) | Type::Nullable(t) | Type::Promise(t) | Type::Rest(t) => walk(t, out),
        Type::Map(k, v) => {
            walk(k, out);
            walk(v, out);
        }
        Type::Function(ps, r) => {
            for p in ps {
                walk(p, out);
            }
            walk(r, out);
        }
        _ => {}
    }
}
