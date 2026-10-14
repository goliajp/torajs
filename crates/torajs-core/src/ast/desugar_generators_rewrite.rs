//! Generator parameter rewrite helpers.
//!
//! Chunk 336 — extracted from `ast::desugar_generators` to shrink
//! the parent fn below the 200-LOC HARD limit. Walks every Expr
//! reachable from a generator body and rewrites every `Ident(name)`
//! matching a generator parameter (or lifted-local) name into
//! `this.<name>` in-place, so the binding survives yield
//! boundaries via the iterator-class field path.

use super::{Ast, Expr, ExprId, Param, Stmt};

mod class_scope;

/// The generator object, aliased as a METHOD LOCAL at the top of the
/// state machine's `next` body (`desugar_generators_assemble`). A
/// function expression's `this` is its own (§10.2.1.2), so a
/// generator local read from inside one cannot spell `this.<field>`;
/// it spells `__gself.<field>` and the closure lift captures the
/// alias like any other enclosing local.
pub(crate) const GEN_SELF_ALIAS: &str = "__gself";

/// What a rewritten reference reaches the generator object through.
#[derive(Clone, PartialEq, Eq)]
enum RefBase {
    /// `this.<field>` — the generator's own body and the arrows in it
    /// (an arrow's `this` is lexical, §8.3.4).
    This,
    /// `__gself.<field>` — inside a function expression or an
    /// object-literal method, whose `this` is not the generator's.
    SelfAlias,
    /// `<handle>.<field>` — the body of a NESTED generator reading an
    /// enclosing generator's local through the frame handle its
    /// factory was given (`desugar_generators_frame_link`). An
    /// ordinary local, so a function expression keeps it.
    Frame(String),
}

/// The name → field map the walk carries, plus how it spells the base.
#[derive(Clone)]
struct ParamMap {
    fields: std::collections::HashMap<String, String>,
    base: RefBase,
}

/// Walk every Expr reachable from `body` and rewrite param-name
/// idents in-place into `this.<name>` field accesses. The same
/// ExprId keeps its semantic meaning; only its kind changes from
/// `Ident` to `Member { obj: this, name }`.
pub(super) fn rewrite_params_to_this(ast: &mut Ast, body: &[Stmt], params: &[Param]) {
    // Identity map: each name rewrites to a this-field of the SAME
    // name. RFC 20260802 generalized the walker to a name→field map
    // so the catch-param slot rewrite below can share it.
    let map = ParamMap {
        fields: params
            .iter()
            .map(|p| (p.name.clone(), p.name.clone()))
            .collect(),
        base: RefBase::This,
    };
    let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for s in body {
        rewrite_params_in_stmt(ast, s, &map, &mut visited);
    }
}

/// RFC 20260802 — rewrite every `Ident(from)` reachable from `body`
/// into `this.<slot>`. Used by the generator try-region arm to point
/// catch-param references at the hoisted exception field.
pub(super) fn rewrite_ident_to_this_slot(ast: &mut Ast, body: &[Stmt], from: &str, slot: &str) {
    let mut fields = std::collections::HashMap::new();
    fields.insert(from.to_string(), slot.to_string());
    let map = ParamMap {
        fields,
        base: RefBase::This,
    };
    let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for s in body {
        rewrite_params_in_stmt(ast, s, &map, &mut visited);
    }
}

/// Frame link (`desugar_generators_frame_link`) — rewrite every
/// `Ident(from)` reachable from `body` into `<handle>.<from>`: a nested
/// generator's body reading an enclosing generator's local through the
/// frame handle its factory was given.
pub(super) fn rewrite_ident_to_frame_field(ast: &mut Ast, body: &[Stmt], from: &str, handle: &str) {
    let mut fields = std::collections::HashMap::new();
    fields.insert(from.to_string(), from.to_string());
    let map = ParamMap {
        fields,
        base: RefBase::Frame(handle.to_string()),
    };
    let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for s in body {
        rewrite_params_in_stmt(ast, s, &map, &mut visited);
    }
}

