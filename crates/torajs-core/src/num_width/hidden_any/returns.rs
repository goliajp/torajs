//! The returns of `hidden_any` that hand back a value of any kind.
//!
//! `function q(x: number, f: () => any): number { return f() }`
//! converted what `f` answered to a number on the way out, while
//! §10.2.1.12 returns the value as is: `q(1, () => "s")` printed NaN.
//! Such a return seeds the function's return ([`Node::RetAny`]), which
//! the check driver retypes `any`.
//!
//! The checker also types `any` a value whose kind the program fixes,
//! where it only failed to follow it: the call of a closure whose
//! return it did not infer (`const f = () => n.tag`), a binding it
//! widened, a field of a union it holds boxed. Converting such a value
//! to the declared return changes nothing, and retyping the function
//! would only take its callers off the typed lane. So a return seeds
//! only when its value traces back to a value the return would convert:
//! one the checker types otherwise than the function declares
//! (`const s: any = "x"; return s` from a `number` function, `(a < b)
//! as any`), or a source of any kind: a parameter annotated `any`,
//! `unknown` or not at all, a catch binding, a field or getter
//! declared `any`, an element of an `any[]`, a member of such a value,
//! a library call the checker types `any` for no argument of any kind
//! (`JSON.parse(s)`, `m.get(k)`), a call of a function or method the
//! program declares to return `any`, a call through a fn-typed slot of
//! the boxed-entry census.
//!
//! A binding hands on what its initializer and its stores hold,
//! whatever it is annotated (a desugar pass writes `any` on bindings of
//! its own), and a loop binding the elements it reads. A call of a
//! function in sight hands on what that function returns, through its
//! return node; a method call what each receiver class's method
//! declares; a call of a binding outside the census whose type declares
//! a typed return answers that return; a class's factory (`__new_C`)
//! an instance of it. A comparison answers a boolean whatever its
//! operands hold, `+` a string or a number by its operands' types,
//! another operator other than `&&`, `||` and `??` a number, or a
//! bigint when an operand of any kind may be one, and a member of a
//! typed value is what its type declares.

use std::collections::HashSet;

use super::{Graph, Node, Origin, Ref};
use crate::ast::{BinOp, Expr, ExprId, UnaryOp};
use crate::check::Type;
use crate::num_width::hidden_any_calls::answers_any;
use crate::num_width::{Analysis, Scope, SlotKey};

mod fn_values;
mod lookups;
mod members;

pub(super) use lookups::Lookups;

