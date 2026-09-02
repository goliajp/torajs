//! `new F()` where `F` is a function, not a class.
//!
//! RFC 20260726-new-on-function blade 2. `desugar_classes_pass2`
//! rewrites every surviving `Expr::New` into a call to `__new_<name>`
//! without checking that the name belongs to a class, and
//! `desugar_classes_pass3` only ever mints that factory for classes. So
//! `new Con(1)` on a plain function reached the checker as a call to an
//! identifier nobody declared: 1138 cases across 42 directories.
//!
//! This pass runs after `bind_this_param`. It mints the missing factory
//! in the same shape the class one has
//! (`lift_arrow_fns::build_factory_body`) but under its own prefix, and
//! repoints the call:
//!
//! ```text
//! function __fnctor_Con(x: number): any {
//!   let __this: any = {};
//!   Con(__this, x);
//!   return __this;
//! }
//! ```
//!
//! The instance is a dynamic object rather than a nominal struct. A
//! class declares its field set; a function assigns `this.x` from
//! whichever branch it likes, so there is no layout to name — which is
//! also why the receiver parameter blade 1 adds is typed `any`.
//!
//! The prefix is deliberately NOT `__new_`. That name is load-bearing
//! for classes in more places than the factory itself: `class_globals`
//! reconstructs the entire class list by stripping it off FnDecl names
//! ("ClassDecl stmts are gone post-desugar; the factory FnDecl names
//! are the most stable handle"), and `ssa_lower_object_lit` reads it to
//! decide an object literal's class tag, vtable and error flag. Minting
//! `__new_Con` for a function therefore announced a class named `Con`,
//! and every `Ident("Con")` — including the one this factory calls —
//! got rewritten to `__class_Con`, which nothing declares.
//!
//! Whether the callee takes `__this` is read off its parameter list
//! instead of being threaded from blade 1: a function that never
//! mentions `this` (`function Empty() {}`) is still constructible, it
//! just does not receive the instance.

use super::{Ast, BinOp, Expr, ExprId, Param, Stmt};
use crate::ast::PropKey;

/// Does any `return <expr>` appear anywhere in this body?
///
/// Spec §10.2.2 step 8 lets a constructor's own return value win over
/// the fresh receiver, but only when it is an Object. Most constructors
/// return nothing, and for those the check is dead weight on every
/// construction — so it is emitted only when the body can actually
/// produce a value. A bare `return;` does not count: it yields
/// undefined, which is never an Object.
pub(super) fn returns_a_value(body: &[Stmt]) -> bool {
    body.iter().any(stmt_returns_a_value)
}

fn stmt_returns_a_value(s: &Stmt) -> bool {
    match s {
        Stmt::Return(v) => v.is_some(),
        Stmt::Block(b) | Stmt::Multi(b) => returns_a_value(b),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_returns_a_value(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_returns_a_value(e))
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => stmt_returns_a_value(body),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| returns_a_value(&c.body))
                || default.as_ref().is_some_and(|d| returns_a_value(d))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            returns_a_value(body)
                || returns_a_value(catch_body)
                || finally_body.as_ref().is_some_and(|f| returns_a_value(f))
        }
        // A nested FnDecl's `return` belongs to that function, not this
        // one.
        _ => false,
    }
}

/// A function this pass can construct: its name, and the parameters the
/// factory has to forward (blade 1's hidden receiver excluded).
pub(super) struct Constructible {
    pub(super) name: String,
    pub(super) params: Vec<Param>,
    pub(super) takes_this: bool,
    /// Body can produce a value, so §10.2.2 step 8 is live for it.
    pub(super) returns_value: bool,
    /// Body reads `arguments` — the factory grows variadic forwarding
    /// when the construct sites agree on an argc (fn_constructor_argc).
    pub(super) body_touches_arguments: bool,
}