fn rewrite_params_in_stmt(
    ast: &mut Ast,
    s: &Stmt,
    pset: &ParamMap,
    visited: &mut std::collections::HashSet<u32>,
) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            rewrite_params_in_expr(ast, *eid, pset, visited);
        }
        Stmt::YieldInto { value, .. } => {
            rewrite_params_in_expr(ast, *value, pset, visited);
        }
        Stmt::Return(Some(eid)) => {
            rewrite_params_in_expr(ast, *eid, pset, visited);
        }
        Stmt::LetDecl { init, .. } => rewrite_params_in_expr(ast, *init, pset, visited),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_params_in_expr(ast, *cond, pset, visited);
            rewrite_params_in_stmt(ast, then_branch, pset, visited);
            if let Some(e) = else_branch {
                rewrite_params_in_stmt(ast, e, pset, visited);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        // F1 (RFC 20260728-gen-forof-yieldstar) — a yield-free for-of
        // kept inline by the state machine still reads lifted outer
        // locals through its element expr and body. (The `src_ident`
        // NAME contract with a lifted source is a separate registered
        // hole — the checker resolves it by name, not ExprId.)
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            rewrite_params_in_expr(ast, *elem_expr, pset, visited);
            if let Some(o) = forin_obj {
                rewrite_params_in_expr(ast, *o, pset, visited);
            }
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rewrite_params_in_expr(ast, *parent, pset, visited);
            rewrite_params_in_expr(ast, *sep, pset, visited);
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        Stmt::While { cond, body } => {
            rewrite_params_in_expr(ast, *cond, pset, visited);
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        Stmt::DoWhile { body, cond } => {
            rewrite_params_in_stmt(ast, body, pset, visited);
            rewrite_params_in_expr(ast, *cond, pset, visited);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                rewrite_params_in_stmt(ast, i, pset, visited);
            }
            if let Some(c) = cond {
                rewrite_params_in_expr(ast, *c, pset, visited);
            }
            if let Some(st) = step {
                rewrite_params_in_expr(ast, *st, pset, visited);
            }
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_params_in_stmt(ast, s, pset, visited);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            rewrite_params_in_expr(ast, *scrutinee, pset, visited);
            for c in cases {
                rewrite_params_in_expr(ast, c.value, pset, visited);
                for s in &c.body {
                    rewrite_params_in_stmt(ast, s, pset, visited);
                }
            }
            if let Some(d) = default {
                for s in d {
                    rewrite_params_in_stmt(ast, s, pset, visited);
                }
            }
        }
        // RFC 20260802 — descend into EVERY try: a yield-bearing one
        // gets region-lowered, and even a yield-free try kept inline
        // still reads generator params / lifted locals through its
        // body (the same reason inline loops descend — a `this.i`
        // read doesn't care whether a try wraps it). Its own `let`s
        // stay plain locals (lift_lets keeps that gate). The catch
        // param shadows any same-named generator param / lifted
        // local inside the catch body, so it drops out of the
        // rewrite map there.
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                rewrite_params_in_stmt(ast, s, pset, visited);
            }
            let reduced;
            let catch_set = match catch_param {
                Some(p) if pset.fields.contains_key(p) => {
                    let mut c = pset.clone();
                    c.fields.remove(p);
                    reduced = c;
                    &reduced
                }
                _ => pset,
            };
            for s in catch_body {
                rewrite_params_in_stmt(ast, s, catch_set, visited);
            }
            if let Some(fs) = finally_body {
                for s in fs {
                    rewrite_params_in_stmt(ast, s, pset, visited);
                }
            }
        }
        // A class declared inside the generator body (`class_scope`).
        Stmt::ClassDecl { .. } => class_scope::rewrite_class(ast, s, pset, visited),
        _ => {}
    }
}

