//! Which names can answer the `undefined` sentinel — functions,
//! parameters, fields, container elements, bindings — and the one
//! predicate that decides it.
//!
//! Split out of `fallthrough.rs` under the 500-line discipline when
//! the field set made three name sets where there had been one. That
//! file keeps the seeding walk that reads these tables; this one owns
//! the question they answer.

use super::Analysis;
use super::fallthrough::alias_fallthrough_closures;
use crate::ast::PropKey;
use crate::ast::{Expr, Stmt};
use std::collections::HashSet;
use torajs_wtf8::Wtf8;

/// The five name sets the sentinel question needs. They feed each
/// other, so they are closed together — see [`close_sentinel_tables`].
#[derive(Default)]
pub(super) struct SentinelTables {
    /// Functions that can answer the sentinel — by handing one back,
    /// or by running off the end of their body.
    pub(super) fns: HashSet<String>,
    /// `(fn name, param name)` pairs some call site can hand one.
    pub(super) params: HashSet<(String, String)>,
    /// Field names some write hands one.
    pub(super) fields: HashSet<PropKey>,
    /// Container bindings some write fills an ELEMENT of with one.
    pub(super) elems: HashSet<String>,
    /// Binding names some declaration or assignment fills with one.
    pub(super) lets: HashSet<String>,
    /// The two shapes that have no name to be recorded under
    /// (`super::sentinel_member_reads`). Collected once, before the
    /// closure below starts, because they do not depend on how far it
    /// has got — only the `fns` test applied to them does.
    pub(super) members: super::sentinel_member_reads::MemberCandidates,
}

/// Close the fall-through table and its tainted-parameter mirror
/// together, before anything reads either.
///
/// The two feed each other and each used to be computed once, in one
/// order: a call site taints a parameter only when the argument
/// answers the sentinel, and one of the ways an argument can is by
/// being a call to a function already on the table — which in turn
/// may be on the table because it hands a tainted parameter back. So
/// the table did not compose even one hop. `function g(): number {
/// return zs[9] }` read `undefined` and `function f(): number {
/// return g() }` printed NaN, and a fall-through returned through
/// another fall-through did the same.
///
/// The admission rule is the one [`seed_and_walk_fn`] applies, so the
/// walk that follows re-derives these same names and its width
/// seeding is untouched — this only gets there first, with the whole
/// answer instead of a prefix of it. Both sets only grow and the
/// names are finite, so it terminates.
pub(super) fn close_sentinel_tables(a: &Analysis<'_>, stmts: &[Stmt]) -> SentinelTables {
    let mut t = SentinelTables {
        members: super::sentinel_member_reads::collect_member_candidates(a),
        fields: super::optional_number_fields::collect(a),
        ..Default::default()
    };
    loop {
        let n = (
            t.fns.len(),
            t.params.len(),
            t.fields.len(),
            t.elems.len(),
            t.lets.len(),
        );
        t.lets.extend(collect_sentinel_lets(a, &t, stmts));
        t.elems
            .extend(super::sentinel_hof::collect_sentinel_elems(a, &t, stmts));
        t.params.extend(collect_undef_sentinel_params(a, &t));
        t.fields.extend(collect_sentinel_fields(a, &t));
        for stmt in stmts {
            let Stmt::FnDecl {
                name,
                body,
                return_type: Some(ret),
                ..
            } = stmt
            else {
                continue;
            };
            if ret == "void" || t.fns.contains(name) {
                continue;
            }
            if !crate::ast::body_always_terminates(body) || body_returns_sentinel(a, name, body, &t)
            {
                t.fns.insert(name.clone());
            }
        }
        alias_fallthrough_closures(a.ast, &mut t.fns);
        if (
            t.fns.len(),
            t.params.len(),
            t.fields.len(),
            t.elems.len(),
            t.lets.len(),
        ) == n
        {
            return t;
        }
    }
}

/// Field names some object literal or member assignment fills with a
/// value that answers the sentinel. The mirror of
/// [`crate::undef_f64_fields`], which asks the same question of the
/// same shapes one stage later, with the lowering context that does
/// not exist yet here.
///
/// By name and across bodies, like everything else on these tables:
/// two structs sharing a field name cost one predictable compare at
/// the consumer, while missing one prints NaN where the program
/// should see `undefined`.
fn collect_sentinel_fields(a: &Analysis<'_>, t: &SentinelTables) -> HashSet<PropKey> {
    let mut out = HashSet::new();
    for eid in 0..a.ast.exprs.len() {
        let eid = crate::ast::ExprId(eid as u32);
        match a.ast.get_expr(eid) {
            Expr::ObjectLit { fields } => {
                for (name, value) in fields {
                    if is_sentinel_source(a, *value, t) {
                        out.insert(name.clone());
                    }
                }
            }
            Expr::Assign { target, value } => {
                if let Expr::Member { name, .. } = a.ast.get_expr(*target)
                    && is_sentinel_source(a, *value, t)
                {
                    out.insert(PropKey::from(name));
                }
            }
            _ => {}
        }
    }
    out
}