fn collect_declared(ast: &Ast) -> (Vec<String>, Vec<Constructible>) {
    let mut declared: Vec<String> = Vec::new();
    let mut candidates: Vec<Constructible> = Vec::new();
    for st in &ast.stmts {
        let Stmt::FnDecl {
            name,
            params,
            body,
            is_generator,
            ..
        } = st
        else {
            continue;
        };
        declared.push(name.clone());
        // A generator's `new` shape is its own question (P-J rewrites it
        // into a state machine), so leave those to the existing path.
        if *is_generator {
            continue;
        }
        let takes_this = params.first().is_some_and(|p| p.name == "__this");
        candidates.push(Constructible {
            name: name.clone(),
            params: if takes_this {
                params[1..].to_vec()
            } else {
                params.clone()
            },
            takes_this,
            returns_value: returns_a_value(body),
            body_touches_arguments: super::fn_constructor_argc::body_touches_arguments(ast, body),
        });
    }
    collect_fn_expr_bindings(ast, &mut candidates);
    (declared, candidates)
}

/// The census's main constructor shape is not a declaration at all:
/// `var Con = function() {};` then `new Con()` — a function EXPRESSION
/// bound to a top-level let/var/const (371 cases at rotation 233's
/// sweep, versus the declaration form blade 2 covered). The factory is
/// the same; the callee just reaches the closure binding instead of a
/// FnDecl name.
///
/// Two gates keep this the loud-failure-preserving subset:
/// - A body that mentions `this` is only accepted after
///   [`promote_fn_expr_ctor_this`] has given it the `__this` param
///   (the promotion has its own strict use-profile gate; a binding it
///   refused keeps the loud reject).
/// - A rest param is skipped: the factory forwards by naming each
///   param, which a `...args` spread-through would misrepresent.
///
/// Untyped params are forwarded as `any` — the instance side is
/// already dynamic, and the closure's own params stay whatever
/// inference gives them.
fn collect_fn_expr_bindings(ast: &Ast, candidates: &mut Vec<Constructible>) {
    for st in &ast.stmts {
        let Stmt::LetDecl { name, init, .. } = st else {
            continue;
        };
        if !ast.fn_expr_exprs.contains(init) {
            continue;
        }
        let Expr::ArrowFn { params, body, .. } = ast.get_expr(*init) else {
            continue;
        };
        if candidates.iter().any(|c| &c.name == name) {
            continue;
        }
        if params.iter().any(|p| p.is_rest) {
            continue;
        }
        let takes_this = params.first().is_some_and(|p| p.name == "__this");
        let user_params = if takes_this {
            &params[1..]
        } else {
            &params[..]
        };
        let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        if !takes_this
            && super::free_vars::free_vars_of_body(ast, &param_names, body)
                .iter()
                .any(|v| v == "__this")
        {
            continue;
        }
        candidates.push(Constructible {
            name: name.clone(),
            params: user_params
                .iter()
                .map(|p| Param {
                    type_ann: p.type_ann.clone().or_else(|| Some("any".into())),
                    ..p.clone()
                })
                .collect(),
            takes_this,
            returns_value: returns_a_value(body),
            body_touches_arguments: super::fn_constructor_argc::body_touches_arguments(ast, body),
        });
    }
}

