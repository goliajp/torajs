//! An array a program deletes from widens to `any[]` at its
//! declaration.
//!
//! ```text
//! var a = [1, 2, 3];
//! delete a[1];        // bun: true, a[1] undefined, a.length 3
//! ```
//!
//! tr refuses that program outright, and the refusal is correct about
//! the storage it was asked about: deleting is not writing a blank, it
//! has to make the index stop being an own property, and an unboxed
//! `number` slot has no value that means absent
//! ([`crate::check_type_of_misc`] spells this out). An `any`-element
//! array does have one — the hole shadow — and every own-property
//! consumer already agrees with bun on it (`in`, `Object.keys`,
//! `JSON.stringify`, `indexOf` all answer correctly on a hole today).
//!
//! So the declaration is the thing that is wrong, not the delete: a
//! binding the program deletes indices out of was never a `number[]`.
//! Rewriting it to the `: any[]` the user could have written is the
//! move [`super::with_object_widen`] and
//! [`super::objlit_nominal_anylane`] already make for their own
//! dynamic-by-contract bindings, and every existing lane follows —
//! the checker admits the delete, the init lowers through the boxed
//! element lane, and the hole machinery is reached.
//!
//! An explicit annotation is the user's word on the shape and stays.
//! By name, not scope-resolved — the same trade `with_object_widen`
//! documents: a same-named binding elsewhere can widen this one, which
//! costs that binding its unboxed element slots and nothing else.
//!
//! A binding [`crate::let_widen`] already claims — reassigned across
//! syntactic families, `var x = [0]; … x = {…}` — is left alone. That
//! pass types it `any`, which is wider than `any[]` and admits the
//! delete on its own; writing an annotation here would take the
//! binding out of its reach (it tracks UNANNOTATED lets) and turn a
//! program that ran into a refusal. Measured: the
//! `Array.prototype[1] = 1; var x = [0]; … x = {0: 0}` idiom behind
//! nine test262 cases. The claim is read off the prelude AST while
//! `let_widen` decides on the desugared one, so the two can disagree
//! about a shape the class desugar changes; disagreeing costs today's
//! answer and never a new wrong one.
//!
//! The same holds for an accessor. `Object.defineProperty(a, "0",
//! { get })` asks an index to stop being a data slot, and a typed cell
//! has nowhere to keep the pair: the runtime declined it silently, so
//! the getter never ran and the program went on reading the old
//! element. A binding the program installs an accessor on (or hands
//! to `defineProperties`, or to `defineProperty` with a descriptor
//! this pass cannot read) widens the same way.
//!
//! Runs in the prelude beside the other `delete` triage, before
//! anything reads element types.

use std::collections::HashSet;

use super::{Ast, Expr, ExprId, Stmt};

/// Names standing as the receiver of a `delete <name>[k]` /
/// `delete <name>.p`, then every unannotated `LetDecl` of such a name
/// whose init is an array literal rewritten to `: any[]`.
///
/// Since the typed delete landed (r704, `99cef557d`) a `number[]` and
/// a `string[]` keep a hole in their own slots (the F64 sentinel, the
/// Str sentinel), every reader answers it as undefined — the callback
/// loops through `hof_len_shrink`'s hole names, for-of through the
/// sentinel element table — so a delete alone no longer needs the
/// boxed lane for them: a literal whose elements are all numbers or
/// all strings stays typed. Still widened: booleans (no hole word in
/// the integer / bool lanes yet), a mixed literal (no one element
/// type), object / array cells (a pointer loop variable cannot tell a
/// hole from null), a binding deleted by a named key (`delete
/// a.length`), every accessor target below, and every literal of a
/// program that writes an index onto `Array.prototype` /
/// `Object.prototype` (a typed hole reads its slot, not the key that
/// fills it from the prototype).
pub fn widen_deleted_array_bindings(ast: &mut Ast) {
    let mut names: HashSet<String> = HashSet::new();
    let mut keep_widening: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        let Expr::Delete { expr } = e else {
            continue;
        };
        // a named key (`delete a.length`, `delete a["x"]`) is not an
        // element: the typed lanes have no hole for it to leave
        let (recv, element) = match &ast.exprs[expr.0 as usize] {
            Expr::Index { obj, index } => (
                *obj,
                !matches!(peel_as(&ast.exprs, *index), Expr::String(_)),
            ),
            Expr::Member { obj, .. } => (*obj, false),
            _ => continue,
        };
        if let Expr::Ident(n) = &ast.exprs[recv.0 as usize] {
            names.insert(n.clone());
            if !element {
                keep_widening.insert(n.clone());
            }
        }
    }
    for e in &ast.exprs {
        if let Some(n) = accessor_define_target(&ast.exprs, e) {
            names.insert(n.to_string());
            keep_widening.insert(n.to_string());
        }
    }
    if names.is_empty() {
        return;
    }
    let widen_all = ast.exprs.iter().any(|e| proto_index_write(&ast.exprs, e));
    let claimed = crate::let_widen::collect_cross_type_widen_inits(ast, None);
    let mut stmts = std::mem::take(&mut ast.stmts);
    // The mutable spine is FULLY recursive (each nested list handed to
    // the callback exactly once), so the per-list body must not
    // recurse itself.
    let marked = Marked {
        widen_all,
        names: &names,
        keep_widening: &keep_widening,
        claimed: &claimed,
    };
    widen_flat(&mut stmts, &ast.exprs, &marked);
    for s in stmts.iter_mut() {
        super::stmt_nested_lists::for_each_nested_vec_mut(s, &mut |inner| {
            widen_flat(inner, &ast.exprs, &marked)
        });
    }
    ast.stmts = stmts;
}

