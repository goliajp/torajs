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

/// The fields of a program's classes and declared object types declared
/// with an object type (`super::retypable::object_ann`) or a function
/// type, and every `o.f = v` of the program that names one of them:
/// what every check round asks again with its own types. Bindings
/// resolve by name, so a same-named binding elsewhere can widen a field
/// too, which costs the typed lane, never an answer. The specializations
/// the mono pass clones are not read: a class factory's all-`any` twin
/// is minted ahead of any call that needs it, and a store of one of its
/// parameters would widen the field for every instance.
pub(super) struct FieldStores {
    held: HashSet<Field>,
    /// `(o, v, f)` of each `o.f = v` with `f` among the held fields' names
    stores: Vec<(ExprId, ExprId, PropKey)>,
    /// the annotation of every annotated binding, by name
    anns: HashMap<String, String>,
}

impl FieldStores {
    pub(super) fn of(ast: &Ast) -> Self {
        let mut held = HashSet::new();
        for s in &ast.stmts {
            let Stmt::TypeDecl { name, fields, .. } = s else {
                continue;
            };
            for (k, ann) in fields {
                if super::retypable::object_ann(ann) || crate::ast::is_fn_like_ann(ann) {
                    held.insert((name.clone(), k.clone()));
                }
            }
        }
        let names: HashSet<&PropKey> = held.iter().map(|(_, k)| k).collect();
        let mut stores = Vec::new();
        for e in &ast.exprs {
            if let Expr::Assign { target, value } = e
                && let Expr::Member { obj, name } = ast.get_expr(*target)
                && names.contains(&PropKey::from(name))
            {
                stores.push((*obj, *value, PropKey::from(name)));
            }
        }
        let anns = if stores.is_empty() {
            HashMap::new()
        } else {
            binding_anns(ast)
        };
        FieldStores { held, stores, anns }
    }

    /// The held fields some store hands a value `types` types `any`,
    /// `o` an instance of the class or a binding annotated with the type.
    pub(super) fn found(&self, ast: &Ast, types: &HashMap<ExprId, Type>) -> HashSet<Field> {
        let mut out = HashSet::new();
        for (obj, value, name) in &self.stores {
            if types.get(value) != Some(&Type::Any) {
                continue;
            }
            let owner = class_of(types, *obj).or_else(|| match ast.get_expr(*obj) {
                Expr::Ident(n) => (self.anns.get(n))
                    .map(|a| crate::check_type_ann::strip_nullable(a).trim().to_string()),
                _ => None,
            });
            if let Some(owner) = owner
                && let key = (owner, name.clone())
                && self.held.contains(&key)
            {
                out.insert(key);
            }
        }
        out
    }
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
