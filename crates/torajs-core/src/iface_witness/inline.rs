//! Object type literals written in place (`s: { m: () => number }`).
//!
//! Such a type is as structural as a declared one: an instance of a
//! class with the method is one of its values. The witness rows are
//! planned per declaration, so each distinct literal with a method
//! that no declaration already spells gets one of its own, under a
//! name no declaration of the program takes. A slot typed by the
//! literal checks as the same type as the declaration
//! (`IfaceSlots::iface_of` compares types), so it dispatches through
//! the same rows.
//!
//! A literal whose members name a type parameter is left out: the
//! declaration would be a non-generic one naming a type nothing binds,
//! and generic code is not yet instantiated where this runs.

use std::collections::BTreeSet;

use crate::ast::{Ast, Expr, PropKey, Stmt};

/// The name of the next declaration made for such a literal: the
/// first `__InlIface<n>` from `*n` on that `taken` does not hold.
fn decl_name(n: &mut usize, taken: &BTreeSet<&str>) -> String {
    loop {
        *n += 1;
        let name = format!("__InlIface{n}");
        if !taken.contains(name.as_str()) {
            return name;
        }
    }
}

/// Every type parameter some declaration of the program declares.
fn type_params(ast: &Ast) -> Vec<String> {
    let mut out: BTreeSet<String> = ast.class_type_params.values().flatten().cloned().collect();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::FnDecl { type_params, .. }
        | Stmt::TypeDecl { type_params, .. }
        | Stmt::ClassDecl { type_params, .. } => out.extend(type_params.iter().cloned()),
        _ => {}
    });
    out.into_iter().collect()
}

/// `fields` in key order, for comparing two spellings of one type.
fn keyed(fields: &[(String, String)]) -> Vec<(String, String)> {
    let mut v = fields.to_vec();
    v.sort();
    v
}

/// Append a declaration for each object type literal with a method
/// that the program's annotations write and no declaration spells.
pub(super) fn synthesize(ast: &mut Ast) {
    let mut anns: BTreeSet<String> = BTreeSet::new();
    let mut take = |a: &str| literals_in(a, &mut anns);
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::FnDecl {
            params,
            return_type,
            ..
        } => {
            params
                .iter()
                .filter_map(|p| p.type_ann.as_deref())
                .for_each(&mut take);
            return_type.as_deref().into_iter().for_each(&mut take);
        }
        Stmt::LetDecl {
            type_ann: Some(a), ..
        } => take(a),
        Stmt::TypeDecl { fields, .. } => fields.iter().for_each(|(_, a)| take(a)),
        _ => {}
    });
    for e in &ast.exprs {
        match e {
            Expr::As { ty_ann, .. } => take(ty_ann),
            Expr::ArrowFn {
                params,
                return_type,
                ..
            } => {
                params
                    .iter()
                    .filter_map(|p| p.type_ann.as_deref())
                    .for_each(&mut take);
                return_type.as_deref().into_iter().for_each(&mut take);
            }
            _ => {}
        }
    }
    let mut spelled: Vec<Vec<(String, String)>> = (ast.stmts.iter())
        .filter_map(|s| match s {
            Stmt::TypeDecl {
                name,
                type_params,
                fields,
            } if type_params.is_empty() && !ast.class_parents.contains_key(name) => (fields.iter())
                .map(|(k, a)| Some((k.as_str()?.to_string(), a.clone())))
                .collect::<Option<Vec<_>>>()
                .map(|f| keyed(&f)),
            _ => None,
        })
        .collect();
    let generic = type_params(ast);
    let taken: BTreeSet<&str> = (ast.stmts.iter())
        .filter_map(|s| match s {
            Stmt::TypeDecl { name, .. } | Stmt::ClassDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .chain(ast.class_parents.keys().map(String::as_str))
        .collect();
    let mut n = 0;
    let mut made = Vec::new();
    for ann in anns {
        let Some(fields) = crate::ast_collect_fn_closure_init::inlobj_fields(&ann) else {
            continue;
        };
        // an optional member is recorded per declaration name
        // (`Ast::optional_members`), which this one has none of
        if fields.iter().any(|(_, optional, _)| *optional)
            || !fields.iter().any(|(_, _, a)| method(a))
        {
            continue;
        }
        if (fields.iter())
            .any(|(_, _, a)| crate::ast_throw_info::obj_cast::names_type_param(a, &generic))
        {
            continue;
        }
        let fields: Vec<(String, String)> = fields.into_iter().map(|(k, _, a)| (k, a)).collect();
        if spelled.contains(&keyed(&fields)) {
            continue;
        }
        spelled.push(keyed(&fields));
        made.push(Stmt::TypeDecl {
            name: decl_name(&mut n, &taken),
            type_params: Vec::new(),
            fields: (fields.into_iter())
                .map(|(k, a)| (PropKey::from(k.as_str()), a))
                .collect(),
        });
    }
    ast.stmts.extend(made);
}

/// A member annotation of a function type.
fn method(ann: &str) -> bool {
    crate::num_width::fn_type_canon(ann)
        .as_deref()
        .and_then(crate::num_width::split_fn_type)
        .is_some()
}

/// Every `__inlobj(..)` spelled in `ann`, nested ones included.
fn literals_in(ann: &str, out: &mut BTreeSet<String>) {
    const OPEN: &str = "__inlobj(";
    let mut from = 0;
    while let Some(at) = ann[from..].find(OPEN).map(|i| from + i) {
        let mut depth = 0usize;
        let mut end = None;
        for (i, c) in ann[at + OPEN.len() - 1..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(at + OPEN.len() - 1 + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { return };
        out.insert(ann[at..end].to_string());
        from = at + OPEN.len();
    }
}
