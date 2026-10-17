//! The `exec` half of the shadow scan: whether the program may give
//! some object an own `exec`, which RegExpExec (§22.2.7.1) calls in
//! place of the builtin matcher.

use crate::ssa::id_hash::HashSet;

use super::ShadowSet;
use super::scan::str_literal;
use crate::ast::{Ast, Expr, ExprId};

/// Any write, delete or definition of a property named `exec`,
/// whatever object it lands on, and any `__proto__` write (see
/// `ShadowSet::exec_written`). A definition whose key the program does
/// not spell — a `defineProperty` / `Reflect.set` key that is no
/// string literal, a descriptor map or `Object.assign` source that is
/// no object literal, or one with a computed or spread member — may
/// name `exec` too, and so may a writing `Object` / `Reflect` static
/// the program holds as a value or reads under a key it does not
/// spell ([`reflective_escape`]). A computed-key assignment `x[k] = v`
/// is left to the checker's types (`typed_exec`): every array store
/// has that shape.
pub(super) fn note_exec_writes(
    ast: &Ast,
    set: &mut ShadowSet,
    assign_targets: &HashSet<ExprId>,
    delete_targets: &HashSet<ExprId>,
) {
    let names = |eid: &ExprId, key: &str| match ast.get_expr(*eid) {
        Expr::Member { name, .. } => name == key,
        Expr::Index { index, .. } => str_literal(ast, *index).as_deref() == Some(key),
        _ => false,
    };
    let written = assign_targets
        .iter()
        .chain(delete_targets)
        .any(|e| names(e, "exec"))
        || assign_targets.iter().any(|e| names(e, "__proto__"))
        || ast.exprs.iter().any(|e| match e {
            Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                reflective_exec_write(ast, *callee, args)
            }
            _ => false,
        })
        || reflective_escape(ast);
    if written {
        set.set_exec_written();
    }
}

/// `e` with its `as` layers peeled: a cast passes the value through.
fn strip_as(ast: &Ast, mut e: ExprId) -> ExprId {
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    e
}

/// `Object` or `Reflect` when `e` names that namespace: the bare
/// identifier, or a read of that name off another object
/// (`globalThis.Object`).
fn namespace_of(ast: &Ast, e: ExprId) -> Option<&'static str> {
    let pick = |n: &str| ["Object", "Reflect"].into_iter().find(|ns| *ns == n);
    match ast.get_expr(strip_as(ast, e)) {
        Expr::Ident(n) | Expr::Member { name: n, .. } | Expr::OptChain { name: n, .. } => pick(n),
        Expr::Index { index, .. } | Expr::OptIndex { index, .. } => {
            str_literal(ast, *index).and_then(|k| pick(&k))
        }
        _ => None,
    }
}

/// The statics of `ns` that can define or write an own property.
fn writes_property(ns: &str, name: &str) -> bool {
    matches!(
        (ns, name),
        ("Object" | "Reflect", "defineProperty")
            | ("Object", "defineProperties" | "assign")
            | ("Reflect", "set")
    )
}

/// The namespace and the spelled static name a read `e` of a static
/// names (`Object.assign`, `Reflect["set"]`, `Object?.assign`), with
/// `None` for the name when the key is not spelled.
fn static_read(ast: &Ast, e: ExprId) -> Option<(&'static str, Option<String>)> {
    match ast.get_expr(strip_as(ast, e)) {
        Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
            Some((namespace_of(ast, *obj)?, Some(name.clone())))
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            Some((namespace_of(ast, *obj)?, str_literal(ast, *index)))
        }
        _ => None,
    }
}

/// Whether `Object.defineProperty / defineProperties / assign` or
/// `Reflect.defineProperty / set` called with `args` may define `exec`.
fn reflective_exec_write(ast: &Ast, callee: ExprId, args: &[ExprId]) -> bool {
    let Some((ns, Some(name))) = static_read(ast, callee) else {
        return false;
    };
    let key_may_be_exec = |i: usize| {
        args.get(i)
            .is_some_and(|&k| str_literal(ast, k).is_none_or(|k| k == "exec"))
    };
    match (ns, name.as_str()) {
        ("Object" | "Reflect", "defineProperty") | ("Reflect", "set") => key_may_be_exec(1),
        ("Object", "defineProperties") => args.get(1).is_some_and(|&d| lit_may_hold_exec(ast, d)),
        // `Object.assign({}, …)` copies onto a fresh object, no RegExp
        ("Object", "assign") => {
            args.first()
                .is_some_and(|&t| !matches!(ast.get_expr(t), Expr::ObjectLit { .. }))
                && args.iter().skip(1).any(|&s| lit_may_hold_exec(ast, s))
        }
        _ => false,
    }
}

/// Whether a writing `Object` / `Reflect` static can be called through
/// a spelling [`reflective_exec_write`] never sees: the namespace held
/// as a value (`const O = Object`, `const { assign } = Object`, an
/// argument), a writing static read as a value
/// (`const dp = Object.defineProperty`, `Object.assign.call(…)`), or a
/// static read under a key the program does not spell (`Object[k]`).
fn reflective_escape(ast: &Ast) -> bool {
    // every escape spelling has a namespace read under it; most
    // programs name neither, and the position sets below cost a hash
    // insert per member read on every scan
    let mut ids = (0..ast.exprs.len() as u32).map(ExprId);
    if !ids.any(|eid| namespace_of(ast, eid).is_some()) {
        return false;
    }
    let mut read_through: HashSet<ExprId> = HashSet::default();
    let mut callees: HashSet<ExprId> = HashSet::default();
    let peel = |mut e: ExprId, into: &mut HashSet<ExprId>| {
        into.insert(e);
        while let Expr::As { expr, .. } = ast.get_expr(e) {
            e = *expr;
            into.insert(e);
        }
    };
    for e in &ast.exprs {
        match e {
            Expr::Member { obj, .. }
            | Expr::Index { obj, .. }
            | Expr::OptChain { obj, .. }
            | Expr::OptIndex { obj, .. } => peel(*obj, &mut read_through),
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => peel(*callee, &mut callees),
            // operators that read the value and hand it to no one
            Expr::InstanceOf { rhs: e, .. } | Expr::TypeOf { expr: e } => {
                peel(*e, &mut read_through)
            }
            _ => {}
        }
    }
    (0..ast.exprs.len() as u32).map(ExprId).any(|eid| {
        if !read_through.contains(&eid) && namespace_of(ast, eid).is_some() {
            return true;
        }
        match static_read(ast, eid) {
            Some((_, None)) => true,
            Some((ns, Some(name))) => writes_property(ns, &name) && !callees.contains(&eid),
            None => false,
        }
    })
}

/// Whether the object `e` may carry an own `exec`: anything but an
/// object literal, or one with an `exec` member (plain or accessor), a
/// computed key or a spread.
fn lit_may_hold_exec(ast: &Ast, e: ExprId) -> bool {
    let Expr::ObjectLit { fields } = ast.get_expr(e) else {
        return true;
    };
    fields.iter().any(|(k, _)| {
        let k = k.lossy();
        k == "exec"
            || k == "__getter_exec"
            || k == "__setter_exec"
            || k.starts_with("__computed_")
            || k.starts_with("__spread")
    })
}
