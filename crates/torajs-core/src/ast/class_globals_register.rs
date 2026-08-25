//! Phase C registration emits, split out of
//! [`super::class_globals::emit_chain_and_registration_stmts`]
//! (rotation 147, file-size fn debt). Each helper appends one
//! family of runtime side-table registrations; the orchestrator
//! calls them in the original order, which is load-bearing — the
//! `__torajs_error_proto_install` emits read the `PROTOS_BY_TAG_IMM`
//! slots that the `__torajs_proto_register` emits above them fill.

use std::collections::HashSet;

use super::class_globals::ClassMetadata;
use super::{Ast, Expr, Stmt};

/// Prototype-object wiring: the `__proto__` chain links, the
/// tag-keyed `__torajs_proto_register` side table, and the
/// %GeneratorPrototype% chain for generator classes.
pub(super) fn emit_proto_chain_and_register(
    ast: &mut Ast,
    meta: &ClassMetadata,
    out: &mut Vec<Stmt>,
) {
    // P4.2 Phase C — chain wire `__proto_<Sub>.__proto__ = __proto_<Super>`
    // for each class that has a parent. ast.class_parents was
    // populated by desugar_classes; root classes (no parent) are
    // left with an empty `__proto_<C>` whose `__proto__` is missing
    // (read returns ANY_NULL via `__torajs_get_proto_of_any`).
    for cname in &meta.class_names {
        let parent = ast.class_parents.get(cname).cloned().flatten();
        let Some(pname) = parent else { continue };
        let proto_sub = ast.add_expr(Expr::Ident(format!("__proto_{cname}")));
        let proto_super = ast.add_expr(Expr::Ident(format!("__proto_{pname}")));
        // RFC 20260825-inject-narrow-define 刀 4a — a Call to the
        // narrow proto-link kernel instead of a `.__proto__ = …`
        // member assign: both operands are the prologue's freshly
        // minted plain dynobjs and the link is the compile-time
        // class tree, so the generic member-set route (whose one
        // reloc kept the whole member-set world alive in every
        // program) has nothing to decide here.
        let callee = ast.add_expr(Expr::Ident("__torajs_proto_link_fresh".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![proto_sub, proto_super],
        });
        out.push(Stmt::Expr(call));
    }

    // P4.2 Phase B+C — register each `__proto_<C>` into the runtime
    // side table keyed by the class's compile-time runtime tag, so
    // `Object.getPrototypeOf(instance)` can look up the prototype
    // from the instance's CLASS_TAG_OFF slot. Emitted as a Call to
    // the magic ident `__torajs_proto_register`, intercepted by
    // ssa_lower → resolves the tag from class_name_to_tag and emits
    // `__torajs_proto_register(<tag_const>, <proto_ident_load>)`.
    // The class-name argument is a String literal so ssa_lower can
    // pick the right tag without re-deriving it from sid.
    for cname in &meta.class_names {
        let proto_ident = ast.add_expr(Expr::Ident(format!("__proto_{cname}")));
        let name_str = ast.add_expr(Expr::String(cname.clone()));
        let callee = ast.add_expr(Expr::Ident("__torajs_proto_register".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![proto_ident, name_str],
        });
        out.push(Stmt::Expr(call));
    }

    // RFC 20260713 blade 5 cut 4 — chain each generator class's
    // prototype object to the shared %GeneratorPrototype% of its
    // kind (§27.3.3.2: the [[Prototype]] of every generator fn's
    // `.prototype` IS %GeneratorPrototype%). Emitted as
    // `__torajs_genfn_chain(__proto_<cls>, <kind>)`, intercepted in
    // the class-synth lowering lane. Sorted for a deterministic
    // emit order (the side table is a HashMap).
    let mut gen_classes: Vec<(String, i64)> = ast
        .generator_factory_classes
        .iter()
        .map(|(factory, cls)| {
            let kind = i64::from(ast.async_generator_fns.contains(factory));
            (cls.clone(), kind)
        })
        .collect();
    gen_classes.sort();
    for (cls, kind) in gen_classes {
        if !meta.class_names.iter().any(|c| c == &cls) {
            continue;
        }
        let proto_ident = ast.add_expr(Expr::Ident(format!("__proto_{cls}")));
        let kind_expr = ast.add_expr(Expr::Number(kind as f64));
        let callee = ast.add_expr(Expr::Ident("__torajs_genfn_chain".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![proto_ident, kind_expr],
        });
        out.push(Stmt::Expr(call));
    }

    // RFC 20260730-iterator-global 刀 1 — chain each stripped-
    // builtin heir's prototype object to its builtin prototype
    // singleton (`class C extends Iterator {}` → C.prototype's
    // [[Prototype]] IS %Iterator.prototype%, §27.1.3). Same emit
    // shape as the genfn chain above; sorted for determinism.
    let mut heirs: Vec<(String, i64)> = ast
        .builtin_proto_heirs
        .iter()
        .map(|(c, t)| (c.clone(), *t))
        .collect();
    heirs.sort();
    for (cls, tag) in heirs {
        if !meta.class_names.iter().any(|c| c == &cls) {
            continue;
        }
        let proto_ident = ast.add_expr(Expr::Ident(format!("__proto_{cls}")));
        let tag_expr = ast.add_expr(Expr::Number(tag as f64));
        let callee = ast.add_expr(Expr::Ident("__torajs_proto_chain_builtin".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![proto_ident, tag_expr],
        });
        out.push(Stmt::Expr(call));
    }
}

