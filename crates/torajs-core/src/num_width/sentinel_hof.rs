//! The shapes where a sentinel and the parameter it reaches are not
//! the same argument of the same call.
//!
//! [`super::sentinel_tables::collect_undef_sentinel_params`] answers
//! the positional case: argument `i` of a direct call names parameter
//! `i` of the callee. A higher-order call does not work that way — the
//! value comes off the receiver or off another argument, and the
//! parameter it lands in belongs to a callback. One arm per shape,
//! because there is no rule covering them all.
//!
//! Split out of the tables module when the reduce arm brought it to
//! the 500-line edge; that module owns the name sets and the predicate
//! over them, this one owns the call shapes that feed them.

use super::Analysis;
use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashSet;

use super::sentinel_tables::{SentinelTables, is_sentinel_source};

/// `xs.reduce(cb, seed)` — the seed lands in `cb`'s FIRST parameter,
/// the accumulator, and §23.1.3.24 hands it over untouched. So a
/// sentinel-bearing seed makes the accumulator sentinel-capable,
/// exactly as a sentinel-bearing argument makes a parameter so above.
///
/// The positional rule up there cannot express this: the sentinel
/// source and the parameter it reaches are in different arguments of
/// different calls. Without it `[1].reduce((acc, x) => …, zs[9])`
/// reported `typeof acc` "number" for an `undefined`.
///
/// Only the seed. The element parameter is the receiver's business,
/// which this pass does not track — see the HOF families still open.
pub(super) fn collect_reduce_seed(
    a: &Analysis<'_>,
    t: &SentinelTables,
    callee: crate::ast::ExprId,
    args: &[crate::ast::ExprId],
    lifted: &std::collections::HashMap<String, String>,
    out: &mut HashSet<(String, String)>,
) {
    let Expr::Member { name, .. } = a.ast.get_expr(callee) else {
        return;
    };
    if !matches!(name.as_str(), "reduce" | "reduceRight") {
        return;
    }
    let (Some(cb), Some(seed)) = (args.first(), args.get(1)) else {
        return;
    };
    if !is_sentinel_source(a, *seed, t) {
        return;
    }
    let Some(fn_name) = callback_fn_name(a, *cb, lifted) else {
        return;
    };
    taint_param(a, &fn_name, 0, out);
}

/// The FnDecl an argument in callback position names. An arrow is
/// lifted to a `__closure_N` before this analysis runs, so a spelled
/// binding resolves through the same alias map the positional
/// collector uses.
fn callback_fn_name(
    a: &Analysis<'_>,
    cb: crate::ast::ExprId,
    lifted: &std::collections::HashMap<String, String>,
) -> Option<String> {
    match a.ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
        Expr::Ident(n) => Some(lifted.get(n).cloned().unwrap_or_else(|| n.clone())),
        _ => None,
    }
}

/// Record that parameter `idx` of `fn_name` can be handed a sentinel.
/// `user_params` skips the `__env` a capturing closure carries, so the
/// index is the one the source spells.
fn taint_param(a: &Analysis<'_>, fn_name: &str, idx: usize, out: &mut HashSet<(String, String)>) {
    if let Some(p) = a.user_params(fn_name).get(idx) {
        out.insert((fn_name.to_string(), p.clone()));
    }
}

/// Array-method names whose callback takes the ELEMENT as its first
/// user parameter (§23.1.3). `sort` / `toSorted` take two elements and
/// are handled beside this list; `reduce` takes the accumulator first
/// and has its own arm above.
const ELEM_FIRST_HOFS: &[&str] = &[
    "forEach",
    "map",
    "filter",
    "find",
    "findLast",
    "findIndex",
    "findLastIndex",
    "some",
    "every",
    "flatMap",
];

/// Writes that can put a sentinel into a container's elements, by the
/// index of the arguments that become elements. `splice` drops two
/// leading positions (start, deleteCount) before its items.
fn elem_writing_args(method: &str, args: &[crate::ast::ExprId]) -> &'static [usize] {
    match method {
        "push" | "unshift" | "fill" => &[0, 1, 2, 3],
        "splice" | "toSpliced" => &[2, 3, 4, 5],
        // A Map entry's VALUE and a Set's member are elements for this
        // purpose: `forEach` hands each to a callback the same way.
        "set" => &[1],
        "add" => &[0],
        _ => {
            let _ = args;
            &[]
        }
    }
}

