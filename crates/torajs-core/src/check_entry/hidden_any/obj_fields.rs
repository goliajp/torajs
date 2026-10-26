//! Fields of a class or a declared object type, declared with an object
//! or a function type, that are stored an `any` value.
//!
//! `h.c = raw` with `raw: any` and `class H { c: C }` stores what `raw`
//! holds (§10.1.9 OrdinarySet, §10.1.6.3 stores the value as it is): a
//! dynamic object, an instance of another class, a number. A field slot
//! laid out by the annotation reads the value at `C`'s offsets, so the
//! value would have to be copied into the layout or rejected, and
//! neither is what the program says. The field is respelled `any` for
//! the rounds after the first, as a return is (`super::obj_rets`): the
//! round that checks the program as written still holds every read and
//! store to the declared type, and the field's slot holds the value of
//! any kind as it is.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};
use crate::ast_throw_info::class_of;
use crate::check::Type;

/// A field: its class and its name.
pub(super) type Field = (String, PropKey);

/// The fields of `ast`'s classes and declared object types declared
/// with an object type (`super::retypable::object_ann`) or a function
/// type that some `o.f = v` of `ast` stores a value `types` types
/// `any`, `o` an instance of the class or a binding annotated with the
/// type. Bindings resolve by name, so a same-named binding elsewhere can
/// widen a field too, which costs the typed lane, never an answer. The
/// specializations the mono pass clones are not read: a class factory's
/// all-`any` twin is minted ahead of any call that needs it, and a store
/// of one of its parameters would widen the field for every instance.
pub(super) fn found(ast: &Ast, types: &HashMap<ExprId, Type>) -> HashSet<Field> {
    let mut stored: HashSet<Field> = HashSet::new();
    let mut anns: Option<HashMap<String, String>> = None;
    for e in &ast.exprs {
        let Expr::Assign { target, value } = e else {
            continue;
        };
        let (Expr::Member { obj, name }, Some(Type::Any)) =
            (ast.get_expr(*target), types.get(value))
        else {
            continue;
        };
        let owner = class_of(types, *obj).or_else(|| match ast.get_expr(*obj) {
            Expr::Ident(n) => (anns.get_or_insert_with(|| binding_anns(ast)).get(n))
                .map(|a| crate::check_type_ann::strip_nullable(a).trim().to_string()),
            _ => None,
        });
        if let Some(owner) = owner {
            stored.insert((owner, PropKey::from(name)));
        }
    }
    let mut out = HashSet::new();
    for s in &ast.stmts {
        let Stmt::TypeDecl { name, fields, .. } = s else {
            continue;
        };
        for (k, ann) in fields {
            let key = (name.clone(), k.clone());
            let held = super::retypable::object_ann(ann) || crate::ast::is_fn_like_ann(ann);
            if held && stored.contains(&key) {
                out.insert(key);
            }
        }
    }
    out
}

/// The annotation of every annotated `let` and parameter of `ast`, by
/// name.
fn binding_anns(ast: &Ast) -> HashMap<String, String> {
    let mut out = HashMap::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl {
            name,
            type_ann: Some(ann),
            ..
        } => {
            out.insert(name.clone(), ann.clone());
        }
        Stmt::FnDecl { params, .. } => {
            for p in params {
                if let Some(ann) = &p.type_ann {
                    out.insert(p.name.clone(), ann.clone());
                }
            }
        }
        _ => {}
    });
    out
}

/// Respell `any` each field in `fields`.
pub(super) fn retype(r: &mut Ast, fields: &HashSet<Field>) {
    for s in &mut r.stmts {
        let Stmt::TypeDecl {
            name, fields: fs, ..
        } = s
        else {
            continue;
        };
        for (k, ann) in fs {
            if fields.contains(&(name.clone(), k.clone())) {
                *ann = "any".to_string();
            }
        }
    }
}
