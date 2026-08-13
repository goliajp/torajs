//! Function-expression `this` in a slot the spec calls with NO
//! receiver — bind it the way a plain function binds it.
//!
//! `bind_this_param` gives every top-level `function` mentioning
//! `this` a `__this` param, and the fn-to-closure wrap
//! (`__forward_<name>`) hardwires `undefined` into it, so a NAMED
//! callback reading `this` works today. The same body spelled as an
//! EXPRESSION does not: `lift_arrow_fns` lists the body's `__this` as
//! a capture and the checker rejects (`closure __closure_N references
//! unknown identifier __this`). The answer therefore depended on
//! whether the callback was written `function cb() {…}` or
//! `function () {…}` — measured on `Promise.resolve(1).then(…)`:
//! the named spelling answered `undefined`, the expression spelling
//! refused to compile.
//!
//! **The admitted set is a SLOT TABLE, not "whatever is left".** A
//! blanket binding was written first and reverted in-knife: it turned
//! `arr.forEach(function () { … this … }, obj)` from today's loud
//! reject into `this === undefined` (the knife-4 promote declines an
//! `any[]`-typed receiver binding, L3b), and it would do the same to
//! `JSON.stringify(v, function (k, v) { … this … })`, whose replacer
//! is called with the HOLDER as `this` (§25.5.2.2). Over-refusal
//! keeps the loud reject; a wrong `undefined` is the silent wrong the
//! design principles rank worst. So each slot is admitted only after
//! reading what the spec passes there.
//!
//! Admitted so far (each entry read off the spec text that invokes
//! it, and each MEASURED against HEAD first — a slot that already
//! answered `undefined` needs no arm here. The Promise executor and
//! Map / Set `forEach` were written and removed for exactly that
//! reason: their callbacks reach the callee through a plain user-fn
//! argument position, where the `__forward_` shim already supplies
//! the receiver-less answer):
//!
//! * `Object.groupBy(items, cb)` / `Map.groupBy` — §7.3.35 step 4.c.
//!
//! * `p.then(f)` / `p.then(f, g)` / `p.catch(f)` / `p.finally(f)` —
//!   §27.2.5.4 step 9's `Call(handler, undefined, «argument»)`, and
//!   the same in §27.2.5.1 / §27.2.5.3. The receiver has to be a
//!   SYNTACTICALLY CERTAIN promise (a `Promise` static call, `new
//!   Promise`, or a chain over one): a user object with its own
//!   `then` method decides for itself how it calls the handler, and
//!   `f.call(this)` in that method would make `undefined` wrong.
//! * The array iteration callbacks over a SYNTACTICALLY CERTAIN
//!   array receiver (an array literal, or a binding `lift_arrow_fns`'
//!   sibling proves only ever holds one). Two sub-shapes, and the
//!   difference between them is the whole reason the blanket version
//!   was wrong:
//!   - `sort` / `toSorted` comparators (§23.1.3.30 step 5's
//!     `Call(comparator, undefined, «x, y»)`) and `reduce` /
//!     `reduceRight` callbacks (§23.1.3.24 step 6's `Call(callbackfn,
//!     undefined, …)`) take no thisArg at all, so any argument count
//!     admits;
//!   - `forEach` / `map` / `filter` / `some` / `every` / the `find`
//!     family / `flatMap` DO take one (§23.1.3.{8,15,20,…}), so they
//!     admit only when the call site passed none — with a thisArg
//!     present the answer is that object, which is exactly the case
//!     the blanket version got wrong.
//!
//! Only function expressions (`ast.fn_expr_exprs`) take the binding.
//! An ARROW inherits its enclosing `this` (§8.3.4) and is already
//! correct through the capture chain; an object-literal method binds
//! the receiver and is `objlit_nominal`'s to claim.
//!
//! Nothing about the closure's ABI moves — the binding is a
//! body-local declaration, so arity, the boxed dual entry, and the
//! argv face are untouched. The `__env` annotation is rewritten
//! alongside the capture list, since it spells the env layout the
//! lowering packs.
//!
//! Runs after `desugar_implicit_generics`, which is where the
//! receiver-promoting knives live: a slot one of them claimed has no
//! `__this` capture left by the time this looks.