/// Class-object registration: the tag-keyed `__class_<C>` side
/// table, plus the own `name` / `message` install for the injected
/// Error family.
pub(super) fn emit_class_object_register(
    ast: &mut Ast,
    meta: &ClassMetadata,
    gen_class_set: &HashSet<String>,
    out: &mut Vec<Stmt>,
) {
    // P4.5 — parallel registration: store each `__class_<C>` Any-box
    // in the classes-by-tag side table. Read inside `__new_<C>`
    // factory bodies via `__torajs_my_class_ref("<C>")` (intercepted
    // at ssa_lower → emits `__torajs_class_get(<tag_const>)`).
    //
    // The third argument flags desugar-synthesized generator classes
    // (`__Gen_<f>` — their `__proto_<C>` IS the generator fn's
    // `.prototype` object, which per §27.3.3.2 carries NO own
    // `constructor`, and the class object itself is unreachable from
    // user code): the runtime skips the first-class MakeConstructor
    // wiring for them (RFC 20260717-class-first-class-value knife A
    // fix-up).
    for cname in &meta.class_names {
        let class_ident = ast.add_expr(Expr::Ident(format!("__class_{cname}")));
        let name_str = ast.add_expr(Expr::String(cname.clone()));
        let is_gen = ast.add_expr(Expr::Number(f64::from(u8::from(
            gen_class_set.contains(cname),
        ))));
        let callee = ast.add_expr(Expr::Ident("__torajs_class_register".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![class_ident, name_str, is_gen],
        });
        out.push(Stmt::Expr(call));
    }

    // RFC 20260718-builtin-error-ctor-first-class 刀 1 — the
    // synthesized Error family carries the §20.5.6.3/6.4 own `name` /
    // `message` data properties on its prototype. Emitted as
    // `__torajs_error_proto_install("<C>")`, intercepted at
    // ssa_lower → (tag, name Str) → runtime define on
    // `PROTOS_BY_TAG_IMM[tag]` (filled by the proto_register emits
    // above). User error subclasses inherit instead (spec shape) —
    // the side table holds injected names only.
    for cname in &meta.class_names {
        if !ast.injected_error_classes.contains(cname) {
            continue;
        }
        let name_str = ast.add_expr(Expr::String(cname.clone()));
        let callee = ast.add_expr(Expr::Ident("__torajs_error_proto_install".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![name_str],
        });
        out.push(Stmt::Expr(call));
    }
}

