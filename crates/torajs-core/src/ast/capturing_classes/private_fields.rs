//! Private instance FIELDS of a class in the runtime-value lane (RFC
//! 20260917-class-eval-unit K6-2).
//!
//! The lane builds its instances as plain objects: no layout slot, no
//! install cursor. The parser's constructor prefix spells a private
//! field's initializer as an ordinary write, `this.#x = init`, and a
//! private WRITE is brand-gated (§7.3.32 step 1) — so the very first
//! one threw `cannot write private member to an object whose class did
//! not declare it`, and no class with a private field could be
//! constructed here at all. What the prefix means is PrivateFieldAdd
//! (§7.3.34 step 3), which for an object the class did not lay out is
//! the foreign store (`torajs-anyvalue` `private_foreign`); every
//! reader, writer and `#x in o` probe already consults it first.
//!
//! So each prefix write becomes `__torajs_priv_field_define(this,
//! "__priv_C__x", init)`, a field with no initializer is defined
//! `undefined` where the prefix starts, and the `add` markers (which
//! the class desugar turns into layout installs) fall away. A write
//! the PROGRAM put in the constructor body stays a write.
//!
//! A REAL class reached as a runtime-value parent has the same prefix
//! in its `__ctorany_` twin, run on the lane's plain-object `this`
//! ([`define_in_twin`]); the kernel keeps the brand-gated write for a
//! receiver whose layout declares the field.

use super::super::private_brand::{FIELD_DEFINE, is_field_add};
use super::super::{Ast, Expr, PropKey, Stmt};

/// Rewrite the initializer prefix of `body` (the constructor body as
/// `extends::ctor_parts` answered it). `src_name` is the class's
/// parse-time name, the key of the parser's two prefix tables;
/// `forwards` says the body opens with the implicit `super(...args)`
/// forward (a derived class that wrote no constructor).
pub(super) fn define_in_prefix(
    ast: &mut Ast,
    fields: &[(PropKey, String)],
    (src_name, forwards): (&str, bool),
    body: &mut Vec<Stmt>,
) {
    let start = prefix_start(ast, body, forwards);
    define_range(ast, fields, (src_name, start), Expr::This, body);
}

/// The same rewrite over the body of root class `cname`'s
/// `__ctorany_` twin, whose receiver pass 2 already spelled `__this`.
pub(in crate::ast) fn define_in_twin(
    ast: &mut Ast,
    cname: &str,
    fields: &[(PropKey, String)],
    body: &mut Vec<Stmt>,
) {
    define_range(ast, fields, (cname, 0), Expr::Ident("__this".into()), body);
}

fn define_range(
    ast: &mut Ast,
    fields: &[(PropKey, String)],
    (src_name, start): (&str, usize),
    recv: Expr,
    body: &mut Vec<Stmt>,
) {
    let private: Vec<&str> = fields
        .iter()
        .filter_map(|(k, _)| k.as_str())
        .filter(|n| n.starts_with("__priv_") && *n != super::super::class_unit::UNIT_SLOT)
        .collect();
    if private.is_empty() {
        return;
    }
    let end = if ast.field_init_synth_ctors.contains(src_name) {
        body.len()
    } else {
        ast.ctor_field_init_end
            .get(src_name)
            .map_or(start, |e| (*e).clamp(start, body.len()))
    };
    let mut defined: Vec<String> = Vec::new();
    for s in &body[start..end] {
        let Stmt::Expr(eid) = s else { continue };
        if is_field_add(ast, *eid) {
            ast.exprs[eid.0 as usize] = Expr::Ident("undefined".into());
            continue;
        }
        let Expr::Assign { target, value } = ast.get_expr(*eid) else {
            continue;
        };
        let (target, value) = (*target, *value);
        let Expr::Member { obj, name } = ast.get_expr(target) else {
            continue;
        };
        let on_this = match ast.get_expr(*obj) {
            Expr::This => true,
            Expr::Ident(n) => n == "__this",
            _ => false,
        };
        if !on_this || !private.contains(&name.as_str()) {
            continue;
        }
        let (obj, name) = (*obj, name.clone());
        ast.exprs[eid.0 as usize] = define_call(ast, obj, &name, value);
        ast.tombstone_expr(target);
        defined.push(name);
    }
    let bare: Vec<Stmt> = private
        .iter()
        .filter(|n| !defined.iter().any(|d| d == *n))
        .map(|n| {
            let this = ast.add_expr(recv.clone());
            let undef = ast.add_expr(Expr::Ident("undefined".into()));
            let call = define_call(ast, this, n, undef);
            Stmt::Expr(ast.add_expr(call))
        })
        .collect();
    body.splice(start..start, bare);
}

/// Where the prefix starts: past the forward call of an implicit
/// derived constructor; otherwise where the parser spliced it — past
/// the first statement-level `super(…)`, or at the top when there is
/// none (a base class, or a `super` buried in a branch).
fn prefix_start(ast: &Ast, body: &[Stmt], forwards: bool) -> usize {
    if forwards {
        return 1;
    }
    body.iter()
        .position(|s| matches!(s, Stmt::Expr(e) if matches!(ast.get_expr(*e), Expr::Super { .. })))
        .map_or(0, |i| i + 1)
}

/// `__torajs_priv_field_define(<recv>, "<name>", <value>)` — PrivateFieldAdd
/// (§7.3.34 step 3) of one private field onto `recv`.
pub(super) fn define_call(
    ast: &mut Ast,
    this: super::super::ExprId,
    name: &str,
    value: super::super::ExprId,
) -> Expr {
    let callee = ast.add_expr(Expr::Ident(FIELD_DEFINE.into()));
    let key = ast.add_expr(Expr::String(name.into()));
    Expr::Call {
        callee,
        args: vec![this, key, value],
    }
}
