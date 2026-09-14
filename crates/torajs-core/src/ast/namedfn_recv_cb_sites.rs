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
//!
//! The re-declaration half used to be a `collect_decls_by_name`
//! census, non-empty for a `const` of that name in ANY block anywhere,
//! which made it a whole-NAME refusal: one block reusing the spelling
//! cost the receiver at every store / object-literal field / `: any`
//! init / push site of that declaration, program-wide. The five
//! censuses that carried it now ask
//! [`super::namedfn_rebound_scope`] per SITE instead, so a block only
//! takes the reads it actually owns. All five, including the
//! HOF-callback slot: `{ const cb = 3 }` anywhere in the program made
//! `[11].every(cb, r)` answer `false` where bun answers `true`,
//! whether the block was written before the call or after it.

use super::fnexpr_this::is_hof_method;
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
    let mut scopes = super::namedfn_rebound_scope::BlockScopes::default();
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
        if super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, cbname)
            || scopes.owns(ast, cbname, cb)
        {
            continue;
        }
        sites.push((cb, cbname.clone()));
    }
    sites
}

/// 401-02 / 595 — the object-literal FIELD sites: a promoted named fn
/// written as a field value of an object literal (`{ k: 10, m: cb }`,
/// shorthand `{ cb }` included). Read back as `o.m(1)` it is a method
/// call and must see the holder as `this` (§13.3.6.2); the plain
/// `__forward_` shim hardwired `undefined`, so `this` was always wrong
/// on these fields.
///
/// The admission is [`super::fnexpr_this_objlit::objlit_field_idents`]
/// — the census the function-EXPRESSION twin of this family uses for
/// the same position, whose module doc carries the three-link proof
/// that a bare-Ident field's read-back always shifts argv: the one
/// closure repr that would NOT shift (`__mth(`) is minted exclusively
/// for an inline method/closure expression, so a bare Ident's slot is
/// either the binding's plain repr or `any`; both reach
/// `ssa_lower_call_recv_gate` through the struct-method dispatch, the
/// detached-read `closure_local`, or the any lane; and a literal the
/// any-promote verdict boxes into a dynobj is an any-lane read by
/// construction.
///
/// Until rotation 595 this arm demanded the HOLDING binding carry an
/// `: any` annotation, which is not something that proof needs and not
/// something the author of `const o = { m: cb }` has any reason to
/// write. Four spellings were measured wrong against bun and were all
/// the same omission: the plain literal, one with a sibling field, a
/// nested literal, and one reached through a parameter.
///
/// The set is walked in ExprId order rather than the census's own hash
/// order: the sites list decides the order forwarder declarations are
/// appended, and an unordered walk would leak into the artifact bytes —
/// the failure `build_determinism.sh` exists to catch.
pub(super) fn collect_objlit_field_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    sites: &mut Vec<(ExprId, String)>,
) {
    let mut scopes = super::namedfn_rebound_scope::BlockScopes::default();
    let mut fields: Vec<ExprId> = super::fnexpr_this_objlit::objlit_field_idents(&ast.exprs)
        .into_iter()
        .collect();
    fields.sort_by_key(|e| e.0);
    for inner in fields {
        let Expr::Ident(n) = ast.get_expr(inner) else {
            continue;
        };
        if !fn_sigs.contains_key(n)
            || ast.generator_factory_classes.contains_key(n)
            || ast.async_generator_fns.contains(n)
            || super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, n)
            || scopes.owns(ast, n, inner)
        {
            continue;
        }
        sites.push((inner, n.clone()));
    }
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
    let mut scopes = super::namedfn_rebound_scope::BlockScopes::default();
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
            || super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, n)
            || scopes.owns(ast, n, inner)
        {
            continue;
        }
        sites.push((inner, n.clone()));
    }
}

