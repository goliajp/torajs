//! What `desugar_enums` needs to know before it rewrites anything:
//! the enum objects by name, every other name the program binds, and
//! the merged declarations.

use std::collections::{HashMap, HashSet};

use super::{ENUM_CONST_ANN, ENUM_MERGE_ANN, ENUM_OBJECT_ANN};
use crate::ast::desugar_with::walk::stmt_children_ref;
use crate::ast::{Ast, Expr, ExprId, Param, StaticInit, Stmt};

/// A merged declaration: its marker call, the name, its own object
/// literal of new members, and the first declaration's object literal.
pub(super) struct Merge {
    pub(super) call: ExprId,
    pub(super) name: String,
    pub(super) objlit: ExprId,
    pub(super) primary: ExprId,
}

/// One enum object's `const`.
pub(super) struct EnumDecl {
    pub(super) objlit: ExprId,
    pub(super) is_const: bool,
}

#[derive(Default)]
pub(super) struct Census {
    pub(super) enums: HashMap<String, Vec<EnumDecl>>,
    pub(super) others: HashSet<String>,
    pub(super) merges: Vec<Merge>,
}

pub(super) fn take_census(ast: &Ast) -> Census {
    let mut c = Census::default();
    for s in &ast.stmts {
        census(ast, s, &mut c);
    }
    for e in &ast.exprs {
        if let Expr::ArrowFn { params, body, .. } = e {
            push_params(params, &mut c.others);
            for s in body {
                census(ast, s, &mut c);
            }
        }
    }
    c.others.extend(ast.fn_expr_self_names.values().cloned());
    c
}

pub(super) fn is_enum_merge(ast: &Ast, s: &Stmt) -> Option<Merge> {
    let Stmt::Expr(call) = s else {
        return None;
    };
    let Expr::Call { callee, args } = ast.get_expr(*call) else {
        return None;
    };
    if !matches!(ast.get_expr(*callee), Expr::Ident(n) if n == ENUM_MERGE_ANN) {
        return None;
    }
    let [name, primary, objlit] = args.as_slice() else {
        return None;
    };
    let (Expr::Ident(name), Expr::Number(primary)) = (ast.get_expr(*name), ast.get_expr(*primary))
    else {
        return None;
    };
    Some(Merge {
        call: *call,
        name: name.clone(),
        objlit: *objlit,
        primary: ExprId(*primary as u32),
    })
}

/// The enum object's `const`: its name, object literal, and whether
/// the enum is a const enum.
pub(super) fn is_enum_decl(s: &Stmt) -> Option<(&String, ExprId, bool)> {
    match s {
        Stmt::LetDecl {
            name,
            type_ann: Some(ann),
            init,
            ..
        } if ann == ENUM_OBJECT_ANN || ann == ENUM_CONST_ANN => {
            Some((name, *init, ann == ENUM_CONST_ANN))
        }
        _ => None,
    }
}

fn push_params(params: &[Param], out: &mut HashSet<String>) {
    for p in params {
        out.insert(p.name.clone());
    }
}

/// Enum objects by name, and every other name the program binds —
/// the two facts that decide whether `E.A` can be folded.
fn census(ast: &Ast, s: &Stmt, c: &mut Census) {
    if let Some((name, objlit, is_const)) = is_enum_decl(s) {
        let d = EnumDecl { objlit, is_const };
        c.enums.entry(name.clone()).or_default().push(d);
        return;
    }
    if let Some(m) = is_enum_merge(ast, s) {
        c.merges.push(m);
        return;
    }
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => {
            c.others.insert(name.clone());
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            c.others.insert(var_name.clone());
        }
        Stmt::Try {
            catch_param: Some(p),
            ..
        } => {
            c.others.insert(p.clone());
        }
        Stmt::FnDecl {
            name, params, body, ..
        } => {
            c.others.insert(name.clone());
            push_params(params, &mut c.others);
            for s in body {
                census(ast, s, c);
            }
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            c.others.insert(name.clone());
            if let Some(ct) = ctor {
                push_params(&ct.params, &mut c.others);
                for s in &ct.body {
                    census(ast, s, c);
                }
            }
            for m in methods.iter().chain(static_methods.iter()) {
                push_params(&m.params, &mut c.others);
                for s in &m.body {
                    census(ast, s, c);
                }
            }
            for si in static_init {
                if let StaticInit::Block(b) = si {
                    for s in b {
                        census(ast, s, c);
                    }
                }
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => census(ast, inner, c),
        // A named import is not counted: it names the declaration the
        // resolver spliced in from the source module (renamed to the
        // local name when they differ), counted where it stands.
        Stmt::ImportDecl {
            default, namespace, ..
        } => {
            c.others.extend(default.iter().cloned());
            c.others.extend(namespace.iter().cloned());
        }
        _ => {}
    }
    for child in stmt_children_ref(s) {
        census(ast, child, c);
    }
}