/// Reification emits (RFC 20260717 knife B cut 2 / RFC
/// 20260718-accessor-reify 刀 2+3): static methods, static
/// accessors, and instance accessors each become a real own entry.
pub(super) fn emit_reify_stmts(
    ast: &mut Ast,
    meta: &ClassMetadata,
    gen_class_set: &HashSet<String>,
    out: &mut Vec<Stmt>,
) {
    // Knife B cut 2 (RFC 20260717-class-first-class-value) — static
    // method reification: `C.staticMethod` must be an own function
    // object of the class object with the §10.2.10 method attribute
    // set. Emitted as `__torajs_static_method_reify("<C>", "<M>")`,
    // intercepted at ssa_lower → resolves `__sm_<C>__<M>`'s boxed
    // adapter and hands runtime the (tag, name, adapter) triple.
    // Call-site dispatch is untouched (static calls were already
    // desugared to bare `__sm_` idents).
    // RFC 20260718-accessor-reify 刀 3 — the accessor FACE FnDecls
    // (`__sm_<C>__<p>_get` / `_set`) stay out of the static-METHOD
    // sweep; they reify as an AccessorPair below. Exact-name set (a
    // user method legitimately named `p_get` keeps its method reify).
    let static_accessor_fns: std::collections::HashSet<String> = ast
        .static_accessor_getters
        .values()
        .chain(ast.static_accessor_setters.values())
        .cloned()
        .collect();
    for cname in &meta.class_names {
        if gen_class_set.contains(cname) {
            continue;
        }
        let prefix = format!("__sm_{cname}__");
        let mnames: Vec<String> = ast
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::FnDecl { name, .. } if !static_accessor_fns.contains(name) => {
                    name.strip_prefix(&prefix).map(str::to_string)
                }
                _ => None,
            })
            // RFC 20260802 刀 2 — a runtime computed member's `__ccm_`
            // sentinel is not a property name; the class-decl-position
            // computed define installs it under its runtime key.
            .filter(|m| !m.starts_with("__ccm_"))
            .collect();
        for m in mnames {
            let cname_str = ast.add_expr(Expr::String(cname.clone()));
            let mname_str = ast.add_expr(Expr::String(m));
            let callee = ast.add_expr(Expr::Ident("__torajs_static_method_reify".to_string()));
            let call = ast.add_expr(Expr::Call {
                callee,
                args: vec![cname_str, mname_str],
            });
            out.push(Stmt::Expr(call));
        }
    }

    // RFC 20260718-accessor-reify 刀 3 — same reify shape for STATIC
    // accessors, onto the class object (`gOPD(C, "s")`).
    {
        let mut pairs: Vec<(String, String)> = ast
            .static_accessor_getters
            .keys()
            .chain(ast.static_accessor_setters.keys())
            .cloned()
            .collect();
        pairs.sort();
        pairs.dedup();
        for (cname, prop) in pairs {
            // RFC 20260802 刀 2 — same `__ccm_` sentinel skip as the
            // instance-accessor sweep below.
            if gen_class_set.contains(&cname) || prop.starts_with("__ccm_") {
                continue;
            }
            let cname_str = ast.add_expr(Expr::String(cname));
            let pname_str = ast.add_expr(Expr::String(prop));
            let callee = ast.add_expr(Expr::Ident(
                "__torajs_class_static_accessor_reify".to_string(),
            ));
            let call = ast.add_expr(Expr::Call {
                callee,
                args: vec![cname_str, pname_str],
            });
            out.push(Stmt::Expr(call));
        }
    }

    // RFC 20260718-accessor-reify 刀 2 — one reify magic per
    // (class, accessor-prop) pair so the prototype carries a real
    // AccessorPair own entry (`gOPD(C.prototype, "x")` answers the
    // reified faces). Emitted as
    // `__torajs_class_accessor_reify("<C>", "<p>")`, intercepted at
    // ssa_lower → resolves the `__cm_<C>__<p>_get` / `_set` boxed
    // adapters and hands runtime the (tag, name, get, set) quad.
    // Compile-time accessor dispatch is untouched.
    {
        let mut pairs: Vec<(String, String)> = ast
            .accessor_getters
            .keys()
            .chain(ast.accessor_setters.keys())
            .cloned()
            .collect();
        pairs.sort();
        pairs.dedup();
        for (cname, prop) in pairs {
            // RFC 20260802 刀 2 — computed accessors (`__ccm_`
            // sentinels) define under their runtime key at the
            // class-decl position, not under the sentinel spelling.
            if gen_class_set.contains(&cname) || prop.starts_with("__ccm_") {
                continue;
            }
            let cname_str = ast.add_expr(Expr::String(cname));
            let pname_str = ast.add_expr(Expr::String(prop));
            let callee = ast.add_expr(Expr::Ident("__torajs_class_accessor_reify".to_string()));
            let call = ast.add_expr(Expr::Call {
                callee,
                args: vec![cname_str, pname_str],
            });
            out.push(Stmt::Expr(call));
        }
    }
}

/// Native-error registry wiring for the runtime-throwable Error
/// classes.
pub(super) fn emit_native_error_register(ast: &mut Ast, meta: &ClassMetadata, out: &mut Vec<Stmt>) {
    // P7.4-a-2 — register each present Error-family class's
    // `__new_<C>` factory into the runtime native-error registry so a
    // runtime native-error throw (bigint RangeError, readonly-prop /
    // Symbol.for TypeError) builds a real catchable instance instead
    // of the bare-string fallback. ssa_lower intercepts this magic
    // call → maps name to its fixed slot + FnAddr(__new_<C>). The
    // arg is a String literal (not an Ident) so the `__class_<C>`
    // rewrite below leaves it untouched. Only the runtime-throwable
    // classes are wired.
    for cname in &meta.class_names {
        if matches!(
            cname.as_str(),
            "Error"
                | "TypeError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                // §27.2.4.2 — `Promise.any`'s all-rejected answer is
                // built by the runtime, so its factory has to be
                // reachable from there like the thrown ones.
                | "AggregateError"
                // §19.2.6 — the URI kernels' malformed-input raise.
                | "URIError"
        ) {
            let name_str = ast.add_expr(Expr::String(cname.clone()));
            let callee = ast.add_expr(Expr::Ident("__torajs_register_native_error".to_string()));
            let call = ast.add_expr(Expr::Call {
                callee,
                args: vec![name_str],
            });
            out.push(Stmt::Expr(call));
        }
    }
}
