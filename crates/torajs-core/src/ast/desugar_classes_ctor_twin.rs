//! 405-01 face 2 — the receiver-polymorphic CTOR twin
//! `__ctorany_<C>`, split from the method twins
//! (`desugar_classes_generic_twin`) at the file-size line: the ctor
//! twin has its own head (`__this, __new_target, …user`), its own
//! `super(…)` retarget, and the heritage-chain closure that decides
//! which real parents need one. The method twins' slot predicate
//! ([`has_lossy_param`]) moved here at the same line.

use super::ast_def::Ast;
use super::clone_body::BodyCloner;
use super::desugar_classes_generic_twin::{neutralize_clone, restore_dynamic_calls};
use super::expr::{Expr, Param};
use super::stmt::Stmt;
use crate::check_pipeline::TypeAliasTables;
use crate::check_type_ann::{str_or_arr_ann, struct_shaped_ann};

/// 405-01 face 2 — the receiver-polymorphic CTOR twin
/// `__ctorany_<C>` beside `__cm_<C>__ctor`, minted only for the
/// real classes some capturing subclass `extends`
/// (`Ast::es5_real_parents`). The lane's `super(…)` calls it
/// directly: the class's own `.call` correctly throws per §10.3.1,
/// and the mono ctor reads its receiver at baked struct offsets a
/// dynobj instance never has. Head mirrors the mono
/// (`__this, __new_target, …user`), every param typed `any` so
/// field writes lower through the any-member lane.
///
/// A derived parent's twin reaches ITS parent through the retargeted
/// super call (`retarget_super_ctor_twin_calls`), so a chain lands
/// twin to twin. `restore_dynamic_calls` can only be asked to restore
/// member-call shapes it has records for; a dropped restore still
/// neutralizes defensively (the subclass's call then fails loudly on
/// the missing name — never silently).
pub(in crate::ast) fn mint_ctor_generic_twin(
    ast: &mut Ast,
    cname: &str,
    (ctor, fields): (&Option<super::ClassCtor>, &[(super::PropKey, String)]),
    appended: &mut Vec<Stmt>,
) {
    let (body, user_params): (Vec<Stmt>, Vec<Param>) = match ctor {
        Some(c) => (c.body.clone(), c.params.clone()),
        None => (Vec::new(), Vec::new()),
    };
    let mut cloner = BodyCloner::new(ast);
    let mut cloned_body = cloner.clone_stmts(&body);
    // K6-2 — the twin's `this` may be the lane's plain object.
    super::capturing_classes::private_fields::define_in_twin(
        cloner.ast,
        cname,
        fields,
        &mut cloned_body,
    );
    super::clone_body_tables::migrate(&mut cloner);
    retarget_super_ctor_twin_calls(&mut cloner);
    if !restore_dynamic_calls(&mut cloner, &user_params, &body) {
        neutralize_clone(&mut cloner);
        return;
    }
    let mut params: Vec<Param> = Vec::with_capacity(user_params.len() + 2);
    for head in ["__this", "__new_target"] {
        params.push(Param {
            name: head.into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        });
    }
    params.extend(user_params.iter().map(|p| Param {
        name: p.name.clone(),
        type_ann: Some(if p.is_rest { "any[]" } else { "any" }.to_string()),
        default: p.default.map(|d| cloner.clone_expr(d)),
        is_rest: p.is_rest,
    }));
    // §10.2.2 step 13 — the twin answers like the mono constructor of
    // a return-override chain does: whatever the body returns, else
    // the receiver it ran on. Its caller is a `super(…)` site of the
    // runtime-value lane (directly, or through
    // `__torajs_anyv_super_call`), which adopts an object answer as
    // the derived `this` — a void twin dropped `constructor(o) {
    // return o }` on the floor.
    // A derived member of a return-override chain cloned its mono
    // body's field carry, which reads `__this_in` — there the
    // parameter that keeps naming the object the factory minted. Here
    // that object is whatever the twin was called on.
    let recv = cloner.ast.add_expr(Expr::Ident("__this".into()));
    cloned_body.insert(
        0,
        Stmt::LetDecl {
            mutable: false,
            name: "__this_in".into(),
            type_ann: Some("any".into()),
            init: recv,
            is_var: false,
        },
    );
    super::desugar_classes_ctor_return::answering_frame(cloner.ast, &mut cloned_body, 1);
    appended.push(Stmt::FnDecl {
        name: format!("__ctorany_{cname}"),
        type_params: Vec::new(),
        params,
        return_type: Some("any".into()),
        body: cloned_body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
}

/// 405-01 residue (rotation 408) — retarget the pass-1.5 `super(…)`
/// rewrite (`__cm_<P>__ctor(this, __new_target, …user)`) inside a
/// CTOR twin's clone at the parent's own receiver-polymorphic twin,
/// resolved through `es5_ctor_forward` the same way the capturing
/// lane's `super(…)` is (a silent ctor-less link in the chain is
/// skipped — its twin would be an empty body). The args already
/// carry `(this-read, __new_target, …user)`; the clone rebound the
/// this-read onto the twin's own `__this` param, and the hoist's
/// chain admit guarantees the landing is itself admitted, so its
/// twin provably mints. Runs BEFORE `restore_dynamic_calls` so the
/// retargeted sites no longer read as record-less `__cm_` forms (the
/// drop condition). Deliberately ctor-twin-only: a METHOD body's
/// `super.ctor(…)` (a user method literally named `ctor`) rewrites
/// to the same `__cm_<P>__ctor` spelling but with no `__new_target`
/// in its args — the method twins keep their existing drop.
fn retarget_super_ctor_twin_calls(cloner: &mut BodyCloner<'_>) {
    // clone order, not hash order: the retargeted callees are minted in this order
    let mut new_ids: Vec<crate::ast::ExprId> = cloner.map.values().copied().collect();
    new_ids.sort_unstable_by_key(|id| id.0);
    for id in new_ids {
        let Expr::Call { callee, args } = cloner.ast.get_expr(id) else {
            continue;
        };
        let (callee, args) = (*callee, args.clone());
        let Expr::Ident(f) = cloner.ast.get_expr(callee) else {
            continue;
        };
        let Some(parent) = f
            .strip_prefix("__cm_")
            .and_then(|rest| rest.strip_suffix("__ctor"))
        else {
            continue;
        };
        let landing = cloner
            .ast
            .es5_ctor_forward
            .get(parent)
            .cloned()
            .unwrap_or_else(|| parent.to_string());
        let callee2 = cloner
            .ast
            .add_expr(Expr::Ident(format!("__ctorany_{landing}")));
        cloner.ast.exprs[id.0 as usize] = Expr::Call {
            callee: callee2,
            args,
        };
    }
}

/// 405-01 residue (rotation 408) — close `es5_real_parents` over the
/// heritage chain: an admitted DERIVED parent's ctor twin says
/// `super(…)` → a direct `__ctorany_<its parent>` call (see
/// [`retarget_super_ctor_twin_calls`]), so every real ancestor needs
/// its twin minted too. Runs in `desugar_classes` once `parent_map`
/// is in hand (the capturing lane's claim recorded only the direct
/// landing — `class_parents` is not filled until here).
pub(in crate::ast) fn close_real_parents_over_chain(
    ast: &mut Ast,
    parent_map: &std::collections::HashMap<String, Option<String>>,
) {
    let mut work: Vec<String> = ast.es5_real_parents.iter().cloned().collect();
    while let Some(c) = work.pop() {
        if let Some(Some(p)) = parent_map.get(&c)
            && ast.top_root_real_classes.contains(p)
            && ast.es5_real_parents.insert(p.clone())
        {
            work.push(p.clone());
        }
    }
}

/// A parameter slot a missing argument cannot cross without loss:
/// the boxed adapter unboxes `undefined` into an F64 slot as the
/// NaN sentinel and into a Str slot as the immortal undefined cell,
/// but a `boolean` slot takes `false` and an integer slot `0`, which
/// is a silent wrong answer for `typeof b` / `b === undefined`. A
/// method with such a slot mints its twin even when its body never
/// reads the receiver: the bare call (`const f = d.m; f()`) then
/// runs the twin, whose every parameter is `any`, instead of the
/// loud "class method called without a receiver". A slot with a
/// Number / Bool literal default is exempt — the adapter
/// materializes that default from the same `undefined`; any other
/// default is an expression the adapter cannot replay, so it counts
/// like a lossy slot and the twin evaluates it. An instance method's
/// `number` without a default stays out: a method read as a value
/// already seeds its number parameters F64. A static method has no
/// such seed (`num_width::method_value` walks `__cm_` bodies only),
/// so its integer-valued `number` slot is an I64 and counts. A
/// struct-typed slot counts too: the mono body reads it through the
/// layout, which only a fitting struct cell has, and the adapter
/// sends every other argument — an array, a Proxy, a `3` — to the
/// twin, where `p.x` is the generic [[Get]] (§13.3.2.1). Struct-shaped
/// is the checker's reading of the annotation (`struct_shaped_ann`),
/// so an alias chain, an inline object type, a `P | null` and an array
/// of any of them (`Tok[]`, read element by element through the
/// element layout) all count exactly where the struct-escape lane
/// counts them. A `string` or array slot counts as well: neither holds
/// `null`, `undefined` or another kind of value, which the twin binds
/// as passed (§10.2.1.3). So does a function-typed slot, which holds a
/// bare function pointer no value handed over through `any` is.
pub(super) fn has_lossy_param(
    ast: &Ast,
    tables: &TypeAliasTables,
    params: &[Param],
    is_static: bool,
) -> bool {
    params.iter().any(|p| match p.default {
        None => match p.type_ann.as_deref() {
            Some("boolean" | "i64" | "i32") => true,
            Some("number") => is_static,
            // the receiver head carries the class's own annotation
            // and is the mono adapter's env, not an argv slot
            // a typed rest is collected argument by argument, and an
            // argument it cannot hold is the twin's to bind
            Some(ann) if p.is_rest => {
                ann != "any[]" && str_or_arr_ann(ann, &tables.aliases, &tables.generic_alias_decls)
            }
            Some(ann) => {
                p.name != "__this"
                    && (struct_shaped_ann(ann, &tables.aliases, &tables.generic_alias_decls)
                        || str_or_arr_ann(ann, &tables.aliases, &tables.generic_alias_decls)
                        || crate::ast_closure_param_tag::is_fnsig_ann(&p.type_ann))
            }
            None => false,
        },
        // a default that reads `arguments` stays on the direct lane:
        // the adapter's argv carries the declared arity only, so the
        // twin would answer `arguments[3]` as undefined
        Some(d) => {
            !matches!(ast.get_expr(d), Expr::Number(_) | Expr::Bool(_))
                && !super::desugar_async_nested_fns::expr_names_arguments(ast, d)
        }
    })
}
