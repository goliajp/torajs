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

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};
use crate::check::{Lit, Type};

/// What a layout declares a member to hold.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Num {
    Number,
    /// `number | undefined`: arithmetic on it is a number, a copy of
    /// it may be `undefined`.
    NumberOrUndefined,
    Other,
}

/// Every layout of the program, by its members: the declared types and
/// classes, and the structs the checker inferred.
pub(super) struct Layouts {
    sets: Vec<HashMap<String, Num>>,
}

impl Layouts {
    pub(super) fn of(ast: &Ast, expr_types: &HashMap<ExprId, Type>) -> Self {
        let mut seen: HashSet<Vec<(String, Num)>> = HashSet::new();
        for s in &ast.stmts {
            if let Stmt::TypeDecl { fields, .. } = s {
                let set = fields.iter().filter_map(|(k, ann)| {
                    let num = match ann.trim() {
                        "number" => Num::Number,
                        crate::ssa_lower_parse_type::OPTIONAL_NUMBER_ANN => Num::NumberOrUndefined,
                        _ => Num::Other,
                    };
                    Some((k.as_wtf8().as_str()?.to_string(), num))
                });
                seen.insert(set.collect());
            }
        }
        for t in expr_types.values() {
            collect(t, &mut seen);
        }
        Layouts {
            sets: seen.into_iter().map(|s| s.into_iter().collect()).collect(),
        }
    }

    /// Whether every layout holding the member `written` declares
    /// `read` a number (`number | undefined` too, when `or_undefined`).
    fn declare_number(&self, written: &str, read: &str, or_undefined: bool) -> bool {
        self.sets
            .iter()
            .filter(|s| s.contains_key(written))
            .all(|s| match s.get(read) {
                Some(Num::Number) => true,
                Some(Num::NumberOrUndefined) => or_undefined,
                _ => false,
            })
    }
}

fn collect(t: &Type, seen: &mut HashSet<Vec<(String, Num)>>) {
    match t {
        Type::Struct(fs) => {
            let set = fs.iter().filter_map(|(k, ft)| {
                let num = match ft {
                    Type::Number | Type::Literal(Lit::Num(_)) => Num::Number,
                    Type::Nullable(n) if matches!(**n, Type::Number) => Num::NumberOrUndefined,
                    _ => Num::Other,
                };
                Some((k.as_wtf8().as_str()?.to_string(), num))
            });
            seen.insert(set.collect());
            for (_, ft) in fs.iter() {
                collect(ft, seen);
            }
        }
        Type::Array(t) | Type::Nullable(t) => collect(t, seen),
        Type::Union(ts) => {
            for t in ts {
                collect(t, seen);
            }
        }
        _ => {}
    }
}

/// What a number-ness verdict reads besides the expression.
pub(super) struct Facts<'a> {
    pub(super) ast: &'a Ast,
    pub(super) expr_types: &'a HashMap<ExprId, Type>,
    pub(super) layouts: &'a Layouts,
    /// The members a write can leave holding a value of any kind.
    pub(super) unknown: &'a HashSet<String>,
    /// Whether an accessor can stand at the member.
    pub(super) accessor: &'a dyn Fn(&str) -> bool,
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
pub(super) fn steps_itself(f: &Facts<'_>, value: ExprId, recv: ExprId, written: &str) -> bool {
    !f.unknown.contains(written)
        && f.layouts.declare_number(written, written, true)
        && stepped(f, value, recv, written)
}

fn stepped(f: &Facts<'_>, e: ExprId, recv: ExprId, written: &str) -> bool {
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
pub(super) fn number_value(f: &Facts<'_>, value: ExprId, recv: ExprId, written: &str) -> bool {
    numeric(f, value, (recv, written), false)
}

fn numeric(f: &Facts<'_>, e: ExprId, (recv, written): (ExprId, &str), arith: bool) -> bool {
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
                && !f.unknown.contains(name)
                && !(f.accessor)(name)
                && f.layouts.declare_number(written, name, arith)
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
