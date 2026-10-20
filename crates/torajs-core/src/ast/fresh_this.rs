//! Which `this.f = v` writes at the head of a constructor body reach
//! an instance nothing else can see yet.
//!
//! `__cm_<C>__ctor` is entered only from the factory `__new_<C>` and
//! from a derived constructor's `super(...)`, and both hand it a cell
//! the factory just minted: its header carries no frozen bit, no
//! field was redefined, and every field holds its type's default.
//! Nothing changes that until the pointer escapes — a call that takes
//! it, a closure that captures it, a read that stores it somewhere.
//! Along the leading run of statements that only write declared
//! fields of `this` from values not involving `this`, or do not
//! mention `this` at all, a field store cannot be refused (§10.1.9.2
//! has nothing to refuse it with), so it carries no writable guard
//! and the throw census does not count it; and the first write to a
//! field whose default is `null` / `undefined` has no old value to
//! release.
//!
//! Both consumers — the lowering's guard elision and the may-throw
//! census that decides whether a caller probes after the constructor
//! — read the same answer, so a store the lowering guards is always a
//! store the census counts. The run stops at the first statement of
//! any other shape: a `super(...)` call (the parent may freeze
//! `this`), a branch, a loop, a write through an accessor or to a
//! name the class does not declare (those take other lanes, which do
//! refuse), anything that mentions `this` outside the receiver slot
//! of a field write. A default parameter mentioning `this`, or a
//! `this` typed `any` (an exotic-rooted class), disqualifies the whole
//! body.

use std::borrow::Cow;
use std::collections::HashSet;

use super::{Ast, Expr, ExprId, Param, Stmt};
use crate::ssa::id_hash::HashSet as IdSet;

const THIS: &str = "__this";

#[derive(Default)]
pub(crate) struct FreshThis {
    /// `this` receivers of field writes reached before the instance
    /// escapes: unfrozen, no field redefined.
    pub(crate) recv: HashSet<ExprId>,
    /// The subset whose field holds the factory's null / undefined
    /// default at that point, so there is no old value to release.
    pub(crate) first_write: HashSet<ExprId>,
    /// The fields those writes store.
    pub(crate) fields: HashSet<String>,
}

pub(crate) fn fresh_this_writes(
    ast: &Ast,
    name: &str,
    params: &[Param],
    body: &[Stmt],
) -> FreshThis {
    let mut out = FreshThis::default();
    let Some(cname) = ctor_class_name(name) else {
        return out;
    };
    let typed_this = params
        .first()
        .is_some_and(|p| p.name == THIS && p.type_ann.as_deref().is_some_and(|a| a != "any"));
    if !typed_this
        || params
            .iter()
            .any(|p| p.default.is_some_and(|d| mentions_this(ast, d)))
    {
        return out;
    }
    let fields = DeclaredFields::of(ast, cname);
    let mut written: IdSet<&str> = IdSet::default();
    for s in body {
        match s {
            Stmt::Expr(e) => {
                if let Some((recv, field, value)) = this_field_write(ast, *e) {
                    // a destructure group temp's store walks its source
                    // through the iterator protocol, which can raise
                    if mentions_this(ast, value)
                        || !fields.plain.contains(field)
                        || ast.iter_destr_srcs.contains_key(&value)
                    {
                        return out;
                    }
                    out.recv.insert(recv);
                    out.fields.insert(field.to_string());
                    if written.insert(field) && fields.nullish.contains(field) {
                        out.first_write.insert(recv);
                    }
                } else if mentions_this(ast, *e) {
                    return out;
                }
            }
            Stmt::LetDecl { init, .. } => {
                if mentions_this(ast, *init) {
                    return out;
                }
            }
            _ => return out,
        }
    }
    out
}

/// `__cm_<C>__ctor`, or a specialised twin of it (`$$` suffix).
fn ctor_class_name(name: &str) -> Option<&str> {
    let (cname, tail) = name.strip_prefix("__cm_")?.split_once("__ctor")?;
    (tail.is_empty() || tail.starts_with("$$")).then_some(cname)
}

/// `__this.<field> = <value>` as (receiver expr, field, value).
fn this_field_write(ast: &Ast, e: ExprId) -> Option<(ExprId, &str, ExprId)> {
    let Expr::Assign { target, value } = ast.get_expr(e) else {
        return None;
    };
    let Expr::Member { obj, name } = ast.get_expr(*target) else {
        return None;
    };
    matches!(ast.get_expr(*obj), Expr::Ident(n) if n == THIS).then_some((*obj, name, *value))
}

