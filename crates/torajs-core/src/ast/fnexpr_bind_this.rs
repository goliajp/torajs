//! A this-reading fn-expr as a `.bind` receiver.
//!
//! The knife-4 bind desugar (RFC 20260804-fn-this-channel,
//! [`crate::ast_desugar_function_prototype_methods`]) already carries
//! a this-using target: `bind_this_param` made the receiver param 0,
//! so the thisArg just leads the partial list. What it cannot see is
//! a FUNCTION EXPRESSION target — the body's `this` stays an
//! unresolved `__this` capture and the checker rejects loudly. The
//! test262 bind family (`15.3.4.5.1-4-*`) writes exactly that:
//!
//! ```text
//! var func = function () { return this; };
//! var newFunc = Function.prototype.bind.call(func, obj);
//! ```
//!
//! Two steps run here (pipeline: after `rewrite_toplevel_this`,
//! before `synthesize_fn_constructors`, while the fn-expr is still an
//! `Expr::ArrowFn`):
//!
//! 1. **Normalize** `Function.prototype.bind.call(f, args…)` →
//!    `f.bind(args…)` — §22.2.3.4's [[Call]] over the bind builtin is
//!    exactly the method form on the first argument. A user binding
//!    named `Function` owns the name (same shadow rule as the
//!    sibling passes).
//! 2. **Promote** a this-mentioning fn-expr binding whose EVERY
//!    `Ident(<name>)` use stands in `.bind`-receiver position:
//!    `__this: any` becomes the first declared param, and the bind
//!    desugar's this-using lane takes over downstream (the thisArg
//!    becomes the leading partial). Any other use — a direct call, a
//!    value escape, an argument position — refuses the promotion,
//!    and the binding keeps today's loud reject: the promoted arity
//!    is only correct where the bind desugar controls the call.
//!
//! The lift/desugar seam is closed by the alias fallback in the
//! desugar itself (`bind` arm only): by the time it runs,
//! `lift_arrow_fns` has turned the capture-less fn-expr into
//! `Expr::Ident("__closure_N")` pointing at a lifted FnDecl, so the
//! binding resolves through its init to that FnDecl's signature.

use super::{Ast, Expr, Param, Stmt};

/// Step 1 — rewrite `Function.prototype.bind.call(f, args…)` to
/// `f.bind(args…)`. Zero-arg spellings (no receiver to route) stay
/// untouched.
pub fn normalize_function_bind_call(ast: &mut Ast) {
    let user_shadows = ast.stmts.iter().any(|s| {
        matches!(s, Stmt::ClassDecl { name, .. } if name == "Function")
            || matches!(s, Stmt::FnDecl { name, .. } if name == "Function")
            || matches!(s, Stmt::LetDecl { name, .. } if name == "Function")
    });
    if user_shadows {
        return;
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let (callee, args) = (*callee, args.clone());
        let Expr::Member { obj: m1, name: n1 } = ast.get_expr(callee) else {
            continue;
        };
        if n1 != "call" {
            continue;
        }
        let Expr::Member { obj: m2, name: n2 } = ast.get_expr(*m1) else {
            continue;
        };
        if n2 != "bind" {
            continue;
        }
        let Expr::Member { obj: m3, name: n3 } = ast.get_expr(*m2) else {
            continue;
        };
        if n3 != "prototype" || !matches!(ast.get_expr(*m3), Expr::Ident(n) if n == "Function") {
            continue;
        }
        let Some(&recv) = args.first() else {
            continue;
        };
        // Rotation 431 — a literal wrong-brand receiver (a number /
        // string / bool / null literal or the `undefined` name) keeps
        // the ORIGINAL spelling: §20.2.3.2 step 2 makes it a runtime
        // TypeError (IsCallable(Target) false, t262 this-not-callable
        // probes the throw), while `undefined.bind()` is a
        // compile-time member reject. The un-normalized form rides
        // the reified proto method cell's `.call` dispatch, whose
        // IsCallable gate reads back bun-equal.
        let wrong_brand_literal = matches!(
            ast.get_expr(recv),
            Expr::Number(_) | Expr::Bool(_) | Expr::String(_) | Expr::Null
        ) || matches!(ast.get_expr(recv), Expr::Ident(n) if n == "undefined");
        if wrong_brand_literal {
            continue;
        }
        let bind_callee = ast.add_expr(Expr::Member {
            obj: recv,
            name: "bind".into(),
        });
        ast.exprs[i] = Expr::Call {
            callee: bind_callee,
            args: args[1..].to_vec(),
        };
    }
}