/// Container bindings some write fills with a value that answers the
/// sentinel — `ys.push(zs[9])`, `ys[0] = zs[9]`, `const ys = [zs[9]]`.
///
/// The element itself reads back right: `ys[0]` is an `Index` and the
/// predicate calls that a source unconditionally. What is lost is the
/// hop through a KERNEL — `ys.forEach(cb)` loads the element and hands
/// it to `cb` at the element's own width, and nothing told `cb` the
/// value can be a sentinel, so `typeof x` inside answered "number".
///
/// By binding name, like the other three sets: two containers sharing
/// a name cost one predictable compare inside a callback, and missing
/// one prints NaN where the program should see `undefined`.
pub(super) fn collect_sentinel_elems(
    a: &Analysis<'_>,
    t: &SentinelTables,
    stmts: &[Stmt],
) -> HashSet<String> {
    let mut out = HashSet::new();
    for eid in 0..a.ast.exprs.len() {
        let eid = crate::ast::ExprId(eid as u32);
        match a.ast.get_expr(eid) {
            Expr::Call { callee, args } => {
                let Expr::Member { obj, name } = a.ast.get_expr(*callee) else {
                    continue;
                };
                let Expr::Ident(recv) = a.ast.get_expr(*obj) else {
                    continue;
                };
                // `Reflect.deleteProperty(xs, k)` leaves the same hole
                // `delete xs[k]` does (the Delete arm below)
                if recv == "Reflect"
                    && name == "deleteProperty"
                    && let Some(Expr::Ident(target)) = args.first().map(|t| a.ast.get_expr(*t))
                {
                    out.insert(target.clone());
                }
                if elem_writing_args(name.as_str(), args)
                    .iter()
                    .filter_map(|i| args.get(*i))
                    .any(|w| is_sentinel_source(a, *w, t))
                {
                    out.insert(recv.clone());
                }
            }
            Expr::Assign { target, value } => {
                // An index write past the end, or a longer `length`,
                // leaves holes — and a hole of a number[] holds the
                // sentinel. Whether the index is past the end is not
                // known here, so every index or length write counts
                // (the predicate may be generous, never clever).
                if let Expr::Index { obj, .. } = a.ast.get_expr(*target)
                    && let Expr::Ident(recv) = a.ast.get_expr(*obj)
                {
                    out.insert(recv.clone());
                }
                if let Expr::Member { obj, name } = a.ast.get_expr(*target)
                    && name == "length"
                    && let Expr::Ident(recv) = a.ast.get_expr(*obj)
                {
                    out.insert(recv.clone());
                }
                if let Expr::Ident(recv) = a.ast.get_expr(*target)
                    && literal_holds_sentinel(a, t, *value)
                {
                    out.insert(recv.clone());
                }
            }
            // An element delete leaves a hole the same way (a typed
            // cell keeps it in its own slot since 99cef557d): for-of
            // and the callback kernels read it as the sentinel.
            Expr::Delete { expr } => {
                if let Expr::Index { obj, .. } = a.ast.get_expr(*expr)
                    && let Expr::Ident(recv) = a.ast.get_expr(*obj)
                {
                    out.insert(recv.clone());
                }
            }
            _ => {}
        }
    }
    let stmts: Vec<&Stmt> = stmts.iter().collect();
    loop {
        let before = out.len();
        collect_literal_decls(a, t, &stmts, &mut out);
        collect_elem_args(a, &mut out);
        if out.len() == before {
            return out;
        }
    }
}

/// A container whose elements can hold a sentinel, handed to a user
/// function, makes that parameter such a container too — the callee's
/// `xs.forEach(cb)` loads the same slots. Registered by parameter
/// name like everything else here; the caller loops to a fixed point
/// so a chain of calls and copies registers.
fn collect_elem_args(a: &Analysis<'_>, out: &mut HashSet<String>) {
    for eid in 0..a.ast.exprs.len() {
        let Expr::Call { callee, args } = a.ast.get_expr(crate::ast::ExprId(eid as u32)) else {
            continue;
        };
        let Expr::Ident(f) = a.ast.get_expr(*callee) else {
            continue;
        };
        let Some(params) = a.fn_params.get(f) else {
            continue;
        };
        for (arg, param) in args.iter().zip(params) {
            let holds = match a.ast.get_expr(*arg) {
                Expr::Ident(n) => out.contains(n),
                _ => copies_sentinel_elems(a, *arg, out),
            };
            if holds && !out.contains(param) {
                out.insert(param.clone());
            }
        }
    }
}