fn rewrite_params_in_expr(
    ast: &mut Ast,
    eid: ExprId,
    pset: &ParamMap,
    visited: &mut std::collections::HashSet<u32>,
) {
    if !visited.insert(eid.0) {
        return;
    }
    let kind = ast.exprs[eid.0 as usize].clone();
    match kind {
        Expr::Ident(name) if pset.fields.contains_key(&name) => {
            let field = pset.fields[&name].clone();
            let base = match &pset.base {
                RefBase::This => ast.add_expr(Expr::This),
                RefBase::SelfAlias => ast.add_expr(Expr::Ident(GEN_SELF_ALIAS.into())),
                RefBase::Frame(handle) => ast.add_expr(Expr::Ident(handle.clone())),
            };
            ast.exprs[eid.0 as usize] = Expr::Member {
                obj: base,
                name: field,
            };
        }
        Expr::BinOp { left, right, .. } => {
            rewrite_params_in_expr(ast, left, pset, visited);
            rewrite_params_in_expr(ast, right, pset, visited);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => {
            rewrite_params_in_expr(ast, expr, pset, visited);
        }
        Expr::InstanceOf { expr, rhs } => {
            rewrite_params_in_expr(ast, expr, pset, visited);
            rewrite_params_in_expr(ast, rhs, pset, visited);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            rewrite_params_in_expr(ast, obj, pset, visited);
        }
        Expr::OptIndex { obj, index } => {
            rewrite_params_in_expr(ast, obj, pset, visited);
            rewrite_params_in_expr(ast, index, pset, visited);
        }
        Expr::OptCall { callee, args } => {
            rewrite_params_in_expr(ast, callee, pset, visited);
            for a in args {
                rewrite_params_in_expr(ast, a, pset, visited);
            }
        }
        Expr::Call { callee, args } => {
            rewrite_params_in_expr(ast, callee, pset, visited);
            for a in args {
                rewrite_params_in_expr(ast, a, pset, visited);
            }
        }
        Expr::Assign { target, value } => {
            rewrite_params_in_expr(ast, target, pset, visited);
            rewrite_params_in_expr(ast, value, pset, visited);
        }
        Expr::Index { obj, index } => {
            rewrite_params_in_expr(ast, obj, pset, visited);
            rewrite_params_in_expr(ast, index, pset, visited);
        }
        Expr::Array(els) => {
            for e in els {
                rewrite_params_in_expr(ast, e, pset, visited);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                // A computed key's expr (`{ [k]: v }`) lives in the
                // objlit_computed_keys side table keyed by the VALUE
                // ExprId, not in the field list — without walking it
                // a lifted-local read inside the brackets (a hoisted
                // yield temp most of all) never becomes `this.<name>`
                // and the checker reports it unknown.
                if let Some(&k) = ast.objlit_computed_keys.get(&e) {
                    rewrite_params_in_expr(ast, k, pset, visited);
                }
                rewrite_params_in_expr(ast, e, pset, visited);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_params_in_expr(ast, cond, pset, visited);
            rewrite_params_in_expr(ast, then_branch, pset, visited);
            rewrite_params_in_expr(ast, else_branch, pset, visited);
        }
        // r633 — the three composite shapes this walk never had an arm
        // for. A generator's locals move onto `this`, and a reference
        // the walk does not reach keeps naming a binding that no longer
        // exists ("unknown identifier", then a ReferenceError at run
        // time) — the same failure the ArrowFn note below records, just
        // reached through a different node. Found from `var x =
        // (log.push("a"), 1)` inside a generator; the other two are its
        // siblings, and a walk that answers for one operand shape has
        // to answer for every one that holds an operand.
        Expr::Sequence { left, right } => {
            rewrite_params_in_expr(ast, left, pset, visited);
            rewrite_params_in_expr(ast, right, pset, visited);
        }
        Expr::Delete { expr } => {
            rewrite_params_in_expr(ast, expr, pset, visited);
        }
        Expr::NewDynamic { callee, args } => {
            rewrite_params_in_expr(ast, callee, pset, visited);
            for e in args {
                rewrite_params_in_expr(ast, e, pset, visited);
            }
        }
        Expr::Nullish { lhs, rhs } => {
            rewrite_params_in_expr(ast, lhs, pset, visited);
            rewrite_params_in_expr(ast, rhs, pset, visited);
        }
        // `new C()` where `C` is one of the lifted locals (a closure
        // value the body bound, `const C = function () { … }`): the
        // parser spells a `new` on a bare name as a class name, but this
        // one names a FIELD now — construct through the value channel
        // (`NewDynamic`, what the parser mints for `new (expr)()`) over
        // the rewritten read, or `route_non_class_new` finds no `C`
        // anywhere (`unknown identifier C`).
        Expr::New {
            class_name,
            args,
            type_args: _,
        } if pset.fields.contains_key(&class_name) => {
            let callee = ast.add_expr(Expr::Ident(class_name));
            rewrite_params_in_expr(ast, callee, pset, visited);
            for e in &args {
                rewrite_params_in_expr(ast, *e, pset, visited);
            }
            ast.exprs[eid.0 as usize] = Expr::NewDynamic { callee, args };
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for e in args {
                rewrite_params_in_expr(ast, e, pset, visited);
            }
        }
        Expr::PostIncr { target, .. } => {
            rewrite_params_in_expr(ast, target, pset, visited);
        }
        // An arrow body is a separate scope, but a nested scope SEES
        // its enclosing bindings — and moving a generator's params and
        // locals onto `this` is a rewrite of those. Skipping the
        // descent left `(n: number) => n + base` reading a `base` that
        // no longer exists as a binding at all ("unknown identifier
        // `base`", then a ReferenceError at run time); nothing
        // downstream could repair it, since `lift_arrow_fns` has
        // already carried the arrow to top level by the time anything
        // else looks.
        //
        // The same omission `nested_fns_idents` carried until r302 —
        // hence the shared `arrow_rebinds`, which drops the names the
        // arrow rebinds itself so a shadowing reference keeps its own
        // meaning.
        //
        // A function EXPRESSION (or an object-literal method — the
        // parser parks both in ArrowFn) owns its `this`, so its body
        // reaches the generator through the `__gself` alias instead
        // (rotation 636: `(function () { return x })()` and
        // `xs.forEach(function () { … x … })` inside `function* g(x)`
        // read `undefined` — the receiver-less call had bound their
        // `this` to undefined, and `this.x` dutifully read nothing).
        // Once inside one, everything below it — arrows included,
        // their `this` being that function's — stays on the alias.
        Expr::ArrowFn { params, body, .. } => {
            let owns_this =
                ast.fn_expr_exprs.contains(&eid) || ast.objlit_method_exprs.contains(&eid);
            let sub = ParamMap {
                fields: pset
                    .fields
                    .iter()
                    .filter(|(k, _)| !super::nested_fns_idents::arrow_rebinds(&params, &body, k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                base: match &pset.base {
                    RefBase::This if owns_this => RefBase::SelfAlias,
                    b => b.clone(),
                },
            };
            if !sub.fields.is_empty() {
                for s in &body {
                    rewrite_params_in_stmt(ast, s, &sub, visited);
                }
            }
        }
        _ => {}
    }
}