/// Step 3 (RFC 20260808-bind-arguments-unified 刀 1) — post-lift
/// registration: a promoted bind-receiver binding's lifted closure
/// joins `ast.fnexpr_recv_fns`, so the construction site stamps
/// `FLAG_CLOSURE_RECV_FIRST` and the runtime bind kernel's
/// `invoke_with_this` routes the bound this into the `__this` slot.
/// Step 2 cannot do this itself — it runs pre-lift, when the lifted
/// name does not exist yet. The static synth_bind lane (capture-less
/// targets) never consults the flag — its wrapper calls the binding
/// directly — so the stamp is inert there; the CAPTURING target,
/// which `collect_closure_aliases` excludes and therefore rides the
/// kernel lane, is who needs it (pre-fix its bound this fell into
/// the plain boxed path's undefined padding: `this === obj` answered
/// false, `this.prop` threw — silent wrong).
///
/// The predicate re-proves step 2's use profile on the post-lift
/// tree rather than carrying state across the lift: a lifted
/// `__closure_N` whose (post-`__env`) first param is `__this` AND
/// whose binding is used exclusively in `.bind`-receiver position.
/// The second leg is what keeps this narrow — a ctor-promoted or
/// face-promoted `__this` closure has non-bind uses (`new F()`, a
/// face argument) and never matches; the fnexpr_this family
/// registers its own fns anyway (duplicate inserts are inert).
pub fn register_bind_receiver_recv_fns(ast: &mut Ast) {
    let mut found: Vec<String> = Vec::new();
    for st in crate::ast::toplevel_stmts_flat(ast) {
        let Stmt::LetDecl { name, init, .. } = st else {
            continue;
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(*init) else {
            continue;
        };
        if !fn_name.starts_with("__closure_") {
            continue;
        }
        let Some(params) = ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name: n, params, ..
            } if n == fn_name => Some(params),
            _ => None,
        }) else {
            continue;
        };
        let user_start = usize::from(params.first().is_some_and(|p| p.name == "__env"));
        if !params.get(user_start).is_some_and(|p| p.name == "__this") {
            continue;
        }
        // Post-lift the by-name profile CANNOT lean on
        // `toplevel_binding_refs` — its Closure arm counts capture
        // names without shadow filtering (the r290 over-approximation),
        // so a harness fn's `func` PARAM captured by its own inner
        // closure poisons both sets. Resolve per fn instead: a use or
        // a capture inside a fn that declares the name as a param
        // belongs to that local; everything else must be a `.bind`
        // receiver.
        let shadow_owned = shadowed_use_eids(ast, name);
        let visible_eids: Vec<u32> = ast
            .exprs
            .iter()
            .enumerate()
            .filter(|(i, e)| matches!(e, Expr::Ident(n) if n == name) && !shadow_owned[*i])
            .map(|(i, _)| i as u32)
            .collect();
        if visible_eids.is_empty() {
            continue;
        }
        let all_bind_receivers = visible_eids.iter().all(|eid| {
            ast.exprs
                .iter()
                .any(|e| matches!(e, Expr::Member { obj, name: m } if obj.0 == *eid && m == "bind"))
        });
        let captured_outside_shadow = ast.exprs.iter().enumerate().any(|(i, e)| {
            matches!(e, Expr::Closure { captures, .. }
                if captures.iter().any(|c| c == name))
                && !shadow_owned[i]
        });
        if all_bind_receivers && !captured_outside_shadow {
            found.push(fn_name.clone());
        }
    }
    // C5 — the inline receiver's lifted mint stands directly in
    // `.bind` position; there is no binding whose use profile could
    // say otherwise (one parent per expression node), so membership
    // needs only the promoted `__this` first param (post-`__env`).
    // Capture-carrying mints qualify too — the kernel lane is
    // exactly where the flag matters.
    for e in &ast.exprs {
        let Expr::Member { obj, name } = e else {
            continue;
        };
        if name != "bind" {
            continue;
        }
        let Expr::Closure { fn_name, .. } = ast.get_expr(*obj) else {
            continue;
        };
        if !fn_name.starts_with("__closure_") {
            continue;
        }
        let Some(params) = ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name: n, params, ..
            } if n == fn_name => Some(params),
            _ => None,
        }) else {
            continue;
        };
        let user_start = usize::from(params.first().is_some_and(|p| p.name == "__env"));
        if params.get(user_start).is_some_and(|p| p.name == "__this") {
            found.push(fn_name.clone());
        }
    }
    for f in found {
        ast.fnexpr_recv_fns.insert(f);
    }
}

