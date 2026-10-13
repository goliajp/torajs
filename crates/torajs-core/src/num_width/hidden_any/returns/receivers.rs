//! What the return walk of `hidden_any::returns` knows of a receiver
//! the checker types `any`, whose member a return reads.
//!
//! The checker types a union of classes or object types `any`, whose
//! members are still the ones each declares. The walk names them by
//! the annotation that types the receiver: the one on the binding or
//! parameter it reads, through a `const` with none to its initializer,
//! the written return of the function or method it calls, the
//! annotation of the field it reads, the type an `as` gives it, each
//! through a type alias. A union of builtin types, named by the members
//! the checker keeps for the value where it is read (a guard narrows
//! them) or else by its annotation, keeps the members its library
//! declares. A receiver whose annotation names something else
//! (an interface, a generic, an inline object type, classes mixed with
//! builtin types) may hold a member of any kind.

use super::Read;
use super::builtins::{self, Arm, scalar_kind};
use super::kinds::{Kind, Kinds};
use super::lookups::{Lookups, open_ann};
use super::members::method_body;
use crate::ast::{Expr, ExprId};
use crate::check::Type;
use crate::num_width::hidden_any::{Graph, Node, Origin, Ref};
use crate::num_width::{Analysis, Scope, SlotKey};

/// How deep the walk follows `const` initializers and type aliases.
const DEPTH: u8 = 8;

/// What the walk knows of a receiver the checker types `any`.
pub(super) enum Receiver {
    /// An instance of the class or object type, or of one of the union
    /// of them, the type names; `null` and `undefined` hold no member.
    Classes(Type),
    /// A value the walk traces to read its members: one with no
    /// annotation or one that leaves its kind open.
    Traced,
    /// A value of a builtin scalar or array type, or a union of them,
    /// whose members its library declares.
    Builtin(Vec<Arm>),
    /// A value whose classes the walk cannot name.
    Unknown,
}