/// Blade B (rotation 234) — a constructed fn-expr whose body says
/// `this`: `var P = function (n) { this.n = n; }; new P(5)`. The body
/// gets the receiver the same way blade 1 gave it to declarations —
/// `__this: any` becomes the first declared param (inserted while the
/// fn-expr is still an `Expr::ArrowFn`, before `lift_arrow_fns`), the
/// factory passes the fresh instance, and a plain direct call passes
/// `undefined` (§10.2.1.2 strict OrdinaryCallBindThis).
///
/// The promotion changes the closure's native arity, so it is gated
/// on a strict use profile — the ONLY tolerated `Ident(<name>)`
/// position is a `.prototype` member read/write (reads the cell,
/// never calls). Everything else refuses the promotion and the
/// binding keeps today's loud reject:
/// - the value passed along (`g = P`, `arr.push(P)`), `.call` /
///   `.apply` / any other member — an unpromoted body reaching an
///   any-lane invoker with a hidden first param would shift every
///   argument, the exact silent-wrong B-4 narrow-surface forbids;
/// - a plain direct call `P(5)` — the published closure signature
///   deliberately stays `__this`-free (the `__mth(` precedent), so a
///   fixed-up call site still arity-rejects at the checker with a
///   diagnostic counting the hidden param. Refusing keeps the
///   `unknown identifier __this` reject, which at least names the
///   root cause; making mixed profiles work (and speak) is RFC
///   20260726 blade 4's scope.
///
/// Arena entries nothing references (dead mints from earlier
/// rewrites) land in the refuse bucket too — a false negative only
/// keeps the loud reject.
fn promote_fn_expr_ctor_this(ast: &mut Ast) {
    use std::collections::HashSet;

    // Bindings whose fn-expr body mentions `this` and that are
    // actually constructed (`__new_<name>` call exists).
    let mut wanted: Vec<(PropKey, ExprId)> = Vec::new();
    for st in &ast.stmts {
        let Stmt::LetDecl { name, init, .. } = st else {
            continue;
        };
        if !ast.fn_expr_exprs.contains(init) {
            continue;
        }
        let Expr::ArrowFn { params, body, .. } = ast.get_expr(*init) else {
            continue;
        };
        if params.iter().any(|p| p.is_rest || p.name == "__this") {
            continue;
        }
        let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        if !super::free_vars::free_vars_of_body(ast, &param_names, body)
            .iter()
            .any(|v| v == "__this")
        {
            continue;
        }
        let factory = format!("__new_{name}");
        let constructed = ast.exprs.iter().any(
            |e| matches!(e, Expr::Call { callee, .. } if matches!(&ast.exprs[callee.0 as usize], Expr::Ident(n) if *n == factory)),
        );
        if constructed {
            wanted.push((PropKey::from(name), *init));
        }
    }

    for (name, init) in wanted {
        let ident_eids: HashSet<u32> = ast
            .exprs
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Expr::Ident(n) if *n == name))
            .map(|(i, _)| i as u32)
            .collect();
        let mut good: HashSet<u32> = HashSet::new();
        for e in &ast.exprs {
            if let Expr::Member { obj, name: m } = e
                && ident_eids.contains(&obj.0)
                && m == "prototype"
            {
                good.insert(obj.0);
            }
        }
        if ident_eids.iter().any(|e| !good.contains(e)) {
            continue;
        }

        let Expr::ArrowFn { params, .. } = &mut ast.exprs[init.0 as usize] else {
            continue;
        };
        params.insert(
            0,
            Param {
                name: "__this".into(),
                type_ann: Some("any".into()),
                default: None,
                is_rest: false,
            },
        );
    }
}

/// Names called as `__new_<X>` for which no `__new_<X>` was declared,
/// plus the callee expressions to repoint at `__fnctor_<X>`.
fn missing_factories(ast: &Ast, declared: &[String]) -> (Vec<String>, Vec<ExprId>) {
    let mut wanted: Vec<String> = Vec::new();
    let mut callees: Vec<ExprId> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, .. } = e else {
            continue;
        };
        let Expr::Ident(n) = &ast.exprs[callee.0 as usize] else {
            continue;
        };
        let Some(bare) = n.strip_prefix("__new_") else {
            continue;
        };
        if declared.iter().any(|d| d == n) {
            continue; // a real factory exists — class path, untouched
        }
        if !wanted.iter().any(|w| w == bare) {
            wanted.push(bare.to_string());
        }
        callees.push(*callee);
    }
    (wanted, callees)
}