/// True when this expression is one of the shapes that answers the
/// `undefined` sentinel rather than an ordinary value: a read past the
/// end of an array, a `find` / `findLast` / `at` miss, a `pop` /
/// `shift` on an empty one, or a `Map<K, number>` `get` miss.
///
/// Receiver-type-agnostic on purpose — the shape alone is the gate, and
/// being on a table only costs one predictable compare at the consumer.
///
/// `t` is the tables as far as they are known. A call to a function
/// on one answers the sentinel one hop further out, and a field read
/// answers whatever some write put in a field of that name; see
/// [`close_sentinel_tables`] for why they have to be closed before
/// anything reads them.
pub(super) fn is_sentinel_source(
    a: &Analysis<'_>,
    eid: crate::ast::ExprId,
    t: &SentinelTables,
) -> bool {
    match a.ast.get_expr(eid) {
        // Reading past the end answers `undefined` (ES §10.4.2.1)
        // exactly as a miss does, and `xs.at(i)` is on the list right
        // below only because it takes that same exit under another
        // spelling. Handing the read straight on left the consumer
        // reading the sentinel as a plain value and printing NaN,
        // while the identical read read in place has always answered
        // `undefined`.
        Expr::Index { .. } | Expr::OptIndex { .. } => true,
        // Either spelling of "someone else already answered it": a
        // builtin miss, or a user function that is itself on the
        // table — because its body hands one back, or because it can
        // run off its end. Without the second the table did not
        // compose one hop: `function g(): number { return zs[9] }`
        // read right and `function f(): number { return g() }`
        // printed NaN.
        Expr::Call { callee, .. } => match a.ast.get_expr(*callee) {
            Expr::Member { name, .. } => {
                matches!(
                    name.as_str(),
                    "find" | "findLast" | "at" | "pop" | "shift" | "get"
                )
            }
            Expr::Ident(f) => t.fns.contains(f),
            _ => false,
        },
        // The value-transparent wrappers, the set the 11-A1 escape
        // visitor names: what comes out is what went in. Without them
        // `return true ? xs[9] : 0` handed the sentinel back as an
        // ordinary value and the caller printed NaN, while `return
        // xs[9]` one line above answered `undefined`. The `??` yields
        // its left arm only when that arm is neither null nor
        // undefined, so a sentinel there can never be the result.
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => is_sentinel_source(a, *then_branch, t) || is_sentinel_source(a, *else_branch, t),
        // A field read answers whatever some write put in a field of
        // that name. `function f(): number { const r = { v: zs[9] };
        // return r.v }` printed NaN while the identical read outside a
        // return has answered `undefined` since the lowering-stage
        // twin of this set existed.
        //
        // The two shapes with no field write behind them ride the
        // same arm: a read that runs a getter whose body falls
        // through, and an awaited `Promise<number>`. Neither is a
        // write to a field of that name, so the set above never saw
        // either, and `const u = src.g; g(u)` left `u` untainted
        // while `typeof src.g` read in place has answered `undefined`
        // since the lowering-side accessor arm landed.
        Expr::Member { name, .. } => {
            t.fields.contains(Wtf8::new(name))
                || super::sentinel_member_reads::hands_back_sentinel(t, eid)
        }
        // A binding reads back what its declaration or some
        // assignment put in it, and a PARAMETER reads back what some
        // call site handed it.
        //
        // The binding half has been tracked WITHIN one body since
        // `body_returns_sentinel` existed — it has to, to decide
        // whether `return a` after `a = zs[9]` hands one back — but
        // nothing tracked it across bodies, so a value read through
        // a binding and read inline gave two answers at a kernel
        // boundary.
        //
        // The parameter half was missing entirely, and a sentinel
        // therefore stopped at the first typed parameter it reached:
        // `g(zs[9])` armed `g`'s `p`, but `h(p)` inside `g` asked
        // this predicate about `p` and got false, so `h`'s `q` was
        // never armed. The same gap left `ys.forEach(x =>
        // ws.push(x))` open — `x` is on the parameter table, and the
        // push that fills `ws` reads it through here.
        //
        // The match is by NAME, not by the `(fn, param)` pair the
        // table is keyed on, because this predicate runs over the
        // expression arena and the arena carries no owner: there is
        // no ExprId → enclosing-function map anywhere in the crate,
        // so the pair's first half is not recoverable here.
        // `body_returns_sentinel` is the one caller that does hold a
        // function name, and it stays exact. By name is the same
        // trade the other four tables already make, and in the same
        // direction — a parameter named `x` in an unrelated function
        // costs one predictable compare at a consumer, while missing
        // one prints NaN where the program should see `undefined`.
        // Arming reads the slot type, it never sets it
        // (`ssa_lower_fn`'s seeding), so nothing widens.
        Expr::Ident(n) => t.lets.contains(n) || t.params.iter().any(|(_, param)| param == n),
        Expr::Nullish { rhs, .. } => is_sentinel_source(a, *rhs, t),
        Expr::Sequence { right, .. } => is_sentinel_source(a, *right, t),
        Expr::Assign { value, .. } => is_sentinel_source(a, *value, t),
        Expr::As { expr, .. } => is_sentinel_source(a, *expr, t),
        _ => false,
    }
}

