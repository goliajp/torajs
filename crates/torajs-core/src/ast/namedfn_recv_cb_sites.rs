//! Which arena slots want the receiver-first shell — the site
//! censuses of [`super::namedfn_recv_cb`].
//!
//! Split out of that file verbatim in rotation 595 on the same line
//! rotation 594 used for `fnexpr_this_recvs_props`: the driver file
//! answers *what the `__fwdrecv_` shell is and how it is minted*,
//! this one answers *which positions ask for it*. The censuses were
//! 246 of that file's 437 prod lines, and the next one to be added
//! (the push/unshift argument arm) does not fit alongside them under
//! the 500-line limit.
//!
//! Every census shares the same three refusals — a name some other
//! declaration shadows or re-declares, a generator-family fn, and a
//! synthetic `__`-prefixed mint — because a promoted closure must
//! only ever reach a call path that shifts argv on
//! `FLAG_CLOSURE_RECV_FIRST`. Over-refusal is the old
//! `undefined`-`this`; a mis-promote is shifted arguments.

use super::fnexpr_this::is_hof_method;
use super::fnexpr_this_names::{collect_decls_by_name, name_shadowed_elsewhere};
use super::fnexpr_this_recvs::{
    collect_any_binding_names, collect_arraylit_binding_names, collect_mapset_binding_names,
};
use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

/// Every `(cb ExprId, target name)` where a promoted named fn sits
/// in a receiver-aware HOF callback slot with a thisArg present.
pub(super) fn collect_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
) -> Vec<(ExprId, String)> {
    let any_recvs = collect_any_binding_names(&ast.stmts, &ast.exprs);
    let arraylit_recvs = collect_arraylit_binding_names(&ast.stmts, &ast.exprs);
    let mapset_recvs = collect_mapset_binding_names(&ast.stmts, &ast.exprs);
    let mut sites: Vec<(ExprId, String)> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name: mname } = ast.get_expr(*callee) else {
            continue;
        };
        // `reduce`/`reduceRight` take no thisArg (§23.1.3.24) — the
        // allowlist already excludes them.
        if !is_hof_method(mname) || args.len() < 2 {
            continue;
        }
        // 398-10 — `cb as any` spells the same intent (the cast
        // erases nothing at runtime); peel it so the inner Ident
        // rewrites while the As shell keeps the checker's any-lane
        // routing (`check_type_of_call_arr_hof_any_cb`).
        let cb = match ast.get_expr(args[0]) {
            Expr::As { expr, ty_ann } if ty_ann == "any" => *expr,
            _ => args[0],
        };
        let Expr::Ident(cbname) = ast.get_expr(cb) else {
            continue;
        };
        if !fn_sigs.contains_key(cbname)
            || ast.generator_factory_classes.contains_key(cbname)
            || ast.async_generator_fns.contains(cbname)
        {
            continue;
        }
        // Receiver-aware gate — knife-4 mirror (see module doc);
        // flatMap joined the typed-receiver set in rotation 286.
        let typed_ok = true;
        let mapset_ok = mname == "forEach";
        let recv_ok = (typed_ok && matches!(ast.get_expr(*obj), Expr::Array(_)))
            || matches!(ast.get_expr(*obj), Expr::Ident(n)
                if any_recvs.contains(n)
                    || (typed_ok && arraylit_recvs.contains(n))
                    || (mapset_ok && mapset_recvs.contains(n)));
        if !recv_ok {
            continue;
        }
        // A shadowed or re-declared name cannot be paired to the top
        // decl syntactically — keep the plain-forwarder route.
        if name_shadowed_elsewhere(&ast.stmts, cbname) {
            continue;
        }
        let mut decls: Vec<(bool, ExprId)> = Vec::new();
        collect_decls_by_name(&ast.stmts, cbname, &mut decls);
        if !decls.is_empty() {
            continue;
        }
        sites.push((cb, cbname.clone()));
    }
    sites
}

/// 401-02 — the object-literal FIELD sites: a promoted named fn
/// stored as a field of an `: any`-annotated binding's literal
/// (`const o: any = { k: 10, m: cb }`). The binding is a dynobj, so
/// EVERY consumer of that field is an any-lane call path — the
/// method-call arm reads `FLAG_CLOSURE_RECV_FIRST` and seeds the
/// holder as the receiver (`o.m(1)` binds `this = o`, §13.3.6.2),
/// while a detached read's plain call / `.call` face rides
/// `invoke_with_this`, which shifts on the same flag. The plain
/// `__forward_` shim hardwired `undefined` instead, so `this` was
/// always wrong on these fields. Same shadow / re-decl / generator
/// gates as the callback sites; the field value admits the bare
/// Ident and its `as any` shell.
pub(super) fn collect_objlit_field_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    sites: &mut Vec<(ExprId, String)>,
) {
    fn walk(
        stmts: &[Stmt],
        ast: &Ast,
        fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
        sites: &mut Vec<(ExprId, String)>,
    ) {
        for s in stmts {
            if let Stmt::LetDecl { type_ann, init, .. } = s
                && type_ann.as_deref() == Some("any")
            {
                let init = super::fnexpr_this_names::peel_as(&ast.exprs, *init);
                if let Expr::ObjectLit { fields } = ast.get_expr(init) {
                    for (_, feid) in fields {
                        let inner = match ast.get_expr(*feid) {
                            Expr::As { expr, ty_ann } if ty_ann == "any" => *expr,
                            _ => *feid,
                        };
                        let Expr::Ident(n) = ast.get_expr(inner) else {
                            continue;
                        };
                        if !fn_sigs.contains_key(n)
                            || ast.generator_factory_classes.contains_key(n)
                            || ast.async_generator_fns.contains(n)
                            || name_shadowed_elsewhere(&ast.stmts, n)
                        {
                            continue;
                        }
                        let mut decls: Vec<(bool, ExprId)> = Vec::new();
                        collect_decls_by_name(&ast.stmts, n, &mut decls);
                        if !decls.is_empty() {
                            continue;
                        }
                        sites.push((inner, n.clone()));
                    }
                }
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
                walk(inner, ast, fn_sigs, sites)
            });
        }
    }
    walk(&ast.stmts, ast, fn_sigs, sites);
}

