//! The promise value points a promise can reach by a path the walk
//! does not follow.
//!
//! A reader of a `Promise<number>` (an `await`, a `.then` handler's
//! parameter, a combinator's result element) takes the settled value
//! at the width of its value point, the `value` field of the promise's
//! slot, and the runtime converts an f64 into an i64 reader without a
//! check (`torajs-promise` `settled_form`). The point is integral only
//! if every promise that can reach it settles with an integer. The walk
//! joins every promise it follows into the slots it reaches; what it
//! cannot follow is a promise that comes from outside the program's
//! types: one held by a value typed `any` (or holding `any` under an
//! array, a promise, a union, an object type), or one written into a
//! container through an `any` alias of it.
//!
//! So the point is not integral by default whenever its slot's class
//! holds such a source: every value the walk keys while its type holds
//! `any` is a source, as is every slot an `any` face was declared on
//! (`escape.rs`'s seeds). The sources close over the alias classes and
//! the alias pairs the walk recorded, an untracked container's elements
//! and program-named fields being untracked too, and every `value`
//! point of a promise of a number held by a reached class widens. A
//! point only typed promises reach keeps the width its writers give it.
//!
//! The walk keys a value wherever it is written into a slot (an
//! initializer, an assignment, an argument, a return, a container
//! method's operand, a literal's element or field), through
//! `container_key_of`; that is where the sources are noted, and with
//! them the number faces a value's type puts under a promise's value
//! point (the point itself, an element of an array it settles with, a
//! field of an object it settles with). An `await` notes the faces of
//! what it reads too, as does a handler declaring a `number`
//! parameter.
//!
//! `promise_taint::PromiseTaint::widens` is the one answer to "is this face wide
//! because a promise can reach it unseen": the analysis seeds the
//! faces it holds true for, and lowering's `await` asks it for the
//! keys it reads through.

use super::{Analysis, SlotKeyId, SlotKeys};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;
use std::collections::HashMap;

/// The number faces noted under a promise's value point, and (with
/// `torajs_checks`) the key each `await` read its operand's faces
/// through, by the operand: lowering must read the same value point.
#[derive(Default)]
pub(crate) struct PromiseReads {
    pub(super) faces: crate::ssa::id_hash::HashSet<SlotKeyId>,
    #[cfg(torajs_checks)]
    pub(super) awaits: crate::ssa::id_hash::HashMap<ExprId, SlotKeyId>,
}

/// Does a value of type `t` come from, or hold, a value whose type the
/// program did not fix? A function value does not by itself: its
/// parameters and its result are expressions of their own.
fn untracked(t: &Type) -> bool {
    match t {
        Type::Any | Type::TypeVar(_) => true,
        Type::Array(e) | Type::Promise(e) | Type::Nullable(e) | Type::MapIter(e) => untracked(e),
        Type::Rest(e) => untracked(e),
        Type::Map(k, v) | Type::Dict(k, v) => untracked(k) || untracked(v),
        Type::Union(ts) => (ts.iter()).any(|t| matches!(t, Type::Promise(_)) || untracked(t)),
        Type::Struct(fs) => fs.iter().any(|(_, t)| untracked(t)),
        _ => false,
    }
}

/// The number faces a value of type `t` held at `k` has under a
/// promise's value point (`under`: `k` is already under one), pushed
/// onto `out`.
fn number_faces(keys: &SlotKeys, t: &Type, k: SlotKeyId, under: bool, out: &mut Vec<SlotKeyId>) {
    match t {
        Type::Number if under => out.push(k),
        Type::Promise(e) => number_faces(keys, e, keys.field(k, "value"), true, out),
        Type::Array(e) => number_faces(keys, e, keys.elem(k), under, out),
        Type::Nullable(e) => number_faces(keys, e, k, under, out),
        Type::Struct(fs) => {
            for (n, ft) in fs.iter() {
                number_faces(keys, ft, keys.field(k, n), under, out);
            }
        }
        _ => {}
    }
}

