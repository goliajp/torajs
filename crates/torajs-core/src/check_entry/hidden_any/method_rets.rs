//! Class methods that may return an `any` value, and every declaration
//! that shares their return slot.
//!
//! `m(): number { return this.c.x }` with `c: any` returns what `x`
//! holds (§10.2.1.12): a string, a fraction, an object. A return slot
//! typed by the annotation would have to convert the value, and the
//! program says to hand it back as it is. The return is retyped `any`
//! as a function's is (`super::retype_returns_any`), but a method does
//! not own its slot alone: an override fills the same vtable slot,
//! called with the signature of the hierarchy's base
//! (`__dispatch_<m>`), and an interface a class of the hierarchy
//! satisfies calls the method through a witness thunk typed with the
//! interface's own signature. Every one of them is retyped together;
//! another class that fills the interface's slot hands its typed answer
//! to the thunk, which boxes it.

use std::collections::{HashMap, HashSet};

use super::retypable::ObjectTypes;
use crate::ast::{Ast, ExprId, PropKey, Stmt, unmangle_key};
use crate::check::Type;
use crate::iface_witness::{class_thunk, get_face, lit_thunk};

/// The methods and getters of a program whose return a round may
/// retype: declared to return a `number`, `string`, `boolean` or an
/// object type, or with no annotation; not a generator or an async
/// method.
pub(super) struct MethodRets {
    /// `__cm_<C>__<m>` → `(C, m, whether its return may be retyped)`
    methods: HashMap<String, (String, String, bool)>,
}

/// The declarations a set of methods shares its return slot with.
#[derive(Default)]
pub(super) struct Family {
    /// functions whose return is retyped `any`
    pub fns: HashSet<String>,
    /// functions returning a method as a value, whose function type
    /// returns `any`
    pub getters: HashSet<String>,
    /// `(interface, member)` whose function type returns `any`
    pub members: HashSet<(String, PropKey)>,
}

impl MethodRets {
    pub(super) fn of(ast: &Ast) -> Self {
        let objects = ObjectTypes::of(ast);
        let retypable =
            |ann: &str| matches!(ann, "number" | "string" | "boolean") || objects.holds(ann);
        let mut decls: Vec<(&str, bool)> = Vec::new();
        for s in &ast.stmts {
            if let Stmt::FnDecl {
                name,
                return_type,
                is_generator,
                ..
            } = s
            {
                let ok = !is_generator
                    && !ast.async_fns.contains(name)
                    && return_type.as_deref().is_none_or(retypable);
                decls.push((name, ok));
            }
        }
        let mut methods = HashMap::new();
        for (f, ok) in decls {
            let Some(rest) = f.strip_prefix("__cm_") else {
                continue;
            };
            let class = (ast.class_parents.keys())
                .filter(|c| rest.len() > c.len() + 2 && rest.starts_with(c.as_str()))
                .filter(|c| rest[c.len()..].starts_with("__"))
                .max_by_key(|c| c.len());
            let Some(c) = class else {
                continue;
            };
            let m = &rest[c.len() + 2..];
            if m.ends_with("_set") || m == "ctor" || m.contains('$') {
                continue;
            }
            methods.insert(f.to_string(), (c.clone(), m.to_string(), ok));
        }
        MethodRets { methods }
    }

    /// Whether `f` is one of these methods.
    pub(super) fn holds(&self, f: &str) -> bool {
        self.methods
            .get(generic_of(f))
            .is_some_and(|(_, _, ok)| *ok)
    }

    /// The declarations the methods `found` share their return slot
    /// with. A hierarchy one of whose declarers of the method cannot
    /// be retyped, or whose method some typed receiver reads as a value
    /// (`types`), the receiver typed as a class of it or as an
    /// interface it fills, keeps its declared return: a value read
    /// hands the method to a slot typed with that return, and the
    /// interface's get face hands on every other row's member too.
    pub(super) fn family<'a>(
        &self,
        ast: &Ast,
        types: &HashMap<ExprId, Type>,
        found: impl Iterator<Item = &'a String>,
    ) -> Family {
        let mut out = Family::default();
        let read = super::value_reads::value_reads(ast, types);
        for f in found {
            let Some((c, m, true)) = self.methods.get(generic_of(f)) else {
                continue;
            };
            let root = root_of(ast, c);
            let tree: Vec<&String> = (self.methods.values())
                .filter(|(o, n, _)| n == m && root_of(ast, o) == root)
                .map(|(o, _, _)| o)
                .collect();
            let key = unmangle_key(m);
            let blocked = tree.iter().any(|o| {
                !self.holds(&format!("__cm_{o}__{m}"))
                    || read.contains(&((*o).clone(), key.clone()))
            }) || (tree_ifaces(ast, &tree, &key))
                .any(|i| read.contains(&(i.clone(), key.clone())));
            if blocked {
                continue;
            }
            if let Some(field) = m.strip_suffix("_get") {
                // an interface reads the getter as its data member,
                // whose slot keeps the declared type
                if witnessed(ast, &tree, &unmangle_key(field)) {
                    continue;
                }
            } else {
                self.ifaces(ast, &tree, m, &key, &mut out);
            }
            out.fns
                .extend(tree.iter().map(|o| format!("__cm_{o}__{m}")));
            if (ast.method_owners.get(m)).is_some_and(|os| tree.contains(&&os[0])) {
                out.fns.insert(format!("__dispatch_{m}"));
            }
        }
        out
    }

    /// The interfaces a class of `tree` fills the witness slots of,
    /// whose member `key` is a method: the member, and the thunks every
    /// row of it calls and reads the method through.
    fn ifaces(&self, ast: &Ast, tree: &[&String], m: &str, key: &PropKey, out: &mut Family) {
        let slots = &ast.iface_slots;
        for i in tree_ifaces(ast, tree, key) {
            out.members.insert((i.clone(), key.clone()));
            let rows = (slots.witnesses.get(i).into_iter().flatten())
                .chain(slots.loose.get(i).into_iter().flatten());
            for c in rows {
                out.fns.insert(class_thunk(c, i, m));
                out.getters.insert(class_thunk(c, i, &get_face(m)));
            }
            out.fns.insert(lit_thunk(i, m));
            out.getters.insert(lit_thunk(i, &get_face(m)));
        }
    }
}