/// A copy of a container whose elements can hold a sentinel, read
/// through [[Get]] so a hole arrives as the sentinel (`toReversed`,
/// `toSorted`, `toSpliced`, `with`, `Array.from(xs)`, `[...xs]`), or
/// kept as a hole (`slice`, `concat`) that the find family then reads
/// the same way. Programs declare the copy after the source, and
/// statements are walked in order, so a chain of copies registers.
fn copies_sentinel_elems(
    a: &Analysis<'_>,
    eid: crate::ast::ExprId,
    elems: &HashSet<String>,
) -> bool {
    let named =
        |e: crate::ast::ExprId| matches!(a.ast.get_expr(e), Expr::Ident(n) if elems.contains(n));
    match a.ast.get_expr(eid) {
        Expr::Call { callee, args } => match a.ast.get_expr(*callee) {
            Expr::Member { obj, name }
                if matches!(
                    name.as_str(),
                    "toReversed" | "toSorted" | "toSpliced" | "with" | "slice" | "concat"
                ) =>
            {
                named(*obj)
            }
            Expr::Member { obj, name } if name == "from" => {
                matches!(a.ast.get_expr(*obj), Expr::Ident(n) if n == "Array")
                    && args.first().is_some_and(|x| named(*x))
            }
            _ => false,
        },
        Expr::Array(els) => els
            .iter()
            .any(|e| matches!(a.ast.get_expr(*e), Expr::Spread { expr } if named(*expr))),
        _ => false,
    }
}

/// An array literal with a sentinel-bearing element. A declaration is
/// a statement, so its name is not reachable from any expression id —
/// which is why this half walks statements.
fn literal_holds_sentinel(a: &Analysis<'_>, t: &SentinelTables, eid: crate::ast::ExprId) -> bool {
    let Expr::Array(els) = a.ast.get_expr(eid) else {
        return false;
    };
    els.iter().any(|e| is_sentinel_source(a, *e, t))
}

fn collect_literal_decls(
    a: &Analysis<'_>,
    t: &SentinelTables,
    stmts: &[&Stmt],
    out: &mut HashSet<String>,
) {
    for s in stmts {
        if let Stmt::LetDecl { name, init, .. } = s
            && (literal_holds_sentinel(a, t, *init) || copies_sentinel_elems(a, *init, out))
        {
            out.insert(name.clone());
        }
        // The element table's half of the same gap the `lets`
        // collector had: a container literal declared inside a
        // function body never registered, so `function f() { const
        // ys = [zs[9]]; for (const x of ys) … }` handed the loop
        // variable a raw sentinel.
        if let Stmt::FnDecl { body, .. } = s {
            collect_literal_decls(a, t, &body.iter().collect::<Vec<_>>(), out);
        }
        collect_literal_decls(
            a,
            t,
            &crate::ast::desugar_with::walk::stmt_children_ref(s),
            out,
        );
    }
}