use super::fnexpr_this_recvs::collect_arraylit_binding_names;
use super::sloppy_this_prologue::has_use_strict_directive;
use super::{Ast, Expr, ExprId, Stmt};

/// The `Promise` statics whose result is a promise — the roots of a
/// syntactically certain promise chain.
const PROMISE_STATICS: [&str; 7] = [
    "resolve",
    "reject",
    "all",
    "allSettled",
    "any",
    "race",
    "try",
];

/// The prototype methods that both take no-receiver handlers and
/// answer a promise, so a chain over them stays certain.
const HANDLER_METHODS: [&str; 3] = ["then", "catch", "finally"];

/// Array iteration methods whose callback NEVER takes a thisArg —
/// the spec hands them `undefined` whatever the call site wrote.
const ARRAY_NO_THISARG: [&str; 4] = ["sort", "toSorted", "reduce", "reduceRight"];

/// Array iteration methods whose callback takes an OPTIONAL thisArg
/// as the second argument: `undefined` is the answer only when the
/// call site omitted it.
const ARRAY_OPTIONAL_THISARG: [&str; 10] = [
    "forEach",
    "map",
    "filter",
    "some",
    "every",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flatMap",
];

pub fn bind_fnexpr_this_default(ast: &mut Ast) {
    let targets = collect_targets(ast);
    if targets.is_empty() {
        return;
    }
    for (eid, _) in &targets {
        if let Expr::Closure { captures, .. } = &mut ast.exprs[eid.0 as usize] {
            captures.retain(|c| c != "__this");
        }
    }
    for (eid, fn_name) in targets {
        let caps = match &ast.exprs[eid.0 as usize] {
            Expr::Closure { captures, .. } => captures.clone(),
            _ => continue,
        };
        // The goal answer is read before the mutable walk: the mint
        // needs an `&mut Ast` and the directive probe an `&Ast`.
        let global_this = ast.sloppy_script_goal
            && ast.stmts.iter().any(|s| {
                matches!(s, Stmt::FnDecl { name, body, .. }
                    if name == &fn_name && !has_use_strict_directive(body, &ast.exprs))
            });
        let init = if global_this {
            // §10.2.1.2 step 4's ToObject over step 6's global
            // object, spelled the way `sloppy_this_prologue` spells
            // it for the promoted bodies.
            let object = ast.add_expr(Expr::Ident("Object".into()));
            let global = ast.add_expr(Expr::Ident("globalThis".into()));
            ast.add_expr(Expr::Call {
                callee: object,
                args: vec![global],
            })
        } else {
            ast.add_expr(Expr::Ident("undefined".into()))
        };
        for s in ast.stmts.iter_mut() {
            let Stmt::FnDecl {
                name, params, body, ..
            } = s
            else {
                continue;
            };
            if name != &fn_name {
                continue;
            }
            if let Some(env) = params.first_mut()
                && env.name == "__env"
            {
                env.type_ann = Some(format!("__env({})", caps.join("|")));
            }
            body.insert(
                0,
                Stmt::LetDecl {
                    mutable: false,
                    name: "__this".into(),
                    type_ann: Some("any".into()),
                    init,
                    is_var: false,
                },
            );
            break;
        }
    }
}

/// Every unclaimed function expression sitting in a no-receiver slot,
/// paired with the name of the FnDecl `lift_arrow_fns` made of it.
fn collect_targets(ast: &Ast) -> Vec<(ExprId, String)> {
    let arrays = collect_arraylit_binding_names(&ast.stmts, &ast.exprs);
    let mut targets: Vec<(ExprId, String)> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let Expr::Member { obj, name } = &ast.exprs[callee.0 as usize] else {
            continue;
        };
        for slot in no_receiver_slots(ast, &arrays, *obj, name, args) {
            if let Some(fn_name) = unclaimed_fnexpr(ast, slot) {
                targets.push((slot, fn_name));
            }
        }
    }
    targets
}

