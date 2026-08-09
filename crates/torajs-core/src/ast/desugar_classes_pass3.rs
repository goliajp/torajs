//! `desugar_classes` Pass 3 — ClassDecl-to-TypeDecl replacement +
//! per-class synthetic FnDecl emission (chunk 184, 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` after Pass 2.5 static-member
//! table build, before the static_field_inits prepend. The big for-loop
//! over `class_index` that does, per class:
//!
//!   * Replace `ast.stmts[idx] = TypeDecl { name, type_params, fields }`
//!     using the flattened field list so subclasses carry parent fields.
//!   * Emit `__cm_<C>__ctor(__this, __new_target, ...ctor_params)` as a
//!     V3-18 wedge — every class gets a callable ctor symbol even when
//!     the user wrote no explicit constructor (subclass `super()` needs
//!     it; factory still elides the no-ctor call).
//!   * Emit instance methods via
//!     `desugar_classes_emit::emit_class_instance_methods`
//!     (records P8.2 accessor entries through `accessor_*_records`).
//!   * Emit factory `__new_<C>(ctor_params): C` via `build_factory_body`.
//!   * Emit static fields as `let __sf_<C>__<name>: T = init`
//!     (mutable for Copy primitives; const for refcounted) + static
//!     blocks as `__sb_<C>__<idx>()` named-fn + top-level Call —
//!     interleaved in source order via `static_init` walk.
//!   * Emit static methods via
//!     `desugar_classes_emit::emit_class_static_methods`.
//!
//! Caller flushes `appended` into `ast.stmts` after this returns
//! (`ast.stmts.extend(appended)`).

use super::desugar_classes_emit::{emit_class_instance_methods, emit_class_static_methods};
use super::desugar_classes_super::ClassIndexEntry;
use super::*;
use std::collections::HashMap;

