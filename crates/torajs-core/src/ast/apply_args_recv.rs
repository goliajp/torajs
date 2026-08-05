//! Receiver ground truth for `apply_default_args`' Member-arm
//! padding gate — split out of `apply_args.rs` (file-size limit)
//! when the receiver-precise suppression landed.

use std::collections::HashMap;

use super::{Ast, Expr, ExprId, Stmt};

/// Which class a receiver expression is provably an instance of, or
/// None when nothing here can tell.
///
/// `desugar_classes` has already rewritten `new C(..)` into a call to
/// the synthesized factory `__new_C`, so a direct `new C().m()` names
/// its class right there in the receiver, and a receiver bound to one
/// carries it as far as the binding map below is trusted. A call of a
/// THIN FACTORY (`factories`, below) names its class the same way,
/// one level of indirection out.
pub(super) fn receiver_class(
    ast: &Ast,
    obj: ExprId,
    recv_class: &HashMap<String, String>,
    factories: &HashMap<String, String>,
) -> Option<String> {
    match ast.get_expr(obj) {
        Expr::Call { callee, .. } => match ast.get_expr(*callee) {
            Expr::Ident(f) => f
                .strip_prefix("__new_")
                .map(str::to_string)
                .or_else(|| factories.get(f).cloned()),
            _ => None,
        },
        Expr::Ident(n) => recv_class.get(n).cloned(),
        // A hoisted generator local, read back as a field of the
        // enclosing `__Gen_*` — the desugar kept its declared class
        // because the `let` that carried it is gone.
        Expr::Member { name, .. } => ast.generator_local_classes.get(name).cloned(),
        _ => None,
    }
}

/// Thin factories, as `name → C`: calling one hands back a `C`, so a
/// receiver bound to its result is as precisely resolved as one bound
/// to `new C()` directly — the indirection is the only difference.
///
/// Two sources, because they cover different authors:
///
/// 1. **`ast.generator_factory_classes`.** `desugar_generators` turns
///    `function* g()` into a `__Gen_g` class plus the thin factory
///    `function g(args) { return new __Gen_g(args); }`, and records
///    the pairing as it goes. Reading the record rather than
///    re-deriving it from the body matters: a generator with a `try`
///    region no longer has a single-`return` factory body by the time
///    this pass runs, and shape inference alone therefore answered for
///    plain generators but not for those (measured: `pk1.ts`, two
///    generators differing only by an empty `try`/`finally`).
/// 2. **A body that is exactly `return new C(..)`**, for the same
///    thing written by hand.
///
/// Without either, `const it = g(); it.next()` had an unresolvable
/// receiver and could only ask the name-keyed table — so one class
/// declaring its own `next(step = 5)` evicted the shared `next` entry
/// and took every generator in the program down with it, while the
/// neighbouring `c.next()` resolved through its own class and worked.
pub(super) fn collect_factory_classes(ast: &Ast) -> HashMap<String, String> {
    let empty = HashMap::new();
    let mut out = ast.generator_factory_classes.clone();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && let [Stmt::Return(Some(ret))] = body.as_slice()
            && let Some(class) = receiver_class(ast, *ret, &empty, &empty)
        {
            out.insert(name.clone(), class);
        }
    }
    out
}

/// The default list a Member call site `obj.name(..)` should pad with,
/// or None to leave the site unpadded.
///
/// Three sources, most precise first:
///
/// 1. **The receiver's own class.** `__cm_C__name`'s defaults ARE the
///    answer when the receiver is provably a `C`. This outranks the
///    name-keyed table, which cannot serve two owners whose defaults
///    disagree: `class A { m(x = 1) {} }` beside
///    `class B { m(x = 99) {} }` evicted `m` outright, and `a.m()`
///    then failed to compile at all ("expected 1 argument(s), got 0")
///    on a program every engine runs. Same shape as the ObjectLit
///    gate below — resolve against the receiver when the receiver can
///    be resolved, and only ask the shared table when it cannot.
/// 2. **A provably-unique ObjectLit-bound receiver's own field** — an
///    own method without defaults, or a plain value field, means NO
///    padding (honest beats a wrong default).
/// 3. **The name-keyed table**, for receivers neither of the above
///    can pin down.
pub(super) fn member_call_defaults(
    ast: &Ast,
    obj: ExprId,
    name: &str,
    recv_fields: &HashMap<String, HashMap<String, Option<String>>>,
    recv_class: &HashMap<String, String>,
    factories: &HashMap<String, String>,
    fn_defaults: &HashMap<String, Vec<Option<ExprId>>>,
    method_defaults: &HashMap<String, Vec<Option<ExprId>>>,
) -> Option<Vec<Option<ExprId>>> {
    if let Some(class) = receiver_class(ast, obj, recv_class, factories)
        && let Some(d) = fn_defaults.get(&format!("__cm_{class}__{name}"))
    {
        // `__cm_C__M`'s first param is the `__this` receiver, which no
        // Member call site passes — same slice the name-keyed merge takes.
        return d.get(1..).map(<[Option<ExprId>]>::to_vec);
    }
    let own = match ast.get_expr(obj) {
        Expr::Ident(recv) => recv_fields.get(recv),
        _ => None,
    };
    match own {
        Some(fields) => match fields.get(name) {
            Some(Some(fname)) => fn_defaults.get(fname).cloned(),
            Some(None) => None,
            None => method_defaults.get(name).cloned(),
        },
        None => method_defaults.get(name).cloned(),
    }
}

