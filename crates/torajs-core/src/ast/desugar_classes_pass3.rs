//! `desugar_classes` Pass 3 — ClassDecl-to-TypeDecl replacement +
//! per-class synthetic FnDecl emission (chunk 184, 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` after Pass 2.5 static-member
//! table build. The big for-loop
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
    ast: &mut Ast,
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
    // RFC 20260820-ctor-return-override blade 3 — a class on a chain
    // that touches a value-returning ctor answers `any` instead of
    // `void`; see `desugar_classes_ctor_return::reshape_ctor`.
    let mut ctor_body = ctor_body;
    let return_type = if ast.ctor_return_override.contains(cname) {
        super::desugar_classes_ctor_return::reshape_ctor(ast, &mut params, &mut ctor_body)
    } else {
        "void".to_string()
    };
    appended.push(Stmt::FnDecl {
        name: format!("__cm_{cname}__ctor"),
        type_params: type_params.to_vec(),
        params,
        return_type: Some(return_type),
        body: ctor_body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
    ctor_params_for_factory
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rewrite_classdecls_pass3(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    full_fields: &HashMap<String, Vec<(String, String)>>,
    class_field_inits: &HashMap<String, Vec<(String, ExprId)>>,
    class_field_preludes: &HashMap<String, Vec<Stmt>>,
    appended: &mut Vec<Stmt>,
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
        // The heritage node dies with the ClassDecl — tombstone it, or
        // the orphan Ident reads as a use of the parent binding to
        // every whole-arena analysis (see `Ast::tombstone_expr`).
        if let Stmt::ClassDecl {
            parent: Some(pid), ..
        } = &ast.stmts[*idx]
        {
            let pid = *pid;
            ast.tombstone_expr(pid);
        }
        ast.stmts[*idx] = type_decl;

        // For generic classes, the `__this` type ann must reference
        // the instantiated form, e.g. `Wrapper<T>` not bare `Wrapper`.
        // An exotic-parent class's instance is a REAL builtin cell in
        // the any world (RFC 20260730 blade 1) — typing __this by the
        // class name would send member access down the Obj-layout
        // typed tier against it.
        let this_ann =
            if super::desugar_classes_builtin_heritage::exotic_root_parent(ast, cname).is_some() {
                "any".to_string()
            } else if type_params.is_empty() {
                cname.clone()
            } else {
                format!("{cname}<{}>", type_params.join("|"))
            };

        // Constructor → C__ctor(__this: C, params...): void { body }
        // — see `emit_ctor_fn` doc (default-ctor synthesis +
        // `__new_target` threading).
        let ctor_params_for_factory =
            emit_ctor_fn(ast, cname, type_params, &this_ann, ctor, appended);

        // 405-01 face 2 — the receiver-polymorphic ctor twin, minted
        // when a capturing subclass `extends` this real class (the
        // lane's `super(…)` calls `__ctorany_<C>` directly). RFC
        // 20260815 knife 2b widens the demand: a value-shaped parent
        // (`class D extends box.cls`) can name ANY class at run time,
        // so a program carrying one mints every class's twin — the
        // runtime registry dispatches `super(…)` to it by class cell.
        if ast.es5_real_parents.contains(cname) || !ast.es5_value_parents.is_empty() {
            super::desugar_classes_generic_twin::mint_ctor_generic_twin(ast, cname, ctor, appended);
        }

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
        // RFC 20260820-ctor-return-override blade 3 — a factory that
        // relays its ctor's answer cannot promise the class type: the
        // answer may be any object at all. Saying `C` anyway would
        // send later typed-tier reads at baked field offsets into a
        // foreign cell — the silent wrong this RFC exists to remove.
        let factory_ret = if super::desugar_classes_ctor_return::factory_relays_answer(
            ast,
            cname,
            ctor.is_some(),
        ) {
            "any".to_string()
        } else {
            this_ann.clone()
        };
        appended.push(Stmt::FnDecl {
            name: format!("__new_{cname}"),
            type_params: type_params.clone(),
            params: ctor_params_for_factory,
            return_type: Some(factory_ret),
            body: factory_body,
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        });

        // M-OO.4 / P8.3-A3 — static fields + `static { ... }` blocks
        // in source order; see `emit_static_inits` doc (420-02 — they
        // ride the class's own patch, not a module-top prepend).
        let mut own_statics: Vec<Stmt> = Vec::new();
        super::desugar_classes_statics::emit_static_inits(
            ast,
            cname,
            type_params,
            static_init,
            appended,
            &mut own_statics,
        );

        // M-OO.4 — emit `function __sm_<C>__<name>(...): R { body }`
        // for each static method. See `ast/desugar_classes_emit.rs`.
        emit_class_static_methods(ast, static_methods, cname, type_params, appended);

        // §15.7.14 evaluates every ClassElementName (step 27) before
        // running any static initializer (step 29), so the reifies —
        // which carry the name evaluations — lead. The one residual
        // gap: a computed STATIC FIELD's initializer rides its own
        // name patch, so it runs ahead of a named static field
        // declared above it. Both are at the class now, which is the
        // half that was wrong.
        let mut patch: Vec<Stmt> = Vec::new();
        emit_computed_member_reifies(ast, cname, methods, static_methods, &mut patch);
        patch.extend(own_statics);
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
            //
            // 419-01 — the park is a ToPropertyKey, not a box. `<key>
            // as any` only carried the raw value across, leaving the
            // §7.1.19 conversion to whoever used the key: for an
            // instance field that is the ctor-prefix keyed write, so
            // a throwing `toString` fired once per construction and
            // never at the class definition §15.7.14 puts it at.
            let key_conv = ast.add_expr(Expr::Ident("__torajs_class_computed_key".to_string()));
            let key_any = ast.add_expr(Expr::Call {
                callee: key_conv,
                args: vec![key_eid],
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