/// `X.<hof>(cb)` where `X`'s elements can hold a sentinel — the kernel
/// loads an element and hands it over at the element's own width, so
/// the callback's element parameter is sentinel-capable.
pub(super) fn collect_hof_elem_params(
    a: &Analysis<'_>,
    t: &SentinelTables,
    callee: crate::ast::ExprId,
    args: &[crate::ast::ExprId],
    lifted: &std::collections::HashMap<String, String>,
    out: &mut HashSet<(String, String)>,
) {
    let Expr::Member { obj, name } = a.ast.get_expr(callee) else {
        return;
    };
    let Some(cb) = args.first() else { return };
    let Some(fn_name) = callback_fn_name(a, *cb, lifted) else {
        return;
    };
    // The find family reads every index below length with [[Get]]
    // (§23.1.3.9 — no HasProperty step), so a hole reaches the callback
    // as the sentinel it holds. Where the holes came from is not
    // tracked (`mk().findIndex(cb)`, a container filled in another
    // function), and the receiver's static type may already be `any`
    // here, so every receiver counts; a slot that cannot hold a
    // sentinel pays nothing for it.
    if matches!(
        name.as_str(),
        "find" | "findLast" | "findIndex" | "findLastIndex"
    ) {
        taint_param(a, &fn_name, 0, out);
        return;
    }
    let holds = match a.ast.get_expr(*obj) {
        Expr::Ident(recv) => t.elems.contains(recv),
        _ => copies_sentinel_elems(a, *obj, &t.elems),
    };
    if !holds {
        return;
    }
    match name.as_str() {
        m if ELEM_FIRST_HOFS.contains(&m) => {
            taint_param(a, &fn_name, 0, out);
            // §24.2.3.6 hands a Set member as BOTH the value and the
            // key. An Array index and a Map key are not elements, so
            // this is the one receiver shape it applies to.
            if m == "forEach" && matches!(a.expr_types.get(obj), Some(crate::check::Type::Set)) {
                taint_param(a, &fn_name, 1, out);
            }
        }
        // The accumulator is parameter 0; the element is parameter 1.
        "reduce" | "reduceRight" => taint_param(a, &fn_name, 1, out),
        // `sort` / `toSorted` are deliberately absent: §23.1.3.30
        // step 3 sorts undefined elements to the end WITHOUT calling
        // the comparator, so its parameters can never see one.
        _ => {}
    }
}

/// `Promise.resolve(X).then(cb)` where `X` answers the sentinel — the
/// promise settles with that value and §27.2.5.4 hands it to the
/// fulfilled handler, so the handler's first parameter can hold one.
///
/// Not `.catch` and not `.then`'s second argument: those receive the
/// rejection reason, which has nothing to do with what resolved.
pub(super) fn collect_promise_then_param(
    a: &Analysis<'_>,
    t: &SentinelTables,
    callee: crate::ast::ExprId,
    args: &[crate::ast::ExprId],
    lifted: &std::collections::HashMap<String, String>,
    out: &mut HashSet<(String, String)>,
) {
    let Expr::Member { obj, name } = a.ast.get_expr(callee) else {
        return;
    };
    if name != "then" {
        return;
    }
    let Expr::Call {
        callee: inner,
        args: settled,
    } = a.ast.get_expr(*obj)
    else {
        return;
    };
    let Expr::Member {
        obj: ctor,
        name: method,
    } = a.ast.get_expr(*inner)
    else {
        return;
    };
    if method != "resolve" || !matches!(a.ast.get_expr(*ctor), Expr::Ident(n) if n == "Promise") {
        return;
    }
    if !settled
        .first()
        .is_some_and(|x| is_sentinel_source(a, *x, t))
    {
        return;
    }
    let Some(cb) = args.first() else { return };
    let Some(fn_name) = callback_fn_name(a, *cb, lifted) else {
        return;
    };
    taint_param(a, &fn_name, 0, out);
}

/// Binding name → the `__closure_N` FnDecl an arrow assigned to it was
/// lifted into, following plain re-bindings (`const j = h`) to a
/// fixpoint so a chain declared out of walk order still resolves.
/// The alias direction the fall-through table needs is the reverse of
/// this one, which is why the two are separate walks.
pub(super) fn lifted_closure_names(ast: &Ast) -> std::collections::HashMap<String, String> {
    fn walk(
        ast: &Ast,
        stmts: &[Stmt],
        out: &mut std::collections::HashMap<String, String>,
        grew: &mut bool,
    ) {
        for s in stmts {
            match s {
                Stmt::LetDecl { name, init, .. } => {
                    if out.contains_key(name) {
                        continue;
                    }
                    let target = match ast.get_expr(*init) {
                        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
                        Expr::Ident(n) => out.get(n).cloned(),
                        _ => None,
                    };
                    if let Some(t) = target {
                        out.insert(name.clone(), t);
                        *grew = true;
                    }
                }
                Stmt::FnDecl { body, .. } | Stmt::Block(body) | Stmt::Multi(body) => {
                    walk(ast, body, out, grew)
                }
                _ => {}
            }
        }
    }
    let mut out = std::collections::HashMap::new();
    loop {
        let mut grew = false;
        walk(ast, &ast.stmts, &mut out, &mut grew);
        if !grew {
            break;
        }
    }
    out
}
