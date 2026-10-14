//! The names of the optional `number` members whose slot is an f64
//! (`ssa_lower_parse_type::parse_struct_field_type`): a literal that
//! leaves one out, or spells it `undefined`, puts the sentinel in the
//! slot, so a read of the member may answer it. The names seed the
//! sentinel field table (`sentinel_tables`).

use super::Analysis;
use crate::ast::{Ast, ExprId, PropKey, Stmt};
use crate::check::Type;
use std::collections::{HashMap, HashSet};

/// By name, like the rest of the sentinel tables: a same-named member
/// of another type costs one predictable compare at the consumer. The
/// shadow census (`ast::named_prop_census::shadow_names`) gathers them
/// ([`names`]) before the width analysis runs, and keeps them on the
/// AST.
pub(super) fn collect(a: &Analysis<'_>) -> HashSet<PropKey> {
    a.ast.accessor_field_names.optional_number_members().clone()
}

/// The names over the checked program's two tables. Two sources,
/// because the two spellings of an object type keep the optional flag
/// in two places: a declaration records it on the AST, an inline object
/// type (`p: { a?: number }`) only in the checker's struct type, which
/// is where every struct type that reaches an expression ends up
/// anyway. Also whether some struct type an expression has holds an
/// optional member of any type (one the checker joined from literals,
/// `c ? { a: 1 } : {}`, among them).
pub(crate) fn names(ast: &Ast, expr_types: &HashMap<ExprId, Type>) -> (HashSet<PropKey>, bool) {
    let mut out = HashSet::new();
    for stmt in &ast.stmts {
        let Stmt::TypeDecl { name, fields, .. } = stmt else {
            continue;
        };
        for (k, ann) in fields {
            if ann == crate::ssa_lower_parse_type::OPTIONAL_NUMBER_ANN
                && ast.optional_members.plain_at(name, k)
            {
                out.insert(k.clone());
            }
        }
    }
    let mut optional = false;
    for t in expr_types.values() {
        walk(t, &mut out, &mut optional);
    }
    (out, optional)
}

fn walk(t: &Type, out: &mut HashSet<PropKey>, optional: &mut bool) {
    match t {
        Type::Struct(fs) => {
            *optional |= !fs.optional().is_empty();
            for (k, ft) in fs.iter() {
                if fs.is_optional(k) && matches!(ft, Type::Nullable(n) if **n == Type::Number) {
                    out.insert(k.clone());
                }
                walk(ft, out, optional);
            }
        }
        Type::Array(t) | Type::Nullable(t) | Type::Promise(t) | Type::Rest(t) => {
            walk(t, out, optional)
        }
        Type::Map(k, v) => {
            walk(k, out, optional);
            walk(v, out, optional);
        }
        Type::Function(ps, r) => {
            for p in ps {
                walk(p, out, optional);
            }
            walk(r, out, optional);
        }
        _ => {}
    }
}
