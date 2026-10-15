//! A factory's seed for a field its constructor writes first.
//!
//! `__new_<C>` seeds every declared field before it calls the
//! constructor, and a field typed as a struct or a keyed collection
//! gets a fresh empty one (`default_init_for_field`; an array field
//! seeds as undefined already, unless it is an array of a type
//! parameter). When
//! the constructor's leading run of statements (`fresh_this`) stores
//! the field before the instance can be seen, no read ever meets that
//! seed: the store releases it unread, and a constructor that throws
//! first drops the instance with it. Such a field is seeded with the
//! annotation's undefined sentinel instead, the immortal cell every
//! release and kind mark already skips, which is also the value
//! §10.2.1.3 gives a field before its constructor assigns it.
//!
//! The constructor must be entered only with a cell this factory made:
//! a class some other class extends, one the capturing lane can reach
//! through a value-shaped parent, an evaluation unit and a generic
//! class keep their seeds.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, PropKey, Stmt, UNDEF_SLOT_MARKER};

const DEF_ARR: &str = "__def_arr_";

pub(crate) fn seed_written_fields_unset(ast: &mut Ast) {
    if !ast.es5_value_parents.is_empty() {
        return;
    }
    let plans: Vec<(usize, Vec<(ExprId, String)>)> = {
        let index = Index::of(ast);
        (0..ast.stmts.len())
            .filter_map(|i| Some((i, plan(ast, &index, &ast.stmts[i])?)))
            .collect()
    };
    for (i, seeds) in plans {
        let mut dead_lets: HashSet<String> = HashSet::new();
        for (eid, ann) in seeds {
            retire(ast, eid, &mut dead_lets);
            ast.exprs[eid.0 as usize] = Expr::Ident(format!("{UNDEF_SLOT_MARKER}{ann}"));
        }
        let mut dead_inits: Vec<ExprId> = Vec::new();
        if let Stmt::FnDecl { body, .. } = &mut ast.stmts[i] {
            body.retain(|s| match s {
                Stmt::LetDecl { name, init, .. } if dead_lets.contains(name) => {
                    dead_inits.push(*init);
                    false
                }
                _ => true,
            });
        }
        for e in dead_inits {
            ast.tombstone_expr(e);
        }
    }
}

/// The top-level declarations the plans look up, read once.
struct Index<'a> {
    fns: HashMap<&'a str, (&'a [Param], &'a [Stmt])>,
    types: HashMap<&'a str, &'a [(PropKey, String)]>,
    parents: HashSet<&'a str>,
}

impl<'a> Index<'a> {
    fn of(ast: &'a Ast) -> Self {
        let mut fns = HashMap::new();
        let mut types = HashMap::new();
        for s in &ast.stmts {
            match s {
                Stmt::FnDecl {
                    name, params, body, ..
                } => {
                    fns.insert(name.as_str(), (params.as_slice(), body.as_slice()));
                }
                Stmt::TypeDecl { name, fields, .. } => {
                    types.insert(name.as_str(), fields.as_slice());
                }
                _ => {}
            }
        }
        let parents = (ast.class_parents.values().flatten())
            .map(String::as_str)
            .collect();
        Index {
            fns,
            types,
            parents,
        }
    }
}

/// The seeds of the factory `s` to replace, each with its field's
/// annotation.
fn plan(ast: &Ast, index: &Index, s: &Stmt) -> Option<Vec<(ExprId, String)>> {
    let Stmt::FnDecl {
        name,
        type_params,
        body,
        ..
    } = s
    else {
        return None;
    };
    let cname = name.strip_prefix("__new_")?;
    if !type_params.is_empty()
        || !ast.class_parents.contains_key(cname)
        || index.parents.contains(cname)
        || ast.es5_real_parents.contains(cname)
        || ast.eval_unit_classes.contains(cname)
    {
        return None;
    }
    let ctor_name = format!("__cm_{cname}__ctor");
    // the seed literal, then straight into the constructor: nothing
    // between them may look at the cell
    let at = body
        .iter()
        .position(|s| matches!(s, Stmt::LetDecl { name, .. } if name == "__this"))?;
    let (Stmt::LetDecl { init: lit, .. }, Some(Stmt::Expr(call))) = (&body[at], body.get(at + 1))
    else {
        return None;
    };
    let calls_ctor = matches!(ast.get_expr(*call), Expr::Call { callee, .. }
        if matches!(ast.get_expr(*callee), Expr::Ident(n) if *n == ctor_name));
    let Expr::ObjectLit { fields } = ast.get_expr(*lit) else {
        return None;
    };
    if !calls_ctor || !fields.iter().any(|(_, e)| allocates(ast, *e)) {
        return None;
    }
    let anns = index.types.get(cname)?;
    let (params, ctor_body) = index.fns.get(ctor_name.as_str())?;
    let written =
        crate::ast::fresh_this::fresh_this_writes(ast, &ctor_name, params, ctor_body).fields;
    let seeds: Vec<(ExprId, String)> = fields
        .iter()
        .filter(|(k, e)| written.contains(k.lossy().as_ref()) && allocates(ast, *e))
        .filter_map(|(k, e)| Some((*e, anns.iter().find(|(f, _)| f == k)?.1.clone())))
        .collect();
    (!seeds.is_empty()).then_some(seeds)
}

/// A seed that builds a cell: an empty array (hoisted to a typed
/// `__def_arr_` let), a nested struct literal, or an empty collection.
fn allocates(ast: &Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::Ident(n) => n.starts_with(DEF_ARR),
        Expr::ObjectLit { .. } => ast.field_default_objlits.contains_key(&e),
        Expr::New {
            class_name, args, ..
        } => {
            args.is_empty() && matches!(class_name.as_str(), "Map" | "Set" | "WeakMap" | "WeakSet")
        }
        _ => false,
    }
}

/// Empty out the seed `e` and everything under it, collecting the
/// hoisted array lets it named: nothing else reads them, and an
/// orphaned arena node must not read as a use of anything.
fn retire(ast: &mut Ast, e: ExprId, dead_lets: &mut HashSet<String>) {
    match ast.get_expr(e).clone() {
        Expr::Ident(n) if n.starts_with(DEF_ARR) => {
            dead_lets.insert(n);
        }
        Expr::ObjectLit { fields } => {
            ast.field_default_objlits.remove(&e);
            for (_, c) in fields {
                retire(ast, c, dead_lets);
                ast.tombstone_expr(c);
            }
        }
        _ => {}
    }
}