fn mentions_this(ast: &Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::Ident(n) => n == THIS,
        Expr::This | Expr::ArrowFn { .. } => true,
        Expr::Closure { captures, .. } => captures.iter().any(|c| c == THIS),
        _ => {
            let mut found = false;
            super::desugar_async_sm_walk::visit_expr_children(ast, e, &mut |c| {
                found = found || mentions_this(ast, c);
            });
            found
        }
    }
}

/// The fields a class declares, as the typed store lane sees them.
struct DeclaredFields<'a> {
    /// Declared on the class with no setter anywhere on its chain:
    /// the write is a plain slot store.
    plain: IdSet<Cow<'a, str>>,
    /// Whose factory default is `null` or `undefined` in the class
    /// and in every class derived from it — a derived class may
    /// redeclare a field with another type, and its factory then
    /// seeds a cell the base constructor's first write would leak.
    nullish: IdSet<Cow<'a, str>>,
}

impl<'a> DeclaredFields<'a> {
    fn of(ast: &'a Ast, cname: &str) -> Self {
        let mut plain: IdSet<Cow<'a, str>> = IdSet::default();
        let mut nullish: IdSet<Cow<'a, str>> = IdSet::default();
        let mut other: IdSet<Cow<'a, str>> = IdSet::default();
        let mut seed: Option<ExprId> = None;
        for s in &ast.stmts {
            if let Stmt::FnDecl { name, body, .. } = s
                && name.strip_prefix("__new_") == Some(cname)
            {
                seed = this_seed(body);
            }
            let Stmt::TypeDecl { name, fields, .. } = s else {
                continue;
            };
            let own = name == cname;
            if !own && !derives_from(ast, name, cname) {
                continue;
            }
            for (f, ann) in fields {
                let f = f.lossy();
                if ann.starts_with("__nullable(")
                    || matches!(ann.as_str(), "any" | "undefined" | "void")
                {
                    nullish.insert(f.clone());
                } else {
                    other.insert(f.clone());
                }
                if own {
                    plain.insert(f);
                }
            }
        }
        nullish.retain(|f| !other.contains(f));
        nullish.extend(undefined_seeds(ast, cname, seed));
        plain.retain(|f| !has_setter_in_chain(ast, cname, f));
        DeclaredFields { plain, nullish }
    }
}

/// The fields `__new_<cname>` seeds with an annotation's undefined
/// sentinel (`desugar_classes::factory_unset`, or a type with no zero of
/// its own), when no class derives from `cname`: a derived factory
/// seeds the base fields itself, possibly with a cell.
fn undefined_seeds<'a>(ast: &'a Ast, cname: &str, seed: Option<ExprId>) -> Vec<Cow<'a, str>> {
    if (ast.class_parents.values()).any(|p| p.as_deref() == Some(cname)) {
        return Vec::new();
    }
    let Some(Expr::ObjectLit { fields }) = seed.map(|e| ast.get_expr(e)) else {
        return Vec::new();
    };
    let marked = |e: &ExprId| matches!(ast.get_expr(*e), Expr::Ident(n) if n.starts_with(super::UNDEF_SLOT_MARKER));
    (fields.iter())
        .filter(|(_, e)| marked(e))
        .map(|(k, _)| k.lossy())
        .collect()
}

/// The literal a factory body seeds `__this` with.
pub(crate) fn this_seed(body: &[Stmt]) -> Option<ExprId> {
    body.iter().find_map(|s| match s {
        Stmt::LetDecl { name, init, .. } if name == "__this" => Some(*init),
        _ => None,
    })
}

fn has_setter_in_chain(ast: &Ast, cname: &str, field: &str) -> bool {
    ast.accessor_setters.keys().any(|(c, k)| {
        let c = c.split('<').next().unwrap_or(c);
        k == field && (c == cname || derives_from(ast, cname, c))
    })
}

fn derives_from(ast: &Ast, class: &str, ancestor: &str) -> bool {
    let mut cur = class;
    while let Some(Some(parent)) = ast.class_parents.get(cur) {
        if parent == ancestor {
            return true;
        }
        cur = parent;
    }
    false
}