/// Rotation 594 — the property-STORE sites: a promoted named fn
/// stored into a slot whose every read-back channel honours the
/// receiver (`o.f = fn`, `rows[0][0] = fn`, `this.m = fn`,
/// `C.prototype.k = fn`). Called back as a method, such a value must
/// see the holder as `this` (§13.3.6.2); the plain `__forward_` shim
/// hardwires `undefined`, so `this` was wrong on every one of them —
/// seven spellings measured against bun in rotation 594, all seven
/// answering `undefined` where bun answers the holder, all exit 0.
///
/// The admission is not re-derived here: it is
/// [`super::fnexpr_this_faces::store_target_admits`], the same verdict
/// the function-EXPRESSION twin of this family uses for the same
/// position. Those gates encode which spellings actually come back
/// through a receiver-honouring channel, and a second copy would
/// drift from the first.
pub(super) fn collect_props_store_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    sites: &mut Vec<(ExprId, String)>,
) {
    let props_recvs = super::fnexpr_this_recvs_props::collect_props_receiver_binding_names(
        &ast.stmts, &ast.exprs,
    );
    let expando_recvs = super::fnexpr_this_expando::ExpandoRecvs::scan(&ast.stmts, &ast.exprs);
    let this_store_keys = super::fnexpr_this_store_fields::this_store_keys(&ast.stmts, &ast.exprs);
    for e in &ast.exprs {
        let Expr::Assign { target, value } = e else {
            continue;
        };
        if !super::fnexpr_this_faces::store_target_admits(
            &ast.stmts,
            &ast.exprs,
            &props_recvs,
            &expando_recvs,
            &this_store_keys,
            *target,
        ) {
            continue;
        }
        // The `as any` shell spells the same intent and erases
        // nothing at runtime — same peel the callback sites do.
        let inner = match ast.get_expr(*value) {
            Expr::As { expr, ty_ann } if ty_ann == "any" => *expr,
            _ => *value,
        };
        let Expr::Ident(n) = ast.get_expr(inner) else {
            continue;
        };
        if !fn_sigs.contains_key(n)
            || ast.generator_factory_classes.contains_key(n)
            || ast.async_generator_fns.contains(n)
            || name_shadowed_elsewhere(&ast.stmts, n)
        {
            continue;
        }
        let mut decls: Vec<(bool, ExprId)> = Vec::new();
        collect_decls_by_name(&ast.stmts, n, &mut decls);
        if !decls.is_empty() {
            continue;
        }
        sites.push((inner, n.clone()));
    }
}

/// Rotation 410 — the any-let BARE-INIT sites: a promoted named fn
/// is the whole initializer of an `: any`-annotated binding
/// (`const B: any = F`, the `as any` shell admitted). The binding is
/// an any cell, so EVERY consumer is an any-lane call path, and all
/// of them honor `FLAG_CLOSURE_RECV_FIRST`: a bare `B(5)` rides
/// `invoke_with_this(undefined)` (§10.2.1.2), `B.call(o, 5)` hands
/// its thisArg through the same shift, and a value-shaped-parent
/// `super(…)` (`__torajs_anyv_super_call`'s closure arm — the
/// extracted `let __ccp<N>: any = F` heritage binding spells exactly
/// this shape) threads the subclass-allocated `this`. The plain
/// `__forward_` shim hardwired `undefined` into `__this` on all
/// three, so a receiver-writing fn body threw "cannot assign to a
/// property of this any value". Same shadow / re-decl / generator
/// gates as the callback sites; rotation 366's global-stamp lesson
/// stays honored — only this SITE's copy carries the flag, typed
/// lanes never see it.
pub(super) fn collect_any_let_init_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    sites: &mut Vec<(ExprId, String)>,
) {
    fn walk(
        stmts: &[Stmt],
        ast: &Ast,
        fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
        sites: &mut Vec<(ExprId, String)>,
    ) {
        for s in stmts {
            if let Stmt::LetDecl { type_ann, init, .. } = s
                && type_ann.as_deref() == Some("any")
            {
                let inner = match ast.get_expr(*init) {
                    Expr::As { expr, ty_ann } if ty_ann == "any" => *expr,
                    _ => *init,
                };
                if let Expr::Ident(n) = ast.get_expr(inner)
                    && fn_sigs.contains_key(n)
                    && !ast.generator_factory_classes.contains_key(n)
                    && !ast.async_generator_fns.contains(n)
                    && !name_shadowed_elsewhere(&ast.stmts, n)
                {
                    let mut decls: Vec<(bool, ExprId)> = Vec::new();
                    collect_decls_by_name(&ast.stmts, n, &mut decls);
                    if decls.is_empty() {
                        sites.push((inner, n.clone()));
                    }
                }
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
                walk(inner, ast, fn_sigs, sites)
            });
        }
    }
    walk(&ast.stmts, ast, fn_sigs, sites);
}
