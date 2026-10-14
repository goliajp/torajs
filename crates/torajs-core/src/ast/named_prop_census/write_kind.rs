//! Whether a value the checker types `any` is a number all the same,
//! when an any-lane write stores it into a member.
//!
//! A receiver-polymorphic twin retypes its receiver and parameters
//! `any`, so its `this.pos = this.pos + 1` stores an `any` value. The
//! receiver is a struct cell or not: a cell's member read answers the
//! member's declared type, unless a write of the program can leave it
//! holding a value of another type (`unknown`) or an accessor can stand
//! there; a receiver that is no struct cell takes the write as a
//! property of its own, which no layout slot holds. So a value built by
//! arithmetic from number literals and the written receiver's own
//! members, which every layout holding the written member declares
//! `number`, is a number.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, BinOp, Expr, ExprId, PropKey, Stmt, UnaryOp};
use crate::check::{Lit, StructFields, Type};

/// What a layout declares a member to hold.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Num {
    Number,
    /// `number | undefined`: arithmetic on it is a number, a copy of
    /// it may be `undefined`.
    NumberOrUndefined,
    Other,
}

/// One layout of the program: a type or class declaration's members by
/// their annotations, or a struct the checker inferred.
enum Layout<'a> {
    Decl(&'a [(PropKey, String)]),
    Struct(&'a StructFields),
}

impl Layout<'_> {
    /// What the layout declares the member `name` to hold, if it has
    /// one; a name spelled twice answers as its last spelling.
    fn member(&self, name: &str) -> Option<Num> {
        let is = |k: &PropKey| k.as_wtf8().as_bytes() == name.as_bytes();
        match self {
            Layout::Decl(fs) => {
                fs.iter()
                    .rev()
                    .find(|(k, _)| is(k))
                    .map(|(_, ann)| match ann.trim() {
                        "number" => Num::Number,
                        crate::ssa_lower_parse_type::OPTIONAL_NUMBER_ANN => Num::NumberOrUndefined,
                        _ => Num::Other,
                    })
            }
            Layout::Struct(fs) => fs.iter().rev().find(|(k, _)| is(k)).map(|(_, t)| match t {
                Type::Number | Type::Literal(Lit::Num(_)) => Num::Number,
                Type::Nullable(n) if matches!(**n, Type::Number) => Num::NumberOrUndefined,
                _ => Num::Other,
            }),
        }
    }
}

/// Every layout of the program: the declared types and classes, and
/// the structs the checker inferred. A layout seen twice is listed
/// twice; the verdicts ask about every layout alike, so a repeat
/// answers as the first did.
pub(super) struct Layouts<'a> {
    sets: Vec<Layout<'a>>,
    /// The verdicts given, by the written member: the read member,
    /// whether `number | undefined` counted, and the answer.
    asked: RefCell<HashMap<String, Vec<(String, bool, bool)>>>,
}

impl<'a> Layouts<'a> {
    pub(super) fn of(ast: &'a Ast, expr_types: &'a HashMap<ExprId, Type>) -> Self {
        let mut sets = Vec::new();
        for s in &ast.stmts {
            if let Stmt::TypeDecl { fields, .. } = s {
                sets.push(Layout::Decl(fields));
            }
        }
        for t in expr_types.values() {
            collect(t, &mut sets);
        }
        Layouts {
            sets,
            asked: RefCell::default(),
        }
    }

    /// Whether every layout holding the member `written` declares
    /// `read` a number (`number | undefined` too, when `or_undefined`).
    fn declare_number(&self, written: &str, read: &str, or_undefined: bool) -> bool {
        let given = self.asked.borrow().get(written).and_then(|v| {
            v.iter()
                .find(|(r, u, _)| r == read && *u == or_undefined)
                .map(|(_, _, a)| *a)
        });
        if let Some(a) = given {
            return a;
        }
        let a = self
            .sets
            .iter()
            .filter(|s| s.member(written).is_some())
            .all(|s| match s.member(read) {
                Some(Num::Number) => true,
                Some(Num::NumberOrUndefined) => or_undefined,
                _ => false,
            });
        let mut asked = self.asked.borrow_mut();
        let given = asked.entry(written.to_string()).or_default();
        given.push((read.to_string(), or_undefined, a));
        a
    }
}

fn collect<'a>(t: &'a Type, sets: &mut Vec<Layout<'a>>) {
    match t {
        Type::Struct(fs) => {
            sets.push(Layout::Struct(fs));
            for (_, ft) in fs.iter() {
                collect(ft, sets);
            }
        }
        Type::Array(t) | Type::Nullable(t) => collect(t, sets),
        Type::Union(ts) => {
            for t in ts {
                collect(t, sets);
            }
        }
        _ => {}
    }
}