/// The interfaces a class of `tree` fills the witness slots of, whose
/// member `key` is a method.
fn tree_ifaces<'a>(
    ast: &'a Ast,
    tree: &'a [&String],
    key: &'a PropKey,
) -> impl Iterator<Item = &'a String> {
    let slots = &ast.iface_slots;
    (slots.witnesses.iter().chain(&slots.loose))
        .filter(move |(i, cs)| cs.iter().any(|c| tree.contains(&c)) && declares_method(ast, i, key))
        .map(|(i, _)| i)
}

/// The declaration a specialization `f` the mono pass made is a clone
/// of (`__cm_Box__get$$_number`), or `f` itself.
fn generic_of(f: &str) -> &str {
    f.split("$$").next().unwrap_or(f)
}

/// The topmost ancestor of class `c`.
fn root_of<'a>(ast: &'a Ast, mut c: &'a str) -> &'a str {
    while let Some(Some(p)) = ast.class_parents.get(c) {
        c = p;
    }
    c
}

/// Whether a class of `tree` fills the witness row of an interface that
/// declares `key`.
fn witnessed(ast: &Ast, tree: &[&String], key: &PropKey) -> bool {
    let slots = &ast.iface_slots;
    (slots.witnesses.iter().chain(&slots.loose)).any(|(i, cs)| {
        cs.iter().any(|c| tree.contains(&c))
            && ast.stmts.iter().any(|s| {
                matches!(s, Stmt::TypeDecl { name, fields, .. }
                    if name == i && fields.iter().any(|(k, _)| k == key))
            })
    })
}

/// Whether the object type `i` declares `key` with a function type.
fn declares_method(ast: &Ast, i: &str, key: &PropKey) -> bool {
    ast.stmts.iter().any(|s| match s {
        Stmt::TypeDecl { name, fields, .. } if name == i => {
            (fields.iter()).any(|(k, a)| k == key && crate::num_width::fn_type_canon(a).is_some())
        }
        _ => false,
    })
}

/// Retype the return of every declaration of `fam` to `any`, and the
/// interfaces' types the witness table matches a receiver against, and
/// the object type literals spelling those interfaces.
pub(super) fn retype(r: &mut Ast, fam: &Family) {
    super::method_lits::retype(r, fam);
    for (i, k) in &fam.members {
        if let Some(Type::Struct(fs)) = r.iface_slots.types.get_mut(i) {
            for (_, t) in fs.iter_mut().filter(|(n, _)| n == k) {
                match t {
                    Type::Function(_, ret) => **ret = Type::Any,
                    Type::Nullable(f) => {
                        if let Type::Function(_, ret) = &mut **f {
                            **ret = Type::Any;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for s in &mut r.stmts {
        match s {
            Stmt::FnDecl {
                name, return_type, ..
            } if fam.fns.contains(name) => *return_type = Some("any".to_string()),
            Stmt::FnDecl {
                name,
                return_type: Some(rt),
                ..
            } if fam.getters.contains(name) => {
                if let Some(a) = returning_any(rt) {
                    *rt = a;
                }
            }
            Stmt::TypeDecl { name, fields, .. } => {
                for (k, a) in fields.iter_mut() {
                    if fam.members.contains(&(name.clone(), k.clone()))
                        && let Some(n) = returning_any(a)
                    {
                        *a = n;
                    }
                }
            }
            _ => {}
        }
    }
}

/// The function type `ann` spells, returning `any`: `__fn(P)->(R)`
/// becomes `__fn(P)->(any)` (`__cls(` keeps its spelling), under any
/// `__nullable(...)` around it.
pub(super) fn returning_any(ann: &str) -> Option<String> {
    if let Some(inner) = ann
        .strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
    {
        return returning_any(inner).map(|a| format!("__nullable({a})"));
    }
    let head = ["__fn(", "__cls("]
        .into_iter()
        .find(|h| ann.starts_with(h))?;
    let canon = crate::num_width::fn_type_canon(ann)?;
    let (ps, _) = crate::num_width::split_fn_type(&canon)?;
    Some(format!("{head}{})->(any)", ps.join("|")))
}

#[cfg(test)]
mod tests {
    use super::returning_any;

    #[test]
    fn a_function_type_returns_any_in_its_own_spelling() {
        assert_eq!(
            returning_any("__fn()->(number)").as_deref(),
            Some("__fn()->(any)")
        );
        assert_eq!(
            returning_any("__cls(number|string)->(__inlobj(x:number))").as_deref(),
            Some("__cls(number|string)->(any)")
        );
        assert_eq!(
            returning_any("__nullable(__fn(Map<string|number>)->(number))").as_deref(),
            Some("__nullable(__fn(Map<string|number>)->(any))")
        );
    }

    #[test]
    fn anything_else_is_no_function_type() {
        assert_eq!(returning_any("number"), None);
        assert_eq!(returning_any("__fn()->(number)[]"), None);
    }
}