impl Graph<'_> {
    /// Whether a return of the body `sc` walks would convert parameter
    /// `e` (`Read::Value`), whose annotation closes its kind though the
    /// checker types it `any` (`x: string | number`): a value of a kind
    /// the annotation names other than the one declared.
    pub(super) fn annotated(
        &self,
        (a, sc): (&Analysis<'_>, &Scope),
        e: ExprId,
        read: Read,
        declared: &Option<Type>,
    ) -> bool {
        let ks = match self.receiver(a, sc, e) {
            Receiver::Builtin(arms) => builtins::values(&arms),
            Receiver::Classes(_) => Kinds::one(Kind::Other),
            Receiver::Unknown => Kinds::ANY,
            // a parameter the compiler adds holds what it is handed
            Receiver::Traced => return false,
        };
        read == Read::Value && self.differs_any((sc.fn_name, read), declared.as_ref(), ks)
    }

    /// What the walk knows of receiver `obj`.
    pub(super) fn receiver(&self, a: &Analysis<'_>, sc: &Scope, obj: ExprId) -> Receiver {
        self.receiver_at(a, sc, obj, 0)
    }

    fn receiver_at(&self, a: &Analysis<'_>, sc: &Scope, obj: ExprId, depth: u8) -> Receiver {
        if depth > DEPTH {
            return Receiver::Unknown;
        }
        // the members the checker keeps for a union, as narrowed here
        if let Some(Type::Union(ms)) = a.union_types.get(&obj)
            && let Some(arms) = builtins::arms_of(ms)
        {
            return Receiver::Builtin(arms);
        }
        let l = self.lookups(a);
        let ann: Option<&str> = match a.ast.get_expr(obj) {
            Expr::Ident(n) => match self.resolve(a, n, sc) {
                Ref::At(Node::Decl(i)) => match self.origin(i) {
                    Origin::Fixed(v) => return self.receiver_at(a, sc, v, depth + 1),
                    Origin::Open(v) => l.let_anns.get(&v).map(String::as_str),
                    Origin::Param(_) | Origin::Unknown => None,
                },
                Ref::At(Node::Read(SlotKey::Param(f, p))) => {
                    l.param_anns.get(&(f, p)).map(String::as_str)
                }
                // a top-level binding a function reads
                Ref::Keyed(_) => match l.decls.get(n).map(Vec::as_slice) {
                    Some([Origin::Open(v)]) => l.let_anns.get(v).map(String::as_str),
                    _ => None,
                },
                _ => None,
            },
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => {
                match a.ast.get_expr(*callee) {
                    Expr::Ident(f)
                        if a.fn_params.contains_key(f) && !a.shadowing_reads.contains(callee) =>
                    {
                        l.written_ret(f)
                    }
                    Expr::Member { obj: o, name } | Expr::OptChain { obj: o, name } => {
                        match a.expr_types.get(o) {
                            Some(Type::ClassRef(c)) => {
                                method_body(a, c, name).and_then(|b| l.written_ret(&b))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                }
            }
            Expr::Member { obj: o, name } | Expr::OptChain { obj: o, name } => {
                match a.expr_types.get(o) {
                    Some(Type::ClassRef(c)) => field_ann(a, l, c, name),
                    _ => None,
                }
            }
            Expr::As { ty_ann, expr } if open_ann(ty_ann) => {
                return self.receiver_at(a, sc, *expr, depth + 1);
            }
            Expr::As { ty_ann, .. } => Some(ty_ann.as_str()),
            _ => None,
        };
        match ann {
            Some(t) if !open_ann(t) => self.classify(a, t, depth),
            _ => Receiver::Traced,
        }
    }

    /// What annotation `ann` says of the values it types.
    fn classify(&self, a: &Analysis<'_>, ann: &str, depth: u8) -> Receiver {
        // a parameter's annotation spells a union `__union(A|B)` and an
        // optional type `__nullable(A)`
        let mut ann = ann.trim();
        while let Some(inner) = ["__union(", "__nullable("]
            .iter()
            .find_map(|w| ann.strip_prefix(w)?.strip_suffix(')'))
        {
            ann = inner.trim();
        }
        // an array arm (`string[]`) is a builtin; a tuple, a function
        // type, an object type or a generic is not read
        if ann
            .replace("[]", "")
            .contains(['(', ')', '<', '>', '{', '}', '[', ']', '&'])
        {
            return Receiver::Unknown;
        }
        let (mut classes, mut builtins) = (Vec::new(), Vec::new());
        for arm in ann.split('|').map(str::trim) {
            if matches!(arm, "null" | "undefined") {
                continue;
            }
            if let Some(el) = arm.strip_suffix("[]") {
                builtins.push(Arm::Array(self.elements(a, el.trim(), depth)));
            } else if let Some(k) = scalar_kind(arm) {
                builtins.push(Arm::Scalar(k));
            } else if a.ast.class_parents.contains_key(arm) || self.lookups(a).structs.contains(arm)
            {
                classes.push(Type::ClassRef(arm.to_string()));
            } else if let Some(t) = self.lookups(a).aliases.get(arm)
                && depth < DEPTH
            {
                match self.classify(a, t, depth + 1) {
                    Receiver::Classes(Type::Union(ts)) => classes.extend(ts),
                    Receiver::Classes(t) => classes.push(t),
                    Receiver::Builtin(arms) => builtins.extend(arms),
                    Receiver::Traced => {}
                    Receiver::Unknown => return Receiver::Unknown,
                }
            } else {
                return Receiver::Unknown;
            }
        }
        match (classes.len(), builtins.is_empty()) {
            (0, false) => Receiver::Builtin(builtins),
            (0, true) => Receiver::Traced,
            (_, false) => Receiver::Unknown,
            (1, true) => Receiver::Classes(classes.remove(0)),
            _ => Receiver::Classes(Type::Union(classes)),
        }
    }

    /// The kinds of the elements of an array annotated `el[]`.
    fn elements(&self, a: &Analysis<'_>, el: &str, depth: u8) -> Kinds {
        let l = self.lookups(a);
        if let Some(k) = scalar_kind(el) {
            return Kinds::one(k);
        }
        if el.ends_with("[]") || a.ast.class_parents.contains_key(el) || l.structs.contains(el) {
            return Kinds::one(Kind::Other);
        }
        match l.aliases.get(el) {
            Some(t) if depth < DEPTH => match self.classify(a, t, depth + 1) {
                Receiver::Builtin(arms) => builtins::values(&arms),
                Receiver::Classes(_) => Kinds::one(Kind::Other),
                Receiver::Traced | Receiver::Unknown => Kinds::ANY,
            },
            // `any`, `unknown`, an interface, a generic
            _ => Kinds::ANY,
        }
    }
}

/// The annotation of field `f` that class `c`, or a class it extends,
/// declares.
fn field_ann<'l>(a: &Analysis<'_>, l: &'l Lookups, c: &str, f: &str) -> Option<&'l str> {
    let mut cur = Some(c.split('<').next().unwrap_or(c));
    while let Some(k) = cur {
        if let Some(ann) = l.fields.get(&(k.to_string(), f.to_string())) {
            return Some(ann.as_str());
        }
        cur = a.ast.class_parents.get(k).and_then(|p| p.as_deref());
    }
    None
}