struct Marked<'a> {
    /// The program writes an index onto a builtin prototype.
    widen_all: bool,
    names: &'a HashSet<String>,
    /// Named-key deletes and accessor targets: widen whatever the
    /// elements are.
    keep_widening: &'a HashSet<String>,
    claimed: &'a HashSet<ExprId>,
}

fn widen_flat(stmts: &mut [Stmt], exprs: &[Expr], marked: &Marked<'_>) {
    for s in stmts.iter_mut() {
        let Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } = s
        else {
            continue;
        };
        if type_ann.is_some() || !marked.names.contains(name) || marked.claimed.contains(init) {
            continue;
        }
        let Expr::Array(els) = peel_as(exprs, *init) else {
            continue;
        };
        if marked.widen_all
            || marked.keep_widening.contains(name)
            || !holes_in_own_slots(exprs, els)
        {
            *type_ann = Some("any[]".to_string());
        }
    }
}

/// The literal's elements share one kind whose typed array keeps a
/// hole in its own slots: all numbers or all strings.
fn holes_in_own_slots(exprs: &[Expr], els: &[ExprId]) -> bool {
    #[derive(PartialEq)]
    enum Kind {
        Number,
        Str,
    }
    let kind = |e: ExprId| match peel_as(exprs, e) {
        Expr::Number(_) => Some(Kind::Number),
        Expr::Unary {
            op: super::UnaryOp::Neg,
            expr,
        } if matches!(exprs[expr.0 as usize], Expr::Number(_)) => Some(Kind::Number),
        Expr::String(_) => Some(Kind::Str),
        _ => None,
    };
    let Some(first) = els.first().and_then(|e| kind(*e)) else {
        return false;
    };
    els.iter().all(|e| kind(*e).as_ref() == Some(&first))
}

/// `Array.prototype[k] = v`, or a define on `Array.prototype` /
/// `Object.prototype`: an own-less index of any array may now read
/// through to it.
fn proto_index_write(exprs: &[Expr], e: &Expr) -> bool {
    let is_proto = |t: ExprId| {
        matches!(peel_as(exprs, t), Expr::Member { obj, name } if name == "prototype"
            && matches!(&exprs[obj.0 as usize], Expr::Ident(ns) if ns == "Array" || ns == "Object"))
    };
    match e {
        Expr::Assign { target, .. } => {
            matches!(&exprs[target.0 as usize], Expr::Index { obj, .. } if is_proto(*obj))
        }
        Expr::Call { callee, args } => {
            matches!(&exprs[callee.0 as usize], Expr::Member { obj, name }
                if matches!(name.as_str(), "defineProperty" | "defineProperties")
                    && matches!(&exprs[obj.0 as usize], Expr::Ident(ns) if ns == "Object" || ns == "Reflect"))
                && args.first().is_some_and(|a| is_proto(*a))
        }
        _ => false,
    }
}

/// TS casts are static assertions — look at the value underneath.
fn peel_as<'e>(exprs: &'e [Expr], mut e: ExprId) -> &'e Expr {
    loop {
        match &exprs[e.0 as usize] {
            Expr::As { expr, .. } => e = *expr,
            other => return other,
        }
    }
}

/// The binding an `Object.defineProperty` / `Reflect.defineProperty`
/// call may install an accessor on, or an `Object.defineProperties`
/// call targets. A literal descriptor with neither `get` nor `set` is
/// a data definition the typed cell holds already.
fn accessor_define_target<'e>(exprs: &'e [Expr], e: &Expr) -> Option<&'e str> {
    let Expr::Call { callee, args } = e else {
        return None;
    };
    let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
        return None;
    };
    let Expr::Ident(ns) = &exprs[obj.0 as usize] else {
        return None;
    };
    let target = match (ns.as_str(), name.as_str()) {
        ("Object", "defineProperties") => args.first()?,
        ("Object" | "Reflect", "defineProperty") => {
            let reads_accessor = match args.get(2).map(|d| peel_as(exprs, *d)) {
                Some(Expr::ObjectLit { fields }) => fields
                    .iter()
                    .any(|(k, _)| matches!(k.lossy().as_ref(), "get" | "set")),
                Some(_) => true,
                None => false,
            };
            if !reads_accessor {
                return None;
            }
            args.first()?
        }
        _ => return None,
    };
    match peel_as(exprs, *target) {
        Expr::Ident(n) => Some(n),
        _ => None,
    }
}