/// Rotation 595 — the container-PUSH sites: a promoted named fn handed
/// to `push` / `unshift` on a runtime-props container (`z.push(fn)`,
/// `rows[0].unshift(fn)`). This is an ARGUMENT position, so
/// `store_target_admits` cannot reach it — there is no assignment and
/// no key — but the fn lands in exactly the slot `z[0] = fn` would
/// have used and comes back out through exactly the same channel, so
/// the receiver half of that verdict
/// ([`super::fnexpr_this_recvs_props::store_receiver_admits`]) is the right
/// bar and is reused rather than re-derived.
///
/// Only the two ES array appenders admit. `splice` / `fill` /
/// `copyWithin` also write slots but take the value at a position that
/// varies with arity, and a user-defined `push` on an `: any` holder is
/// harmless either way: the shell it gets is called through the any
/// lane, which shifts argv on `FLAG_CLOSURE_RECV_FIRST` and so answers
/// `undefined` for a plain `f(x)` inside that method — the same thing
/// the plain forwarder answered.
pub(super) fn collect_props_push_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    sites: &mut Vec<(ExprId, String)>,
) {
    let props_recvs = super::fnexpr_this_recvs_props::collect_props_receiver_binding_names(
        &ast.stmts, &ast.exprs,
    );
    let mut scopes = super::namedfn_rebound_scope::BlockScopes::default();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name: mname } = ast.get_expr(*callee) else {
            continue;
        };
        if !matches!(mname.as_str(), "push" | "unshift")
            || !super::fnexpr_this_recvs_props::store_receiver_admits(
                &ast.exprs,
                &props_recvs,
                *obj,
            )
        {
            continue;
        }
        // Every argument is a stored slot, so each is judged on its
        // own — `z.push(1, fn)` parks the fn just as `z.push(fn)`
        // does. The `as any` shell spells the same intent and erases
        // nothing at runtime, the same peel the other censuses do.
        for a in args {
            let inner = match ast.get_expr(*a) {
                Expr::As { expr, ty_ann } if ty_ann == "any" => *expr,
                _ => *a,
            };
            let Expr::Ident(n) = ast.get_expr(inner) else {
                continue;
            };
            if !fn_sigs.contains_key(n)
                || ast.generator_factory_classes.contains_key(n)
                || ast.async_generator_fns.contains(n)
                || super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, n)
                || scopes.owns(ast, n, inner)
            {
                continue;
            }
            sites.push((inner, n.clone()));
        }
    }
}