/// What a number-ness verdict reads besides the expression: the
/// program (`'a`) and one pass's sets (`'b`).
pub(super) struct Facts<'a, 'b> {
    pub(super) ast: &'a Ast,
    pub(super) expr_types: &'a HashMap<ExprId, Type>,
    pub(super) layouts: &'b std::cell::OnceCell<Layouts<'a>>,
    /// The members a write can leave holding a value of any kind.
    pub(super) unknown: &'b HashSet<String>,
    /// The members a verdict asked about while `unknown` left them out.
    pub(super) asked: &'b RefCell<Vec<String>>,
    /// Whether an accessor can stand at the member.
    pub(super) accessor: &'b dyn Fn(&str) -> bool,
    /// The optional `number` members kept in an f64 slot.
    pub(super) optional_number: &'b HashSet<PropKey>,
}

impl Facts<'_, '_> {
    /// Whether a write can leave the member `name` holding a value of
    /// any kind, noting the question when the answer may grow.
    fn unknown(&self, name: &str) -> bool {
        let hit = self.unknown.contains(name);
        if !hit && !self.asked.borrow().iter().any(|n| n == name) {
            self.asked.borrow_mut().push(name.to_string());
        }
        hit
    }

    /// The program's layouts, gathered on the first verdict that reads
    /// them.
    fn layouts(&self) -> &Layouts<'_> {
        self.layouts
            .get_or_init(|| Layouts::of(self.ast, self.expr_types))
    }
}

/// Whether `value`, stored into the member `written` of the receiver
/// `recv`, fits that member's own slot whatever its width: the member
/// itself stepped by integer literals (`this.pos = this.pos + 1`). Read
/// from a cell, the member holds a number of the slot's width (it is
/// declared `number` everywhere, and no other write leaves a value of
/// another kind there), so an integer slot reads an integer and the
/// sum is one again; an f64 slot holds any number. A product or a
/// negation could make -0, which no integer slot holds. An accessor
/// standing at the member takes the write itself, so the slot never
/// sees it.
pub(super) fn steps_itself(f: &Facts<'_, '_>, value: ExprId, recv: ExprId, written: &str) -> bool {
    !f.unknown(written)
        && f.layouts().declare_number(written, written, true)
        && stepped(f, value, recv, written)
}

fn stepped(f: &Facts<'_, '_>, e: ExprId, recv: ExprId, written: &str) -> bool {
    use BinOp::*;
    match f.ast.get_expr(e) {
        // a literal is never -0: `-0` is a negation
        Expr::Number(n) => n.fract() == 0.0 && n.abs() <= 9007199254740992.0,
        Expr::Member { obj, name } => name == written && same_receiver(f.ast, *obj, recv),
        Expr::BinOp { op, left, right }
            if matches!(op, Add | Sub | BitAnd | BitOr | BitXor | Shl | Shr | UShr) =>
        {
            stepped(f, *left, recv, written) && stepped(f, *right, recv, written)
        }
        _ => false,
    }
}

/// Whether `value`, stored into the member `written` of the receiver
/// `recv`, is a number.
pub(super) fn number_value(f: &Facts<'_, '_>, value: ExprId, recv: ExprId, written: &str) -> bool {
    numeric(f, value, (recv, written), false)
}

fn numeric(f: &Facts<'_, '_>, e: ExprId, (recv, written): (ExprId, &str), arith: bool) -> bool {
    match f.expr_types.get(&e) {
        Some(Type::Number | Type::Literal(Lit::Num(_))) => return true,
        None | Some(Type::Any) => {}
        Some(_) => return false,
    }
    let at = (recv, written);
    match f.ast.get_expr(e) {
        Expr::Number(_) => true,
        Expr::Member { obj, name } => {
            same_receiver(f.ast, *obj, recv)
                && !f.unknown(name)
                && !(f.accessor)(name)
                && f.layouts().declare_number(written, name, arith)
        }
        Expr::BinOp { op, left, right } if arithmetic(*op) => {
            numeric(f, *left, at, true) && numeric(f, *right, at, true)
        }
        Expr::Unary {
            op: UnaryOp::Plus, ..
        } => true,
        Expr::Unary {
            op: UnaryOp::Neg | UnaryOp::BitNot,
            expr,
        } => numeric(f, *expr, at, true),
        _ => false,
    }
}

fn arithmetic(op: BinOp) -> bool {
    use BinOp::*;
    matches!(
        op,
        Add | Sub | Mul | Div | Mod | Pow | BitAnd | BitOr | BitXor | Shl | Shr | UShr
    )
}

/// The same receiver: `this` both times, or one binding.
fn same_receiver(ast: &Ast, a: ExprId, b: ExprId) -> bool {
    let this = |e: &Expr| matches!(e, Expr::This) || matches!(e, Expr::Ident(n) if n == "__this");
    match (ast.get_expr(a), ast.get_expr(b)) {
        (x, y) if this(x) && this(y) => true,
        (Expr::Ident(x), Expr::Ident(y)) => x == y,
        _ => false,
    }
}