/// Every arena expr slot where a by-name use of `name` resolves to
/// something OTHER than the top-level binding:
///
/// 1. the body of a fn declaring `name` as a PARAM (the harness's
///    `func` param is the motivating trap). Param-only on purpose —
///    a body-`let` shadow keeps today's conservative refusal (the
///    nested-decl recursion of `collect_local_binding_names` would
///    over-shadow an OUTER fn's real reads, flipping conservative
///    into wrong);
/// 2. the body of a lifted closure LISTING `name` in its captures —
///    those uses read the env slot, whose value came from the MINT
///    site's scope, so the mint site (not the body) is where the
///    reference really points. Callers judge un-owned mint sites
///    separately: one outside every owned region captures the
///    top-level binding itself.
pub(super) fn shadowed_use_eids(ast: &Ast, name: &str) -> Vec<bool> {
    let mut owned = vec![false; ast.exprs.len()];
    let capture_fns: std::collections::HashSet<&str> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Closure { fn_name, captures } if captures.iter().any(|c| c == name) => {
                Some(fn_name.as_str())
            }
            _ => None,
        })
        .collect();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name: fname,
            params,
            body,
            ..
        } = s
            && (params.iter().any(|p| p.name == name) || capture_fns.contains(fname.as_str()))
        {
            super::desugar_eval::body_owned_exprs(ast, body, &mut owned);
        }
    }
    owned
}

/// Step 2 — give a bind-only this-mentioning fn-expr binding its
/// `__this: any` leading param.
pub fn promote_bind_receiver_this(ast: &mut Ast) {
    // The use profile must not count a SHADOW's uses: the test262
    // harness declares a param named `func` (`__t262_throwsAsync`),
    // and a by-name arena walk saw its body uses and refused every
    // binding the bind family actually names `func`. A use inside a
    // fn body is exempt only when the shadow-aware reference walks
    // prove no fn sees the top-level binding at all — a real capture
    // or named-fn read still refuses (the promoted arity would be
    // wrong there).
    let owned = super::desugar_eval::fn_owned_exprs(ast);
    let refs = crate::ast_refs::toplevel_binding_refs(ast);
    let mut wanted: Vec<super::ExprId> = Vec::new();
    for st in crate::ast::toplevel_stmts_flat(ast) {
        let Stmt::LetDecl { name, init, .. } = st else {
            continue;
        };
        if !ast.fn_expr_exprs.contains(init) {
            continue;
        }
        // A second top-level declarator of the same name makes the
        // by-name use walk unpairable — refuse (loud).
        if crate::ast::toplevel_stmts_flat(ast)
            .iter()
            .filter(|s| matches!(s, Stmt::LetDecl { name: n, .. } if n == name))
            .count()
            != 1
        {
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
        let visible_eids: Vec<u32> = ast
            .exprs
            .iter()
            .enumerate()
            .filter(|(i, e)| matches!(e, Expr::Ident(n) if n == name) && !owned[*i])
            .map(|(i, _)| i as u32)
            .collect();
        if visible_eids.is_empty() {
            continue;
        }
        let all_bind_receivers = visible_eids.iter().all(|eid| {
            ast.exprs
                .iter()
                .any(|e| matches!(e, Expr::Member { obj, name: m } if obj.0 == *eid && m == "bind"))
        });
        if all_bind_receivers
            && !refs.named_fn_refs.contains(name)
            && !refs.closure_captured.contains(name)
        {
            wanted.push(*init);
        }
    }
    // C5 (rotation 435) — the INLINE receiver:
    // `(function () { …this… }).bind(obj)` has no binding to
    // profile — the fn-expr node itself stands in `.bind`-receiver
    // position, and an expression node has exactly one parent, so
    // the zero-other-uses proof the binding walk needs holds by
    // construction (the `.apply` inline arm in the cb-slot face
    // argues the same way). The lifted mint then rides the runtime
    // bind kernel lane (the desugar's sig resolve only reads Ident
    // receivers), which is why step 3 registers it below.
    let n_exprs = ast.exprs.len();
    for i in 0..n_exprs {
        let Expr::Member { obj, name } = &ast.exprs[i] else {
            continue;
        };
        if name != "bind" {
            continue;
        }
        let obj = *obj;
        if !ast.fn_expr_exprs.contains(&obj) {
            continue;
        }
        let Expr::ArrowFn { params, body, .. } = ast.get_expr(obj) else {
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
        wanted.push(obj);
    }
    for init in wanted {
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