/// Constructor → `__cm_<C>__ctor(__this: C, __new_target: any,
/// params...): void { body }`. Returns the user ctor params for the
/// factory's signature.
///
/// V3-18 wedge — always emit a `__cm_<C>__ctor` symbol, even when
/// the user wrote no explicit constructor. Per ES spec §15.7.10
/// every class has a default ctor (empty body, or `super(...args)`
/// for subclasses); subclass `super()` calls need a callable parent
/// ctor, and pre-fix tora panicked at typecheck with `unknown
/// identifier __cm_<Parent>__ctor` when the parent had no explicit
/// constructor. The factory still elides the no-ctor call
/// (build_factory_body gates on `ctor.is_some()`), so this only adds
/// an unreferenced empty fn for ctor-less classes — observable only
/// via `super()` in a subclass, which is exactly what we want.
///
/// P4.5 — `__new_target: any` carries the class function that was
/// used at the `new` site. Threaded through `super()` so chain
/// ancestors see the actual derived class, not the static ctor
/// owner. Used inside ctor body via the Expr::NewTarget →
/// Ident("__new_target") rewrite (Pass 2).
fn emit_ctor_fn(
    cname: &str,
    type_params: &[String],
    this_ann: &str,
    ctor: &Option<ClassCtor>,
    appended: &mut Vec<Stmt>,
) -> Vec<Param> {
    let mut ctor_params_for_factory: Vec<Param> = Vec::new();
    let (ctor_body, ctor_user_params): (Vec<Stmt>, Vec<Param>) = if let Some(c) = ctor {
        ctor_params_for_factory = c.params.clone();
        (c.body.clone(), c.params.clone())
    } else {
        (Vec::new(), Vec::new())
    };
    let mut params: Vec<Param> = Vec::with_capacity(ctor_user_params.len() + 2);
    params.push(Param {
        name: "__this".into(),
        type_ann: Some(this_ann.to_string()),
        default: None,
        is_rest: false,
    });
    params.push(Param {
        name: "__new_target".into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    });
    params.extend(ctor_user_params);
    appended.push(Stmt::FnDecl {
        name: format!("__cm_{cname}__ctor"),
        type_params: type_params.to_vec(),
        params,
        return_type: Some("void".into()),
        body: ctor_body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
    ctor_params_for_factory
}

/// M-OO.4 — emit `let __sf_<C>__<name>: T = init;` for each static
/// field (const-form, mutable=false, so K.4 refcount globals accept
/// it; the `init` ExprId is reused — desugar runs before any pass
/// that might mutate the expression referenced by it).
///
/// P8.3-A3 — `static { ... }` blocks share this walk with static
/// fields so spec §15.7.10 source-order interleaving is preserved.
/// Each Block desugars to a named-fn `__sb_<C>__<idx>` (mirrors
/// `__sm_<C>__<m>`, no `__this` param, void return) appended to
/// `ast.stmts`, plus a top-level `Stmt::Expr(Call(...))` at the
/// block's source-order position.
///
/// CRITICAL: static field LetDecls + static block Calls go into
/// `static_field_inits` (NOT `appended`) so they can be prepended to
/// `ast.stmts` before the user's top-level code runs. Otherwise the
/// synth main fn would call `check()` BEFORE the static slot was
/// initialized — every read of `Counter.label` inside `check()`
/// would see the slot's null/zero default. This was a real silent
/// leak + correctness bug uncovered by the m-oo-04-static
/// `leaks --atExit` audit.
fn emit_static_inits(
    ast: &mut Ast,
    cname: &str,
    type_params: &[String],
    static_init: &[StaticInit],
    appended: &mut Vec<Stmt>,
    static_field_inits: &mut Vec<Stmt>,
) {
    for (block_idx, si) in static_init.iter().enumerate() {
        match si {
            StaticInit::Field(sf) => {
                // V3-18 m1.h.26 — static fields are mutable by
                // default (`Counter.value = 5` is valid TS). The
                // historical carve-out that kept refcount-typed
                // fields `mutable: false` (the K.6 globals path once
                // had no dec-old/inc-new for mutable refcount slots,
                // so ssa_lower skipped them and reads failed) is
                // retired — rotation 346: top-level `var g: any` /
                // `var s: string` reassignment has ridden K.6 for a
                // long while, and the false spelling made every
                // WRITE to an uninitialized `static x;` slot (typed
                // "any", init undefined) an unknown-ident reject
                // (the 68-case rs-static-privatename family).
                static_field_inits.push(Stmt::LetDecl {
                    mutable: true,
                    name: format!("__sf_{cname}__{}", sf.name),
                    type_ann: Some(sf.type_ann.clone()),
                    init: sf.init,
                    is_var: false,
                });
                // L3b static-field-reflect (2026-07-22) — mirror the
                // static-METHOD reify: the class object gets a real
                // own data entry so gOPD / any-lane member reads
                // answer (`verifyProperty(C, "<f>", ...)`). Emitted
                // right after the slot's LetDecl (value ready), and
                // the class registration ran earlier (class_globals
                // stmts prepend ahead of static_field_inits). The
                // value rides as the `__sf_` Ident so lowering reuses
                // the plain global-read path.
                let cname_str = ast.add_expr(Expr::String(cname.to_string()));
                let fname_str = ast.add_expr(Expr::String(sf.name.clone()));
                let val_ident = ast.add_expr(Expr::Ident(format!("__sf_{cname}__{}", sf.name)));
                let callee = ast.add_expr(Expr::Ident("__torajs_static_field_reify".to_string()));
                let call = ast.add_expr(Expr::Call {
                    callee,
                    args: vec![cname_str, fname_str, val_ident],
                });
                static_field_inits.push(Stmt::Expr(call));
            }
            StaticInit::Block(stmts) => {
                let fn_name = format!("__sb_{cname}__{block_idx}");
                appended.push(Stmt::FnDecl {
                    name: fn_name.clone(),
                    type_params: type_params.to_vec(),
                    params: Vec::new(),
                    return_type: Some("void".into()),
                    body: stmts.clone(),
                    is_generator: false,
                    span: crate::lexer::Span { start: 0, end: 0 },
                });
                let callee_id = ast.add_expr(Expr::Ident(fn_name));
                let call_id = ast.add_expr(Expr::Call {
                    callee: callee_id,
                    args: Vec::new(),
                });
                static_field_inits.push(Stmt::Expr(call_id));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rewrite_classdecls_pass3(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    full_fields: &HashMap<String, Vec<(String, String)>>,
    class_field_inits: &HashMap<String, Vec<(String, ExprId)>>,
    class_field_preludes: &HashMap<String, Vec<Stmt>>,
    appended: &mut Vec<Stmt>,
    static_field_inits: &mut Vec<Stmt>,
    accessor_getter_records: &mut Vec<(String, String, String)>,
    accessor_setter_records: &mut Vec<(String, String, String)>,
) {
    // RFC 20260802-class-computed-member 刀 2 — per-class reify
    // patches spliced AFTER the TypeDecl at the class-decl position
    // (ClassDefinitionEvaluation: the computed key exprs must see
    // bindings declared above the class, so they cannot ride the
    // module-top class_globals prepend). Collected during the loop,
    // spliced in reverse so earlier indices stay valid.
    let mut computed_patches: Vec<(usize, Vec<Stmt>)> = Vec::new();
    // Pass 3 — rewrite the stmt list. Replace each ClassDecl in-place
    // with its TypeDecl (using the flattened field list so subclasses
    // carry parent fields too), and accumulate the generated FnDecls.
    for (
        idx,
        cname,
        type_params,
        _parent,
        _own_fields,
        static_init,
        ctor,
        methods,
        static_methods,
    ) in class_index
    {
        let type_decl = Stmt::TypeDecl {
            name: cname.clone(),
            type_params: type_params.clone(),
            fields: full_fields[cname].clone(),
        };
        ast.stmts[*idx] = type_decl;

        // For generic classes, the `__this` type ann must reference
        // the instantiated form, e.g. `Wrapper<T>` not bare `Wrapper`.
        // An exotic-parent class's instance is a REAL builtin cell in
        // the any world (RFC 20260730 blade 1) — typing __this by the
        // class name would send member access down the Obj-layout
        // typed tier against it.
        let this_ann = if ast.exotic_parent.contains_key(cname) {
            "any".to_string()
        } else if type_params.is_empty() {
            cname.clone()
        } else {
            format!("{cname}<{}>", type_params.join("|"))
        };

        // Constructor → C__ctor(__this: C, params...): void { body }
        // — see `emit_ctor_fn` doc (default-ctor synthesis +
        // `__new_target` threading).
        let ctor_params_for_factory = emit_ctor_fn(cname, type_params, &this_ann, ctor, appended);

        // Methods → __cm_C__m(__this: C, params...): R { body }
        // See `ast/desugar_classes_emit.rs`.
        emit_class_instance_methods(
            ast,
            methods,
            cname,
            type_params,
            &this_ann,
            &full_fields[cname],
            accessor_getter_records,
            accessor_setter_records,
            appended,
        );

        // Factory: __new_C(ctor_params...): C {
        //   let __this: C = { f0: <init>, f1: <init>, ... };
        //   C__ctor(__this, ctor_params...);   // only if a ctor was declared
        //   return __this;
        // }
        let factory_body = super::build_factory_body(
            ast,
            cname,
            type_params,
            &class_field_inits[cname],
            class_field_preludes.get(cname).cloned().unwrap_or_default(),
            ctor.as_ref(),
        );
        appended.push(Stmt::FnDecl {
            name: format!("__new_{cname}"),
            type_params: type_params.clone(),
            params: ctor_params_for_factory,
            return_type: Some(this_ann.clone()),
            body: factory_body,
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        });

        // M-OO.4 / P8.3-A3 — static fields + `static { ... }` blocks
        // in source order; see `emit_static_inits` doc (CRITICAL
        // init-before-user-code ordering rationale).
        emit_static_inits(
            ast,
            cname,
            type_params,
            static_init,
            appended,
            static_field_inits,
        );

        // M-OO.4 — emit `function __sm_<C>__<name>(...): R { body }`
        // for each static method. See `ast/desugar_classes_emit.rs`.
        emit_class_static_methods(ast, static_methods, cname, type_params, appended);

        let mut patch: Vec<Stmt> = Vec::new();
        emit_computed_member_reifies(ast, cname, methods, static_methods, &mut patch);
        if !patch.is_empty() {
            computed_patches.push((*idx, patch));
        }
    }
    for (idx, patch) in computed_patches.into_iter().rev() {
        ast.stmts.splice(idx + 1..idx + 1, patch);
    }
}

/// RFC 20260802-class-computed-member 刀 2 — one
/// `__torajs_class_computed_reify("<C>", "<sentinel>", <key expr>,
/// kind, is_static)` call per runtime computed member, in DECLARATION
/// order across the instance / static split (the `__ccm_<n>__`
/// sentinel number IS the declaration order — each
/// ComputedPropertyName evaluates once, in order, per §15.7.14).
/// kind: 0 = method, 1 = getter, 2 = setter.
fn emit_computed_member_reifies(
    ast: &mut Ast,
    cname: &str,
    methods: &[ClassMethod],
    static_methods: &[ClassMethod],
    patch: &mut Vec<Stmt>,
) {
    // kind: 0 = method, 1 = getter, 2 = setter (reify call);
    // 3 = instance FIELD (key-global let only — the ctor prefix does
    // the per-construction keyed write); 4 = static FIELD (key-global
    // let + a keyed store onto the class object, both at the
    // class-decl position per §15.7.14 definition-time evaluation).
    let mut entries: Vec<(usize, String, i64, i64, Option<ExprId>)> = Vec::new();
    for (ms, is_static) in [(methods, 0i64), (static_methods, 1i64)] {
        for m in ms {
            let Some(rest) = m.name.strip_prefix("__ccm_") else {
                continue;
            };
            let Ok(n) = rest.trim_end_matches('_').parse::<usize>() else {
                continue;
            };
            let kind = match m.accessor_kind {
                None => 0,
                Some(AccessorKind::Getter) => 1,
                Some(AccessorKind::Setter) => 2,
            };
            entries.push((n, m.name.clone(), kind, is_static, None));
        }
    }
    for (fc, sentinel, init) in &ast.class_computed_static_fields {
        if fc != cname {
            continue;
        }
        let Some(rest) = sentinel.strip_prefix("__ccm_") else {
            continue;
        };
        let Ok(n) = rest.trim_end_matches('_').parse::<usize>() else {
            continue;
        };
        entries.push((n, sentinel.clone(), 4, 1, Some(*init)));
    }
    // Instance fields: every sentinel with a recorded key that is
    // neither a method/accessor nor a static field.
    let claimed: std::collections::HashSet<String> =
        entries.iter().map(|(_, s, ..)| s.clone()).collect();
    for (ck, _) in ast.class_computed_keys.clone() {
        let (kc, sentinel) = ck;
        if kc != cname || claimed.contains(&sentinel) {
            continue;
        }
        let Some(rest) = sentinel.strip_prefix("__ccm_") else {
            continue;
        };
        let Ok(n) = rest.trim_end_matches('_').parse::<usize>() else {
            continue;
        };
        entries.push((n, sentinel, 3, 0, None));
    }
    entries.sort_by_key(|(n, ..)| *n);
    for (n, sentinel, kind, is_static, init) in entries {
        let Some(&key_eid) = ast
            .class_computed_keys
            .get(&(cname.to_string(), sentinel.clone()))
        else {
            continue;
        };
        if kind >= 3 {
            // Field lanes — the evaluated key parks in a module
            // global the ctor prefix / static store reference.
            let key_any = ast.add_expr(Expr::As {
                expr: key_eid,
                ty_ann: "any".into(),
            });
            // `mutable: true` — never reassigned, but the K.6/chunk-809
            // module-global promote (which is what makes the name
            // readable from the `__new_<C>` factory body) only admits
            // mutable refcounted lets; an immutable one stays a
            // main-fn local the factory cannot see.
            patch.push(Stmt::LetDecl {
                mutable: true,
                name: format!("__ccmk_{cname}_{n}"),
                type_ann: Some("any".into()),
                init: key_any,
                is_var: false,
            });
            if kind == 4 {
                let cls_ref = ast.add_expr(Expr::Ident(cname.to_string()));
                let cls_any = ast.add_expr(Expr::As {
                    expr: cls_ref,
                    ty_ann: "any".into(),
                });
                let key_ref = ast.add_expr(Expr::Ident(format!("__ccmk_{cname}_{n}")));
                let lhs = ast.add_expr(Expr::Index {
                    obj: cls_any,
                    index: key_ref,
                });
                let assign = ast.add_expr(Expr::Assign {
                    target: lhs,
                    value: init.expect("static computed field carries its init"),
                });
                patch.push(Stmt::Expr(assign));
            }
            continue;
        }
        let cname_str = ast.add_expr(Expr::String(cname.to_string()));
        let sent_str = ast.add_expr(Expr::String(sentinel));
        let kind_e = ast.add_expr(Expr::Number(kind as f64));
        let stat_e = ast.add_expr(Expr::Number(is_static as f64));
        let callee = ast.add_expr(Expr::Ident("__torajs_class_computed_reify".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![cname_str, sent_str, key_eid, kind_e, stat_e],
        });
        patch.push(Stmt::Expr(call));
    }
}