/// Binding names whose declaration or some assignment fills them with
/// a value that answers the sentinel.
///
/// By name and across bodies, like the other four: two functions with
/// a local `u` cost one predictable compare at the consumer, while
/// missing one prints NaN where the program should see `undefined`.
/// Walking statements rather than the expression arena is what a
/// declaration needs — a `LetDecl` is a statement, and its name is not
/// reachable from any expression id.
fn collect_sentinel_lets(a: &Analysis<'_>, t: &SentinelTables, stmts: &[Stmt]) -> HashSet<String> {
    fn walk(a: &Analysis<'_>, t: &SentinelTables, stmts: &[&Stmt], out: &mut HashSet<String>) {
        for s in stmts {
            match s {
                Stmt::LetDecl { name, init, .. } if is_sentinel_source(a, *init, t) => {
                    out.insert(name.clone());
                }
                Stmt::Expr(e) => {
                    if let Expr::Assign { target, value } = a.ast.get_expr(*e)
                        && let Expr::Ident(n) = a.ast.get_expr(*target)
                        && is_sentinel_source(a, *value, t)
                    {
                        out.insert(n.clone());
                    }
                }
                // `stmt_children_ref` stops at a nested function's
                // body on purpose — a function is its own scope, and
                // the passes that share that walker enter one
                // deliberately. This table does not work that way:
                // the doc above says "by name and across bodies",
                // and every other collector here reads the whole
                // arena. Only these two statement walkers were
                // confined to the top level, so `function f() {
                // const u = zs[9]; g(u) }` never registered `u` and
                // `g`'s typed parameter read the sentinel as a plain
                // number — `typeof p` answered "number" where the
                // same declaration one scope up answers "undefined".
                // The lowering-side mirror of this table
                // (`undef_f64_fields::prime`) has always walked fn
                // bodies; this is the analysis side catching up to
                // its own doc and to that mirror at once.
                Stmt::FnDecl { body, .. } => {
                    walk(a, t, &body.iter().collect::<Vec<_>>(), out);
                }
                _ => {}
            }
            walk(
                a,
                t,
                &crate::ast::desugar_with::walk::stmt_children_ref(s),
                out,
            );
        }
    }
    let mut out = HashSet::new();
    walk(a, t, &stmts.iter().collect::<Vec<_>>(), &mut out);
    out
}