/// Does a value of type `t` hold a number under a promise (`under`:
/// it is under one already)? [`number_faces`] without the keys.
fn has_number_face(t: &Type, under: bool) -> bool {
    match t {
        Type::Number => under,
        Type::Promise(e) => has_number_face(e, true),
        Type::Array(e) | Type::Nullable(e) => has_number_face(e, under),
        Type::Struct(fs) => fs.iter().any(|(_, ft)| has_number_face(ft, under)),
        _ => false,
    }
}

/// Does the program read a number out of a promise: an expression of
/// a type holding a number under a promise (`Promise<number>`,
/// `Promise<number[]>`, `Promise<{ n: number }>`, `Promise<number>[]`
/// …), or a handler declaring a `number` parameter (a promise of `any`
/// hands it whatever it settles with)? A program that does not has no
/// value point at stake.
pub(super) fn reads_number_promises(ast: &Ast, expr_types: &HashMap<ExprId, Type>) -> bool {
    let number = |t: &Type| has_number_face(t, false);
    let number_param = |a: &ExprId| matches!(expr_types.get(a), Some(Type::Function(ps, _)) if ps.first() == Some(&Type::Number));
    let handler = |e: &Expr| match e {
        Expr::Call { callee, args } => {
            matches!(ast.get_expr(*callee), Expr::Member { name, .. } if name == "then" || name == "catch")
                && args.iter().any(number_param)
        }
        _ => false,
    };
    (expr_types.values()).any(number) || ast.exprs.iter().any(handler)
}

impl Analysis<'_> {
    /// Note `k`, the key the walk gave the value expression `eid`, as
    /// a source the walk does not follow when `eid`'s type holds `any`,
    /// and its number faces under a promise. `Promise.reject(r)` is no
    /// source: its `Promise<any>` stands for `never`, as it never
    /// fulfills (§27.2.4.6).
    pub(super) fn note_untracked_value(&mut self, eid: ExprId, k: SlotKeyId, scope: &super::Scope) {
        if !self.promises {
            return;
        }
        let unions = |e| self.union_types.get(e).is_some_and(untracked);
        let ty = self.expr_types.get(&eid);
        if (ty.is_some_and(untracked) || unions(&eid)) && !self.is_promise_reject(eid, scope) {
            self.untracked_values.insert(k);
        }
        if let Some(t) = ty {
            self.note_number_faces(t, k);
        }
    }

    fn is_promise_reject(&self, eid: ExprId, scope: &super::Scope) -> bool {
        let Expr::Call { callee, .. } = self.ast.get_expr(eid) else {
            return false;
        };
        matches!(self.ast.get_expr(*callee), Expr::Member { obj, name } if name == "reject"
            && matches!(self.ast.get_expr(*obj), Expr::Ident(ns) if ns == "Promise" && self.resolve(ns, scope).is_none()))
    }

    /// Note the number faces a value of type `t` held at `k` has under
    /// a promise's value point.
    pub(super) fn note_number_faces(&mut self, t: &Type, k: SlotKeyId) {
        if !has_number_face(t, false) {
            return;
        }
        let mut out = Vec::new();
        number_faces(&self.keys, t, k, false, &mut out);
        self.promise_reads.faces.extend(out);
    }

    /// What a member read `obj.name` (`eid`) does besides walking `obj`:
    /// a method value read off it may escape, and an await's read
    /// notes its number faces.
    pub(super) fn member_read_effects(
        &mut self,
        eid: ExprId,
        obj: ExprId,
        name: &str,
        scope: &super::Scope,
    ) {
        self.seed_method_value_escape(obj, name);
        self.note_awaited_read(eid, obj, scope);
    }

    /// The marked read `eid` of an `await` (`obj.value`, `obj` a
    /// promise): its faces are read whatever wrote them, keyed the way
    /// `width_of` keys the read.
    fn note_awaited_read(&mut self, eid: ExprId, obj: ExprId, scope: &super::Scope) {
        if !self.promises {
            return;
        }
        if super::awaited_promise_inner_of(self.ast, self.expr_types, eid).is_some()
            && let Some(t) = self.expr_types.get(&obj)
            && has_number_face(t, false)
            && let Some(k) = self.container_key_lookup(obj, scope)
        {
            self.note_number_faces(t, k);
            #[cfg(torajs_checks)]
            self.promise_reads.awaits.insert(obj, k);
        }
    }
}