impl Graph<'_> {
    /// `return e` from the body `sc` walks.
    pub(in crate::num_width) fn returned(&mut self, a: &Analysis<'_>, sc: &Scope, e: ExprId) {
        if !traced(sc.fn_name) || a.expr_types.get(&e) != Some(&Type::Any) {
            return;
        }
        let ret = Node::RetAny(sc.fn_name.to_string());
        let mut from = Vec::new();
        if self.spelled_any((a, sc), e, Read::Value, &mut from, &mut HashSet::new()) {
            self.seeds.push(ret.clone());
        }
        for n in from {
            self.edges.entry(n).or_default().push(ret.clone());
        }
    }

    fn lookups(&self, a: &Analysis<'_>) -> &Lookups {
        self.lookups.get_or_init(|| Lookups::build(a))
    }

    /// Whether the return of the body `sc` walks would convert `e`
    /// (`Read::Value`): a value of a type other than the declared one,
    /// or, when the checker types it `any`, one whose origin may be of
    /// any kind. A receiver whose member is read (`Read::Members`) only
    /// asks the second question. The returns of the functions `e` may
    /// answer through go to `from`.
    fn spelled_any(
        &self,
        (a, sc): (&Analysis<'_>, &Scope),
        e: ExprId,
        read: Read,
        from: &mut Vec<Node>,
        seen: &mut HashSet<(ExprId, Read)>,
    ) -> bool {
        let declared = match read {
            Read::Value => self.lookups(a).declared(sc.fn_name),
            Read::Members => None,
        };
        let differs = |t: &Type| declared.as_ref().is_some_and(|d| d != t);
        match a.expr_types.get(&e) {
            // a hoisted `var` before its first store
            _ if matches!(a.ast.get_expr(e), Expr::Uninit) => return false,
            Some(Type::Any) => {}
            Some(t) => return differs(t),
            None => return false,
        }
        if !seen.insert((e, read)) {
            return false;
        }
        let mut go =
            |x: ExprId, r: Read, from: &mut Vec<Node>| self.spelled_any((a, sc), x, r, from, seen);
        match a.ast.get_expr(e) {
            // an any-widened clone answers what its widened parameters
            // were handed
            Expr::Call { args, .. } if answers_any(a, e) => {
                args.iter().fold(false, |h, x| go(*x, read, from) | h)
            }
            Expr::Call { callee, .. } if factory(a, *callee) => declared.is_some(),
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => {
                match self.callees(a, sc, e, *callee) {
                    Some(rets) => {
                        let l = self.lookups(a);
                        let open = (rets.iter())
                            .any(|n| matches!(n, Node::RetAny(f) if l.returns_open(f)));
                        from.extend(rets);
                        open
                    }
                    // a binding typed with a typed return answers it, though
                    // the checker typed the call `any` (a variadic value's
                    // boxed entry); a method answers what its declaration
                    // does; any other call the walk does not follow answers
                    // what its callee declares, which the checker typed `any`
                    None => match (
                        self.typed_callee(a, sc, *callee),
                        a.ast.get_expr(e),
                        a.ast.get_expr(*callee),
                    ) {
                        (Some(r), ..) => differs(r),
                        (
                            None,
                            Expr::Call { args, .. } | Expr::OptCall { args, .. },
                            Expr::Member { obj, name } | Expr::OptChain { obj, name },
                        ) => match a.expr_types.get(obj) {
                            // a method of a value of any kind
                            Some(Type::Any) => go(*obj, Read::Members, from),
                            recv @ Some(Type::ClassRef(_) | Type::Union(_)) => {
                                self.method_spelled_any(a, (recv, name), declared.as_ref(), from)
                            }
                            // a library method the checker types `any` for an
                            // argument of any kind answers what it computes
                            // from it (`Math.abs(x)`); one it types `any`
                            // otherwise answers `any` by its own declaration
                            // (`JSON.parse(s)`)
                            _ => !args.iter().any(|x| a.expr_types.get(x) == Some(&Type::Any)),
                        },
                        _ => true,
                    },
                }
            }
            Expr::Ident(n) => {
                let mut hit = false;
                for o in self.origins(a, sc, n) {
                    hit |= match (o, self.values(a, n, o)) {
                        (Origin::Param(open), _) => open,
                        (_, Some(vs)) => vs.into_iter().fold(false, |h, v| go(v, read, from) | h),
                        (_, None) => true,
                    };
                }
                hit
            }
            // a field or element of a value the program types holds what
            // that type declares
            Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
                match a.expr_types.get(obj) {
                    Some(Type::Any) => go(*obj, Read::Members, from),
                    Some(Type::Struct(fs)) => {
                        let getter = format!("__getter_{name}");
                        fs.iter().any(|(k, t)| match t {
                            Type::Function(_, r) if k == getter.as_str() => **r == Type::Any,
                            _ => k == name.as_str() && *t == Type::Any,
                        })
                    }
                    Some(Type::ClassRef(c)) => self.member_spelled_any(a, (c, name), from),
                    // a member of a builtin value is typed by its library
                    _ => false,
                }
            }
            Expr::Index { obj, .. } | Expr::OptIndex { obj, .. } => match a.expr_types.get(obj) {
                Some(Type::Any) => go(*obj, Read::Members, from),
                Some(Type::Array(el)) => **el == Type::Any,
                _ => true,
            },
            // an object, which no declared scalar return is; its members
            // hold what it is built from
            Expr::New { .. } | Expr::Closure { .. } => declared.is_some(),
            Expr::ObjectLit { .. } | Expr::Array(_) | Expr::NewDynamic { .. }
                if declared.is_some() =>
            {
                true
            }
            Expr::ObjectLit { fields } => fields
                .iter()
                .fold(false, |h, (_, v)| go(*v, read, from) | h),
            Expr::Array(vs) => vs.iter().fold(false, |h, v| go(*v, read, from) | h),
            Expr::NewDynamic { callee, .. } => go(*callee, read, from),
            // a string when an operand is one, a number when both are
            Expr::BinOp {
                op: BinOp::Add,
                left,
                right,
            } => {
                let ty = |x: &ExprId| a.expr_types.get(x);
                let kind = match (ty(left), ty(right)) {
                    (Some(Type::String), _) | (_, Some(Type::String)) => Some(Type::String),
                    (Some(Type::Number), Some(Type::Number)) => Some(Type::Number),
                    _ => None,
                };
                kind.is_some_and(|k| differs(&k))
                    | go(*left, Read::Members, from)
                    | go(*right, Read::Members, from)
            }
            Expr::BinOp {
                op: BinOp::LAnd | BinOp::LOr,
                left,
                right,
            }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            }
            | Expr::Ternary {
                then_branch: left,
                else_branch: right,
                ..
            } => go(*left, read, from) | go(*right, read, from),
            Expr::BinOp { op, .. } if compares(*op) => differs(&Type::Boolean),
            Expr::Unary {
                op: UnaryOp::Not, ..
            }
            | Expr::InstanceOf { .. } => differs(&Type::Boolean),
            Expr::TypeOf { .. } => differs(&Type::String),
            // `+x` refuses a bigint
            Expr::Unary {
                op: UnaryOp::Plus, ..
            } => differs(&Type::Number),
            // the other operators answer a number, or a bigint when an
            // operand of any kind may be one
            Expr::BinOp { left, right, .. } => {
                differs(&Type::Number)
                    | (declared.is_some()
                        && (go(*left, Read::Members, from) | go(*right, Read::Members, from)))
            }
            Expr::Unary { expr: x, .. } | Expr::PostIncr { target: x, .. } => {
                differs(&Type::Number) | (declared.is_some() && go(*x, Read::Members, from))
            }
            Expr::Sequence { right: x, .. }
            | Expr::As { expr: x, .. }
            | Expr::Assign { value: x, .. } => go(*x, read, from),
            _ => true,
        }
    }

    /// The typed return of the type of `callee`, a binding or parameter
    /// outside the boxed-entry census (`Ast::face_mismatch_lets`,
    /// `Ast::argv_boxed_params`), whose calls answer what the stored
    /// function does.
    fn typed_callee<'t>(
        &self,
        a: &'t Analysis<'_>,
        sc: &Scope,
        callee: ExprId,
    ) -> Option<&'t Type> {
        let (Expr::Ident(n), Some(Type::Function(_, r))) =
            (a.ast.get_expr(callee), a.expr_types.get(&callee))
        else {
            return None;
        };
        if **r == Type::Any || a.ast.face_mismatch_globals.contains(n) {
            return None;
        }
        let plain = match self.resolve(a, n, sc) {
            Ref::At(Node::Decl(i)) => match self.origin(i) {
                Origin::Fixed(v) | Origin::Open(v) => !a.ast.face_mismatch_lets.contains(&v),
                Origin::Param(_) | Origin::Unknown => false,
            },
            Ref::At(Node::Read(SlotKey::Param(f, p))) => {
                !a.ast.boxed_params_of(&f).is_some_and(|ps| ps.contains(&p))
            }
            _ => false,
        };
        plain.then_some(r)
    }

    /// What the declarations `n` may name say about their values; an
    /// open parameter spells `any`.
    fn origins(&self, a: &Analysis<'_>, sc: &Scope, n: &str) -> Vec<Origin> {
        match self.resolve(a, n, sc) {
            Ref::At(Node::Decl(i)) => vec![self.origin(i)],
            Ref::At(Node::Read(SlotKey::Param(f, p))) => {
                let open = self.lookups(a).open_params.get(&(f, p)).copied();
                vec![Origin::Param(open.unwrap_or(true))]
            }
            Ref::Keyed(_) => match self.lookups(a).decls.get(n) {
                Some(os) => os.clone(),
                None => vec![Origin::Unknown],
            },
            Ref::At(_) => vec![Origin::Unknown],
        }
    }

    /// The values a binding `n` declared with `o` may hold, when the
    /// walk traces them.
    fn values(&self, a: &Analysis<'_>, n: &str, o: Origin) -> Option<Vec<ExprId>> {
        match o {
            Origin::Fixed(v) => Some(vec![v]),
            Origin::Open(v) => {
                let mut vs = self.lookups(a).stores.get(n).cloned().unwrap_or_default();
                vs.push(v);
                Some(vs)
            }
            Origin::Param(_) | Origin::Unknown => None,
        }
    }

    /// [`super::Decl::origin`] of declaration `i`.
    fn origin(&self, i: usize) -> Origin {
        match self.decls.get(i) {
            Some(d) => d.origin,
            None => (self.origins_in.get(i).copied()).unwrap_or(Origin::Unknown),
        }
    }
}

/// Whether the returns of function `f` are traced. The machinery of
/// generators and async functions (a step of the state machine, a
/// generator class's methods and their any twins, the runtime's own
/// helpers) is never retyped, so a call of it counts as a value of any
/// kind instead of being walked.
fn traced(f: &str) -> bool {
    const MACHINERY: [&str; 5] = ["__cmany", "__gfor", "__agen", "__torajs_", "__promise"];
    !(f.contains("__Gen") || MACHINERY.iter().any(|m| f.starts_with(m)))
}

/// Is `callee` a class's factory (`__new_C`, what `new C()` calls),
/// which answers an instance of it, as `new` does?
fn factory(a: &Analysis<'_>, callee: ExprId) -> bool {
    matches!(a.ast.get_expr(callee), Expr::Ident(f) if f.strip_prefix("__new_")
        .is_some_and(|c| a.ast.class_parents.contains_key(c)))
}

/// Does `op` compare its operands, answering a boolean?
fn compares(op: BinOp) -> bool {
    use BinOp::*;
    matches!(op, Lt | Gt | Le | Ge | Eq | Neq | LooseEq | LooseNeq)
}

/// How the walk reads a value: as what a return hands back, or as a
/// receiver whose members it reads.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Read {
    Value,
    Members,
}