/// Rotation 597 — the JSON callback slots: a promoted named fn handed
/// to `JSON.parse(text, R)` as the reviver or to
/// `JSON.stringify(value, R)` as the replacer. §25.5.1.1
/// InternalizeJSONProperty calls the reviver with the HOLDER as
/// `this`, and §25.5.2.2 step 3 calls the replacer the same way (the
/// synthetic `{ "": value }` wrapper at the root), so both slots owe
/// their callback a real receiver rather than the plain shim's
/// hardwired `undefined`.
///
/// This is the arm the function-EXPRESSION twin has had since RFC
/// 20260808-json-parse-any blade 4
/// ([`super::fnexpr_this_cb_slots::collect_json_face`], which admits
/// both slots through one collector for exactly this reason). The
/// named-declaration family never got it, so the same program in the
/// two spellings disagreed: measured, `JSON.parse('{"k":7}', R)` with
/// a named `R` answered a `this` that is not the holder while the
/// `const R = function (…)` spelling answered the holder, matching
/// bun. The kernel was never the problem — `json_reviver.rs` and
/// `json_stringify/replacer.rs` both dispatch through
/// `invoke_with_this`, which seeds the holder off the header flag; it
/// was only ever the shim the site got.
///
/// A rebound `JSON` disarms the whole census. The twin matches the
/// bare `Ident("JSON")` without that test, but the twin's mistake
/// would be to promote a callback for a user-written `parse` whose
/// lane has no receiver channel — a mis-promote, not an over-refusal,
/// so this family does not copy it.
///
/// An ALIAS of the reviver (`const g = R; JSON.parse(t, g)`) stays on
/// the plain route: the site's Ident is `g`, which names no fn decl.
/// That is today's answer, kept.
pub(super) fn collect_json_cb_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    sites: &mut Vec<(ExprId, String)>,
) {
    if super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, "JSON") {
        return;
    }
    let mut scopes = super::namedfn_rebound_scope::BlockScopes::default();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name: mname } = ast.get_expr(*callee) else {
            continue;
        };
        if !matches!(mname.as_str(), "parse" | "stringify")
            || !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "JSON")
        {
            continue;
        }
        let Some(cb) = args.get(1) else {
            continue;
        };
        // The `as any` shell spells the same intent and erases
        // nothing at runtime — the same peel the sibling censuses do.
        let inner = match ast.get_expr(*cb) {
            Expr::As { expr, ty_ann } if ty_ann == "any" => *expr,
            _ => *cb,
        };
        let Expr::Ident(n) = ast.get_expr(inner) else {
            continue;
        };
        if !fn_sigs.contains_key(n)
            || ast.generator_factory_classes.contains_key(n)
            || ast.async_generator_fns.contains(n)
            || super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, n)
            || scopes.owns(ast, n, inner)
        {
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
        scopes: &mut super::namedfn_rebound_scope::BlockScopes,
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
                    && !super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, n)
                    && !scopes.owns(ast, n, inner)
                {
                    sites.push((inner, n.clone()));
                }
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
                walk(inner, ast, fn_sigs, sites, scopes)
            });
        }
    }
    let mut scopes = super::namedfn_rebound_scope::BlockScopes::default();
    walk(&ast.stmts, ast, fn_sigs, sites, &mut scopes);
}

/// RFC 20260915 item 3 — `(F as any)` as the RECEIVER of a `.bind` /
/// `.call` / `.apply` call. The bind kernel's bound cell constructs
/// its target with a fresh `this` (§10.4.1.2), and `new ((F as any)
/// .bind(null))(5)` reached it — but through the plain `__forward_`
/// shim the cast had promoted the value to, which pins `undefined`
/// into `__this`: "cannot assign to a property of this any value".
/// The `: any` binding of the same value (`const fv: any = F`) already
/// constructed correctly because `collect_any_let_init_sites` claims
/// the receiver-first shim for it. These three receivers are the
/// same boundary — each hands the value to a runtime kernel whose
/// calls honor FLAG_CLOSURE_RECV_FIRST — so they claim the
/// `__fwdrecv_` copy, with the same shadow / re-decl / generator
/// gates. Every OTHER `(F as any)` keeps the plain shim: the two
/// shells are distinct cells (RFC 20260717 §8 residue 1 — their
/// `.prototype` objects differ), and `(E as any).prototype` read
/// through the receiver-first copy missed the methods `E.prototype`
/// holds (gate red on `fnexpr-proto-method-this-001`).
pub(super) fn collect_as_any_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    sites: &mut Vec<(ExprId, String)>,
) {
    let mut scopes = super::namedfn_rebound_scope::BlockScopes::default();
    for e in &ast.exprs {
        let Expr::Call { callee, .. } = e else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        if !matches!(name.as_str(), "bind" | "call" | "apply") {
            continue;
        }
        let Expr::As { expr, ty_ann } = ast.get_expr(*obj) else {
            continue;
        };
        if ty_ann != "any" {
            continue;
        }
        let inner = *expr;
        let Expr::Ident(n) = ast.get_expr(inner) else {
            continue;
        };
        if !fn_sigs.contains_key(n)
            || ast.generator_factory_classes.contains_key(n)
            || ast.async_generator_fns.contains(n)
            || super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, n)
            || scopes.owns(ast, n, inner)
            || sites.iter().any(|(e, _)| *e == inner)
        {
            continue;
        }
        sites.push((inner, n.clone()));
    }
}
