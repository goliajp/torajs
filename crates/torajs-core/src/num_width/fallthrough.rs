//! RFC 20260725-fallthrough-return — the fall-through table: which
//! functions can run off the end of their body, and therefore answer
//! `undefined` there (ES §10.2.1.4 step 11) rather than through a
//! `return`. The call site reads it to know a result may hold that
//! return width's sentinel.
//!
//! Split out of `mod.rs` when knife 4's arrow-binding alias pass pushed
//! that file past the 500-line limit.

use super::{Analysis, Scope, SlotKey, let_names};
use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashSet;

/// One top-level `FnDecl`: seed its param / return escape faces, put
/// it on the fall-through table when it can answer `undefined`, then
/// walk its body under its own scope.
pub(super) fn seed_and_walk_fn(
    a: &mut Analysis<'_>,
    stmt: &Stmt,
    tables: &SentinelTables,
    fallthrough_fns: &mut HashSet<String>,
) {
    let Stmt::FnDecl {
        name,
        params,
        body,
        return_type,
        ..
    } = stmt
    else {
        return;
    };
    // W-ESC — any-annotated param / return faces are escape sinks
    // (escape.rs).
    for p in params {
        if let Some(ann) = &p.type_ann {
            let pk = SlotKey::Param(name.clone(), p.name.clone());
            a.seed_any_face(&pk, ann);
        }
    }
    if let Some(r) = return_type {
        let rk = SlotKey::Ret(name.clone());
        a.seed_any_face(&rk, r);
        seed_fallthrough_return(a, rk, r, name, body, tables, fallthrough_fns);
    }
    let scope = Scope {
        fn_name: name,
        params: params.iter().map(|p| p.name.clone()).collect(),
        locals: {
            let mut s = HashSet::new();
            for b in body {
                let_names::collect_let_names(b, &mut s);
            }
            s
        },
    };
    for b in body {
        a.walk_stmt(b, &scope);
    }
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
#[derive(Default)]
pub(super) struct SentinelTables {
    /// Functions that can answer the sentinel — by handing one back,
    /// or by running off the end of their body.
    pub(super) fns: HashSet<String>,
    /// `(fn name, param name)` pairs some call site can hand one.
    pub(super) params: HashSet<(String, String)>,
    /// Field names some write hands one.
    pub(super) fields: HashSet<String>,
}

pub(super) fn close_sentinel_tables(a: &Analysis<'_>, stmts: &[Stmt]) -> SentinelTables {
    let mut t = SentinelTables::default();
    loop {
        let n = (t.fns.len(), t.params.len(), t.fields.len());
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
        if (t.fns.len(), t.params.len(), t.fields.len()) == n {
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
fn collect_sentinel_fields(a: &Analysis<'_>, t: &SentinelTables) -> HashSet<String> {
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
                    out.insert(name.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// RFC 20260725-fallthrough-return knife 4 — an arrow function is
/// lifted to a `__closure_N` FnDecl before this analysis runs, so it
/// lands on the table under that synthetic name. The call site spells
/// it with the binding it was assigned to (`const h = (f) => …; h(x)`),
/// which would miss. Record the binding as an alias.
///
/// A binding aliased to another binding (`const j = h`) rides the same
/// rule — it reads as a plain Ident init — so the walk runs to a
/// fixpoint rather than once, which also covers a chain declared out of
/// order relative to the walk.
///
/// Same-named bindings merge conservatively, matching how `SlotKey`
/// treats them: naming one fall-through closure is enough for every
/// `h(...)` to take the sentinel-aware branch, which is the safe
/// direction (one predictable compare) rather than the silent one.
pub(super) fn alias_fallthrough_closures(ast: &Ast, out: &mut HashSet<String>) {
    fn walk(ast: &Ast, stmts: &[Stmt], out: &mut HashSet<String>, grew: &mut bool) {
        for s in stmts {
            match s {
                Stmt::LetDecl { name, init, .. } => {
                    if out.contains(name) {
                        continue;
                    }
                    let aliases_fallthrough = match ast.get_expr(*init) {
                        Expr::Closure { fn_name, .. } => out.contains(fn_name),
                        Expr::Ident(n) => out.contains(n),
                        _ => false,
                    };
                    if aliases_fallthrough {
                        out.insert(name.clone());
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
    loop {
        let mut grew = false;
        walk(ast, &ast.stmts, out, &mut grew);
        if !grew {
            break;
        }
    }
}

/// RFC 20260725-fallthrough-return knives 1-2 — a body that can run
/// off its end answers `undefined` there (ES §10.2.1.4 step 11). Every
/// such function goes on the table, which the call site reads to know
/// a result may hold that answer's sentinel.
///
/// `number` additionally needs a WIDER slot to carry it: I64 has no
/// bit pattern to spare and F64 does, so seed the return slot and let
/// the fixpoint carry the width to every binding the result flows
/// into. Pointer-shaped returns need no seed — their slots already
/// decode three ways (NULL / sentinel / live cell).
///
/// A body can also hand the sentinel back on purpose — `return
/// xs.find(...)` passes along whatever a miss answered. The call site
/// has to route the same way either way, so both reasons put the
/// function on one table.
pub(super) fn seed_fallthrough_return(
    a: &mut Analysis<'_>,
    rk: SlotKey,
    return_ann: &str,
    fn_name: &str,
    body: &[Stmt],
    tables: &SentinelTables,
    out: &mut HashSet<String>,
) {
    if return_ann == "void" {
        return;
    }
    let falls_through = !crate::ast::body_always_terminates(body);
    if !falls_through && !body_returns_sentinel(a, fn_name, body, tables) {
        return;
    }
    out.insert(fn_name.to_string());
    if return_ann == "number" && falls_through {
        a.seeds.push(rk);
    }
}

/// True when this expression is one of the shapes that answers the
/// `undefined` sentinel rather than an ordinary value: a read past the
/// end of an array, a `find` / `findLast` / `at` miss, or a `pop` /
/// `shift` on an empty one.
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
                matches!(name.as_str(), "find" | "findLast" | "at" | "pop" | "shift")
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
        Expr::Member { name, .. } => t.fields.contains(name),
        Expr::Nullish { rhs, .. } => is_sentinel_source(a, *rhs, t),
        Expr::Sequence { right, .. } => is_sentinel_source(a, *right, t),
        Expr::Assign { value, .. } => is_sentinel_source(a, *value, t),
        Expr::As { expr, .. } => is_sentinel_source(a, *expr, t),
        _ => false,
    }
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
    let lifted = lifted_closure_names(a.ast);
    let mut out = HashSet::new();
    for eid in 0..a.ast.exprs.len() {
        let Expr::Call { callee, args } = a.ast.get_expr(crate::ast::ExprId(eid as u32)) else {
            continue;
        };
        let Expr::Ident(f) = a.ast.get_expr(*callee) else {
            continue;
        };
        // An arrow is lifted to a `__closure_N` FnDecl before this
        // analysis runs, so the parameter list lives under that
        // synthetic name while the call site still spells the binding
        // it was assigned to.
        let f = lifted.get(f).unwrap_or(f);
        // `user_params`, not the raw list: a capturing closure's
        // lifted FnDecl carries `__env` first, and the call site's
        // arguments line up with the user-facing params after it.
        let params = a.user_params(f);
        for (i, arg) in args.iter().enumerate() {
            if let Some(p) = params.get(i)
                && is_sentinel_source(a, *arg, t)
            {
                out.insert((f.clone(), p.clone()));
            }
        }
    }
    out
}

/// Binding name → the `__closure_N` FnDecl an arrow assigned to it was
/// lifted into, following plain re-bindings (`const j = h`) to a
/// fixpoint so a chain declared out of walk order still resolves.
/// The alias direction the fall-through table needs is the reverse of
/// this one, which is why the two are separate walks.
fn lifted_closure_names(ast: &Ast) -> std::collections::HashMap<String, String> {
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
fn body_returns_sentinel(
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