/// The mirror of the fall-through table: which **parameters** can be
/// handed the `undefined` sentinel, rather than which returns can hand
/// one back.
///
/// A binding that receives one is recorded where it is declared
/// (`undefable_f64_lets`, at the let-decl site), so the consumers in
/// that body know to check. A parameter has no such site — its value
/// arrives from the *caller's* body, which is lowered separately — so
/// nothing ever recorded it and `h(xs[7])` printed NaN inside `h`
/// while the identical `console.log(xs[7])` at the call site printed
/// `undefined`.
///
/// Scanning the expression arena rather than walking statements
/// catches call sites at any nesting depth. Positional: argument `i`
/// names parameter `i` of the callee. Recording one is enough for
/// every call of that function to take the sentinel-aware branch,
/// which is the safe direction — the same conservative merge the
/// fall-through table makes for same-named bindings.
fn collect_undef_sentinel_params(
    a: &Analysis<'_>,
    t: &SentinelTables,
) -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    // A class method read as a value may run bare through its boxed
    // adapter, where a missing argument is `undefined` in whatever
    // slot the parameter has — no call site in the program spells
    // that, so the read itself names the parameters.
    for f in super::method_value::detached_methods(a) {
        for p in a.user_params(&f) {
            if p != "__this" {
                out.insert((f.clone(), p));
            }
        }
    }
    for eid in 0..a.ast.exprs.len() {
        let Expr::Call { callee, args } = a.ast.get_expr(crate::ast::ExprId(eid as u32)) else {
            continue;
        };
        let Expr::Ident(_) = a.ast.get_expr(*callee) else {
            super::sentinel_hof::collect_reduce_seed(a, t, *callee, args, &mut out);
            super::sentinel_hof::collect_hof_elem_params(a, t, *callee, args, &mut out);
            super::sentinel_hof::collect_promise_then_param(a, t, *callee, args, &mut out);
            continue;
        };
        // An arrow is lifted to a `__closure_N` FnDecl before this
        // analysis runs, so the call site's binding names the closures
        // whose (user-facing, past `__env`) params take the arguments.
        let fns = super::bound_closures::callback_fn_name(a, *callee);
        for (i, arg) in args.iter().enumerate() {
            if is_sentinel_source(a, *arg, t) {
                super::sentinel_hof::taint_param(a, &fns, i, &mut out);
            }
        }
        // A spread stands for an unknown number of arguments, so every
        // parameter from its position on may be one the list does not
        // reach, which the spread lanes bind to undefined (§10.2.11).
        if let Some(k) = args
            .iter()
            .position(|x| matches!(a.ast.get_expr(*x), Expr::Spread { .. }))
        {
            let width = fns.iter().map(|f| a.user_params(f).len()).max();
            for i in k..width.unwrap_or(0) {
                super::sentinel_hof::taint_param(a, &fns, i, &mut out);
            }
        }
    }
    out
}

/// True when some `return` in `body` hands back a value that may be
/// the `undefined` answer: an index read past the end, a `find` /
/// `findLast` / `at` miss, or a `pop` / `shift` on an empty array.
/// Those already put their width's sentinel in the slot; without this
/// the caller reads it as a plain value and prints NaN (or answers
/// `typeof` "string" for the immortal cell).
///
/// A value parked in a local first (`const m = xs.find(...); return m`)
/// counts too — the binding is recorded on the way past, mirroring how
/// the in-function consumers track the same shape through
/// `undefable_f64_lets` / `nullable_str_lets`.
pub(super) fn body_returns_sentinel(
    a: &Analysis<'_>,
    fn_name: &str,
    body: &[Stmt],
    t: &SentinelTables,
) -> bool {
    // Handing back a parameter a call site tainted passes the sentinel
    // straight through, the same way handing back the read itself does.
    let mut lets: HashSet<String> = a
        .fn_params
        .get(fn_name)
        .into_iter()
        .flatten()
        .filter(|p| t.params.contains(&(fn_name.to_string(), (*p).clone())))
        .cloned()
        .collect();
    fn walk(
        a: &Analysis<'_>,
        stmts: &[Stmt],
        lets: &mut HashSet<String>,
        t: &SentinelTables,
    ) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::LetDecl { name, init, .. } => {
                if is_sentinel_source(a, *init, t) {
                    lets.insert(name.clone());
                }
                false
            }
            // An assignment taints its binding exactly as a
            // declaration does; only the declaration was read, so
            // `let a: number = 0; a = zs[9]; return a` printed NaN.
            Stmt::Expr(e) => {
                if let Expr::Assign { target, value } = a.ast.get_expr(*e)
                    && let Expr::Ident(n) = a.ast.get_expr(*target)
                    && is_sentinel_source(a, *value, t)
                {
                    lets.insert(n.clone());
                }
                false
            }
            Stmt::Return(Some(eid)) => {
                is_sentinel_source(a, *eid, t)
                    || matches!(a.ast.get_expr(*eid), Expr::Ident(n) if lets.contains(n))
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => walk(a, inner, lets, t),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk(a, std::slice::from_ref(then_branch), lets, t)
                    || else_branch
                        .as_deref()
                        .is_some_and(|e| walk(a, std::slice::from_ref(e), lets, t))
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Labeled { body, .. } => walk(a, std::slice::from_ref(body), lets, t),
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                walk(a, body, lets, t)
                    || walk(a, catch_body, lets, t)
                    || finally_body.as_deref().is_some_and(|f| walk(a, f, lets, t))
            }
            _ => false,
        })
    }
    walk(a, body, &mut lets, t)
}
