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

use super::inlobj_fields;
use super::retypable::ObjectTypes;
use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};
use crate::ast_throw_info::class_of;
use crate::check::Type;

/// A field: its class and its name.
type Field = (String, PropKey);

/// The fields of a program's classes and declared object types declared
/// with an object type (`super::retypable::object_ann`) or a function
/// type, and every `o.f = v` of the program that names one of them:
/// what every check round asks again with its own types. Bindings
/// resolve by name, and every annotation a name is written with counts,
/// so a same-named binding elsewhere can widen a field too, which costs
/// the typed lane, never an answer. The specializations the mono pass
/// clones are not read: a class factory's all-`any` twin is minted
/// ahead of any call that needs it, and a store of one of its
/// parameters would widen the field for every instance.
pub(super) struct FieldStores {
    objects: ObjectTypes,
    held: HashSet<Field>,
    /// `(o, v, f)` of each `o.f = v` with `f` among the held fields' names
    stores: Vec<(ExprId, ExprId, PropKey)>,
    /// the annotations of the annotated bindings, by name
    anns: HashMap<String, Vec<String>>,
}

impl FieldStores {
    pub(super) fn of(ast: &Ast) -> Self {
        let objects = ObjectTypes::of(ast);
        let mut held = HashSet::new();
        for s in &ast.stmts {
            let Stmt::TypeDecl { name, fields, .. } = s else {
                continue;
            };
            for (k, ann) in fields {
                if objects.held(ann) {
                    held.insert((name.clone(), k.clone()));
                }
            }
        }
        // a member of an object type literal is a field of a type with no name
        let lits = inlobj_held_names(ast, &objects);
        let mut names: HashSet<&PropKey> = held.iter().map(|(_, k)| k).collect();
        names.extend(&lits);
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
        FieldStores {
            objects,
            held,
            stores,
            anns,
        }
    }

    /// The names of the held fields some store hands a value `types`
    /// types `any`, `o` an instance of the class or a binding annotated
    /// with the type. A field widened is widened under its name in every
    /// held type and object type literal ([`retype`]): a
    /// subclass shares its parent's slots, and an object of one type
    /// is handed as it is to a slot of another type its shape fits (a
    /// class instance to a parameter of a declared object type, an
    /// interface to its parent's), so a reader through any of them
    /// reads the slot the store wrote.
    pub(super) fn found(&self, ast: &Ast, types: &HashMap<ExprId, Type>) -> HashSet<PropKey> {
        let mut names = HashSet::new();
        for (obj, value, name) in &self.stores {
            if types.get(value) != Some(&Type::Any) {
                continue;
            }
            let owners: Vec<String> = match class_of(types, *obj) {
                Some(c) => vec![c],
                None => match ast.get_expr(*obj) {
                    Expr::Ident(n) => (self.anns.get(n).into_iter().flatten())
                        .map(|a| crate::check_type_ann::strip_nullable(a).trim().to_string())
                        .collect(),
                    _ => Vec::new(),
                },
            };
            if (owners.iter()).any(|o| self.holds(o, name)) {
                names.insert(name.clone());
            }
        }
        names
    }

    /// Whether the type `owner` spells declares `name` held.
    fn holds(&self, owner: &str, name: &PropKey) -> bool {
        self.held.contains(&(owner.to_string(), name.clone()))
            || inlobj_fields::members(owner)
                .is_some_and(|ms| (ms.iter()).any(|(k, a)| *name == *k && self.objects.held(a)))
    }
}

/// The annotations of the annotated `let`s and parameters of `ast`, by
/// name.
fn binding_anns(ast: &Ast) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl {
            name,
            type_ann: Some(ann),
            ..
        } => {
            out.entry(name.clone()).or_default().push(ann.clone());
        }
        Stmt::FnDecl { params, .. } => {
            for p in params {
                if let Some(ann) = &p.type_ann {
                    out.entry(p.name.clone()).or_default().push(ann.clone());
                }
            }
        }
        _ => {}
    });
    out
}

/// The names of the held members of the object type literals the
/// program's bindings are annotated with.
fn inlobj_held_names(ast: &Ast, objects: &ObjectTypes) -> HashSet<PropKey> {
    let mut out = HashSet::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl {
            type_ann: Some(a), ..
        } => inlobj_fields::held_names(objects, a, &mut out),
        Stmt::FnDecl { params, .. } => {
            for a in params.iter().filter_map(|p| p.type_ann.as_deref()) {
                inlobj_fields::held_names(objects, a, &mut out);
            }
        }
        _ => {}
    });
    out
}

impl FieldStores {
    /// Respell `any` every held field named in `names`, of every class,
    /// declared object type and object type literal.
    pub(super) fn retype(&self, r: &mut Ast, names: &HashSet<PropKey>) {
        for s in &mut r.stmts {
            let Stmt::TypeDecl { fields: fs, .. } = s else {
                continue;
            };
            for (k, ann) in fs {
                if names.contains(k) && self.objects.held(ann) {
                    *ann = "any".to_string();
                }
            }
        }
        inlobj_fields::retype(r, &self.objects, names);
    }
}