/// Receiver ground truth for the Member-arm padding gate: walk every
/// statement container recursing into fn bodies, counting EVERY
/// binding occurrence per name (let/const decls, params, for-of loop
/// vars, catch params) and recording the field map of each
/// ObjectLit-init let (field name → `Some(closure fn name)` for
/// method-shaped fields, `None` for plain value fields). The caller
/// only trusts a field map when the name's total binding count is 1.
pub(super) fn collect_objlit_recv_fields(
    ast: &Ast,
    stmts: &[Stmt],
    classify: &impl Fn(&Expr) -> Option<String>,
    factories: &HashMap<String, String>,
    counts: &mut HashMap<String, usize>,
    objlit: &mut HashMap<String, HashMap<String, Option<String>>>,
    classes: &mut HashMap<String, String>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                *counts.entry(name.clone()).or_insert(0) += 1;
                if let Expr::ObjectLit { fields } = ast.get_expr(*init) {
                    let fm = fields
                        .iter()
                        .map(|(f, feid)| (f.clone(), classify(ast.get_expr(*feid))))
                        .collect();
                    objlit.insert(name.clone(), fm);
                }
                if let Some(c) = receiver_class(ast, *init, &HashMap::new(), factories) {
                    classes.insert(name.clone(), c);
                }
            }
            Stmt::FnDecl { params, body, .. } => {
                for p in params {
                    *counts.entry(p.name.clone()).or_insert(0) += 1;
                }
                collect_objlit_recv_fields(ast, body, classify, factories, counts, objlit, classes);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                collect_objlit_recv_fields(
                    ast, inner, classify, factories, counts, objlit, classes,
                );
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(then_branch),
                    classify,
                    factories,
                    counts,
                    objlit,
                    classes,
                );
                if let Some(eb) = else_branch {
                    collect_objlit_recv_fields(
                        ast,
                        core::slice::from_ref(eb),
                        classify,
                        factories,
                        counts,
                        objlit,
                        classes,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(body),
                    classify,
                    factories,
                    counts,
                    objlit,
                    classes,
                );
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_objlit_recv_fields(
                        ast,
                        core::slice::from_ref(i),
                        classify,
                        factories,
                        counts,
                        objlit,
                        classes,
                    );
                }
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(body),
                    classify,
                    factories,
                    counts,
                    objlit,
                    classes,
                );
            }
            Stmt::ForOf {
                var_name,
                i_ident,
                body,
                ..
            } => {
                *counts.entry(var_name.clone()).or_insert(0) += 1;
                *counts.entry(i_ident.clone()).or_insert(0) += 1;
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(body),
                    classify,
                    factories,
                    counts,
                    objlit,
                    classes,
                );
            }
            Stmt::ForOfSplitIter { var_name, body, .. } => {
                *counts.entry(var_name.clone()).or_insert(0) += 1;
                collect_objlit_recv_fields(
                    ast,
                    core::slice::from_ref(body),
                    classify,
                    factories,
                    counts,
                    objlit,
                    classes,
                );
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                if let Some(cp) = catch_param {
                    *counts.entry(cp.clone()).or_insert(0) += 1;
                }
                collect_objlit_recv_fields(ast, body, classify, factories, counts, objlit, classes);
                collect_objlit_recv_fields(
                    ast, catch_body, classify, factories, counts, objlit, classes,
                );
                if let Some(fb) = finally_body {
                    collect_objlit_recv_fields(
                        ast, fb, classify, factories, counts, objlit, classes,
                    );
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_objlit_recv_fields(
                        ast, &c.body, classify, factories, counts, objlit, classes,
                    );
                }
                if let Some(d) = default {
                    collect_objlit_recv_fields(
                        ast, d, classify, factories, counts, objlit, classes,
                    );
                }
            }
            _ => {}
        }
    }
}