/// The argument positions of `recv.<name>(args…)` that the spec
/// invokes with no receiver — empty for every call this table does
/// not name, which is what keeps an unaudited slot on its loud
/// reject.
fn no_receiver_slots(
    ast: &Ast,
    arrays: &std::collections::HashSet<String>,
    obj: ExprId,
    name: &str,
    args: &[ExprId],
) -> Vec<ExprId> {
    // §7.3.35 GroupBy step 4.c — `Call(callback, undefined, «value,
    // key»)`, reached through `Object.groupBy` / `Map.groupBy`.
    if name == "groupBy"
        && matches!(&ast.exprs[obj.0 as usize], Expr::Ident(n) if n == "Object" || n == "Map")
    {
        return args.iter().skip(1).take(1).copied().collect();
    }
    if HANDLER_METHODS.contains(&name) && promise_certain(&ast.exprs, obj) {
        // `then` takes two handler slots; `catch` / `finally` one.
        // Anything past them is not a handler and is left alone.
        let slots = if name == "then" { 2 } else { 1 };
        return args.iter().take(slots).copied().collect();
    }
    if !array_certain(&ast.exprs, arrays, obj) {
        return Vec::new();
    }
    let admits = ARRAY_NO_THISARG.contains(&name)
        || (ARRAY_OPTIONAL_THISARG.contains(&name) && args.len() == 1);
    if admits {
        args.iter().take(1).copied().collect()
    } else {
        Vec::new()
    }
}

/// `true` when `eid` is syntactically certain to be an array: a
/// literal, or a binding the knife-2 receiver census proves holds
/// only array literals. Same bar the promote knives use — a bare
/// binding that might hold a user object with its own `map` would
/// let that object's method decide the callback's receiver.
fn array_certain(exprs: &[Expr], arrays: &std::collections::HashSet<String>, eid: ExprId) -> bool {
    match &exprs[eid.0 as usize] {
        Expr::Array(_) => true,
        Expr::Ident(n) => arrays.contains(n),
        _ => false,
    }
}

/// The lifted name of a function expression still carrying an
/// unbound `__this`, or `None` for everything else — an arrow, an
/// object-literal method, a closure a promote already claimed, or a
/// plain value.
fn unclaimed_fnexpr(ast: &Ast, eid: ExprId) -> Option<String> {
    let Expr::Closure { fn_name, captures } = &ast.exprs[eid.0 as usize] else {
        return None;
    };
    if !captures.iter().any(|c| c == "__this") {
        return None;
    }
    if !ast.fn_expr_exprs.contains(&eid) || ast.objlit_method_exprs.contains(&eid) {
        return None;
    }
    Some(fn_name.clone())
}

/// `true` when `eid` is syntactically certain to be a real promise:
/// a `Promise` static call, `new Promise(…)`, or a handler-method
/// chain over one of those. A bare binding does not qualify even if
/// it holds a promise at runtime — the point of the check is that no
/// USER-written `then` can be reached.
fn promise_certain(exprs: &[Expr], eid: ExprId) -> bool {
    let Expr::Call { callee, .. } = &exprs[eid.0 as usize] else {
        return false;
    };
    match &exprs[callee.0 as usize] {
        // `new Promise(executor)` never survives to this pass as an
        // `Expr::New`: the prelude's `rewrite_promise_new` turns it
        // into a call to this synthesized helper, whose return is
        // `__pr.promise` (§27.2.4.8's cell).
        Expr::Ident(n) => n == "__promise_from_executor",
        Expr::Member { obj, name } if PROMISE_STATICS.contains(&name.as_str()) => {
            matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Promise")
        }
        Expr::Member { obj, name } if HANDLER_METHODS.contains(&name.as_str()) => {
            promise_certain(exprs, *obj)
        }
        _ => false,
    }
}