pub fn synthesize_fn_constructors(ast: &mut Ast) {
    promote_fn_expr_ctor_this(ast);
    super::fn_constructor_assign::promote_assign_bound_ctor_this(ast);
    let (declared, mut candidates) = collect_declared(ast);
    super::fn_constructor_assign::collect_assign_bound(ast, &mut candidates);
    let (wanted, callees) = missing_factories(ast, &declared);
    if wanted.is_empty() {
        return;
    }
    let ctor_argc = super::fn_constructor_argc::uniform_construct_argc(ast);

    // Repoint first, and only for names that turn out to be functions:
    // `new NotAThing()` keeps its `__new_NotAThing` callee so the
    // checker still reports the name the source actually wrote.
    for id in callees {
        let Expr::Ident(n) = &ast.exprs[id.0 as usize] else {
            continue;
        };
        let Some(bare) = n.strip_prefix("__new_") else {
            continue;
        };
        if candidates.iter().any(|c| c.name == bare) {
            let repointed = format!("__fnctor_{bare}");
            ast.exprs[id.0 as usize] = Expr::Ident(repointed);
        }
    }

    for bare in wanted {
        // Not a function either — leave the unknown identifier to the
        // checker. Reporting `new NotAThing()` as a missing name is the
        // honest failure; inventing a factory would turn it silent.
        let Some(c) = candidates.iter().find(|c| c.name == bare) else {
            continue;
        };

        // §10.2.2 step 5 (OrdinaryCreateFromConstructor) — the instance's
        // [[Prototype]] is F.prototype, not %Object.prototype%. Built as
        // an object-literal `__proto__` field so it rides the existing
        // literal-proto lane; the `.prototype` read materializes the
        // function's prototype object (with its `constructor` back-ref)
        // on first touch. This is what makes `new F().constructor === F`
        // and the classic `F.prototype.m = ...` method pattern resolve.
        let proto_base = ast.add_expr(Expr::Ident(c.name.clone()));
        let proto_read = ast.add_expr(Expr::Member {
            obj: proto_base,
            name: "prototype".into(),
        });
        let empty_obj = ast.add_expr(Expr::ObjectLit {
            fields: vec![("__proto__".into(), proto_read)],
        });
        let let_this = Stmt::LetDecl {
            mutable: true,
            name: "__this".into(),
            type_ann: Some("any".into()),
            init: empty_obj,
            is_var: false,
        };

        let fparams = super::fn_constructor_argc::factory_params(c, ctor_argc.get(&bare));
        let callee = ast.add_expr(Expr::Ident(c.name.clone()));
        let mut args: Vec<ExprId> = Vec::with_capacity(fparams.len() + 1);
        if c.takes_this {
            args.push(ast.add_expr(Expr::Ident("__this".into())));
        }
        for p in &fparams {
            args.push(ast.add_expr(Expr::Ident(p.name.clone())));
        }
        let call = ast.add_expr(Expr::Call { callee, args });

        // §10.2.2 step 8 — the constructor's own return value wins, but
        // only if it is an Object; anything else (including a bare
        // `return;`) leaves the fresh receiver in place. Emitted only
        // when the body can actually produce a value, so the common
        // constructor pays nothing for it.
        let factory_body = if c.returns_value {
            let r = ast.add_expr(Expr::Ident("__r".into()));
            let let_r = Stmt::LetDecl {
                mutable: false,
                name: "__r".into(),
                type_ann: Some("any".into()),
                init: call,
                is_var: false,
            };
            // `typeof` answers "function" for callables, which are
            // Objects too — hence the second arm rather than a bare
            // `=== "object"`.
            let is_obj = {
                let t1 = ast.add_expr(Expr::TypeOf { expr: r });
                let s1 = ast.add_expr(Expr::String("object".into()));
                let eq1 = ast.add_expr(Expr::BinOp {
                    op: BinOp::Eq,
                    left: t1,
                    right: s1,
                });
                let t2 = ast.add_expr(Expr::TypeOf { expr: r });
                let s2 = ast.add_expr(Expr::String("function".into()));
                let eq2 = ast.add_expr(Expr::BinOp {
                    op: BinOp::Eq,
                    left: t2,
                    right: s2,
                });
                ast.add_expr(Expr::BinOp {
                    op: BinOp::LOr,
                    left: eq1,
                    right: eq2,
                })
            };
            // `typeof null` is "object", so null has to be excluded
            // explicitly or `return null` would win over the receiver.
            let null_lit = ast.add_expr(Expr::Null);
            let not_null = ast.add_expr(Expr::BinOp {
                op: BinOp::Neq,
                left: r,
                right: null_lit,
            });
            let cond = ast.add_expr(Expr::BinOp {
                op: BinOp::LAnd,
                left: not_null,
                right: is_obj,
            });
            let this_ref = ast.add_expr(Expr::Ident("__this".into()));
            let picked = ast.add_expr(Expr::Ternary {
                cond,
                then_branch: r,
                else_branch: this_ref,
            });
            vec![let_this, let_r, Stmt::Return(Some(picked))]
        } else {
            let ret = ast.add_expr(Expr::Ident("__this".into()));
            vec![let_this, Stmt::Expr(call), Stmt::Return(Some(ret))]
        };

        ast.stmts.push(Stmt::FnDecl {
            name: format!("__fnctor_{bare}"),
            type_params: Vec::new(),
            params: fparams,
            return_type: Some("any".into()),
            body: factory_body,
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        });
    }
}
