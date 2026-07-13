//! P4.prototype-chain Phase A — class-globals synthesis pass.
//!
//! Chunk 343 — extracted from ast.rs. The pass + its doc block (160
//! LOC body + 28 LOC docstring) lift cleanly into a single sibling.
//! ast.rs re-exports the entry so the external caller
//! (`crates/torajs-cli/src/repl.rs`) keeps its existing
//! `torajs_core::ast::synthesize_class_globals` path.

use super::{Ast, Expr, Stmt};

/// P4.prototype-chain Phase A — expose every user-declared class as
/// a first-class value. Runs AFTER `desugar_classes` (which has
/// flattened `ClassDecl` into `TypeDecl` + `__new_<C>` factory /
/// `__cm_<C>__<M>` method / `__sm_<C>__<M>` static FnDecls). For
/// each class C:
///
///   1. Prepends a top-level `let __class_<C>: any = { name: "<C>" };`
///      LetDecl. The ObjectLit lowers to a dynobj-backed Any so the
///      class object behaves like a normal Object at the type / runtime
///      layer. Singleton — multiple `A === A` reads return the same
///      heap pointer.
///   2. Rewrites every `Expr::Ident("<C>")` in value position to
///      `Expr::Ident("__class_<C>")` so user-source `const x = A` and
///      `A.name` etc. resolve to the synthesized global. Static-member
///      calls (`A.staticMethod()`) were already rewritten by
///      `desugar_classes` to bare `__sm_A__staticMethod` Ident calls,
///      so they don't appear as `Member { Ident("A"), ... }` here.
///
/// Constructor call shapes (`new A()` / `A()` for [[Construct]] /
/// [[Call]]) stay on their existing paths — `new A()` is `Expr::New`
/// (separate variant, not Ident) and `A()` as bare call is still
/// rejected as un-callable Any. Real callable constructor object is a
/// follow-up.
///
/// Phase B added the `prototype` field (singleton `__proto_<C>`
/// dynobj); Phase C wired the prototype chain across `extends`;
/// chunk 812 added `length` (ES §15.7.13 expected argument count —
/// formal params before the first default / rest, 0 for a
/// synthesized derived default ctor).
pub fn synthesize_class_globals(ast: &mut Ast) {
    use std::collections::HashSet;

    // Extract class names from the `__new_<C>` factories produced by
    // `desugar_classes`. (ClassDecl stmts are gone post-desugar; the
    // factory FnDecl names are the most stable handle.)
    let mut class_names: Vec<String> = Vec::new();
    // Chunk 812 — `C.length` per ES §15.7.13: the ctor's expected
    // argument count (formal params before the first default / rest).
    // The factory's params ARE the user ctor's, except a synthesized
    // derived default ctor (rest-shaped per spec, so length 0) —
    // desugar records those in `derived_default_ctor_classes`.
    let mut class_lengths: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s
            && let Some(c) = name.strip_prefix("__new_")
        {
            let len = if ast.derived_default_ctor_classes.contains(c) {
                0
            } else {
                params
                    .iter()
                    .take_while(|p| p.default.is_none() && !p.is_rest)
                    .count()
            };
            class_names.push(c.to_string());
            class_lengths.insert(c.to_string(), len);
        }
    }
    if class_names.is_empty() {
        return;
    }
    let class_set: HashSet<String> = class_names.iter().cloned().collect();

    let mut prepended: Vec<Stmt> = Vec::with_capacity(class_names.len() * 3);

    // P4.2 Phase B — `let __proto_<C>: any = {}` per class. Singleton
    // prototype object held in a top-level local; Phase A1's class
    // dynobj's `prototype` field points here, and `Object.getPrototypeOf
    // (instance)` lowers to a load on `__proto_<class_name>`. Empty
    // body — methods stay on the nominal vtable for perf; this is the
    // identity / introspection substrate.
    for cname in &class_names {
        let empty_obj = ast.add_expr(Expr::ObjectLit { fields: vec![] });
        prepended.push(Stmt::LetDecl {
            mutable: false,
            name: format!("__proto_{cname}"),
            type_ann: Some("any".to_string()),
            init: empty_obj,
            is_var: false,
        });
    }

    // Synthesize `let __class_<C>: any = { name: "<C>", prototype:
    // __proto_<C> }`. The `prototype` field is the value that
    // `C.prototype` reads — same any-box that `Object.getPrototypeOf
    // (instance)` returns, so identity holds across both paths via
    // the P4.0 nested-Any-dynobj fix.
    for cname in &class_names {
        let name_expr = ast.add_expr(Expr::String(cname.clone()));
        let proto_ident = ast.add_expr(Expr::Ident(format!("__proto_{cname}")));
        let length_expr = ast.add_expr(Expr::Number(class_lengths[cname] as f64));
        let obj_expr = ast.add_expr(Expr::ObjectLit {
            fields: vec![
                ("name".to_string(), name_expr),
                ("prototype".to_string(), proto_ident),
                ("length".to_string(), length_expr),
            ],
        });
        prepended.push(Stmt::LetDecl {
            mutable: false,
            name: format!("__class_{cname}"),
            type_ann: Some("any".to_string()),
            init: obj_expr,
            is_var: false,
        });
    }

    // P4.2 Phase C — chain wire `__proto_<Sub>.__proto__ = __proto_<Super>`
    // for each class that has a parent. ast.class_parents was
    // populated by desugar_classes; root classes (no parent) are
    // left with an empty `__proto_<C>` whose `__proto__` is missing
    // (read returns ANY_NULL via `__torajs_get_proto_of_any`).
    for cname in &class_names {
        let parent = ast.class_parents.get(cname).cloned().flatten();
        let Some(pname) = parent else { continue };
        let proto_sub = ast.add_expr(Expr::Ident(format!("__proto_{cname}")));
        let proto_super = ast.add_expr(Expr::Ident(format!("__proto_{pname}")));
        let member = ast.add_expr(Expr::Member {
            obj: proto_sub,
            name: "__proto__".to_string(),
        });
        let assign = ast.add_expr(Expr::Assign {
            target: member,
            value: proto_super,
        });
        prepended.push(Stmt::Expr(assign));
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
    for cname in &class_names {
        let proto_ident = ast.add_expr(Expr::Ident(format!("__proto_{cname}")));
        let name_str = ast.add_expr(Expr::String(cname.clone()));
        let callee = ast.add_expr(Expr::Ident("__torajs_proto_register".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![proto_ident, name_str],
        });
        prepended.push(Stmt::Expr(call));
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
        if !class_names.iter().any(|c| c == &cls) {
            continue;
        }
        let proto_ident = ast.add_expr(Expr::Ident(format!("__proto_{cls}")));
        let kind_expr = ast.add_expr(Expr::Number(kind as f64));
        let callee = ast.add_expr(Expr::Ident("__torajs_genfn_chain".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![proto_ident, kind_expr],
        });
        prepended.push(Stmt::Expr(call));
    }

    // P4.5 — parallel registration: store each `__class_<C>` Any-box
    // in the classes-by-tag side table. Read inside `__new_<C>`
    // factory bodies via `__torajs_my_class_ref("<C>")` (intercepted
    // at ssa_lower → emits `__torajs_class_get(<tag_const>)`).
    for cname in &class_names {
        let class_ident = ast.add_expr(Expr::Ident(format!("__class_{cname}")));
        let name_str = ast.add_expr(Expr::String(cname.clone()));
        let callee = ast.add_expr(Expr::Ident("__torajs_class_register".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![class_ident, name_str],
        });
        prepended.push(Stmt::Expr(call));
    }

    // P7.4-a-2 — register each present Error-family class's
    // `__new_<C>` factory into the runtime native-error registry so a
    // runtime native-error throw (bigint RangeError, readonly-prop /
    // Symbol.for TypeError) builds a real catchable instance instead
    // of the bare-string fallback. ssa_lower intercepts this magic
    // call → maps name to its fixed slot + FnAddr(__new_<C>). The
    // arg is a String literal (not an Ident) so the `__class_<C>`
    // rewrite below leaves it untouched. Only the three
    // runtime-throwable classes are wired.
    for cname in &class_names {
        if matches!(cname.as_str(), "Error" | "TypeError" | "RangeError") {
            let name_str = ast.add_expr(Expr::String(cname.clone()));
            let callee = ast.add_expr(Expr::Ident("__torajs_register_native_error".to_string()));
            let call = ast.add_expr(Expr::Call {
                callee,
                args: vec![name_str],
            });
            prepended.push(Stmt::Expr(call));
        }
    }

    // Rewrite Ident("<C>") → Ident("__class_<C>") for each known
    // class name. Walks the entire expr arena since class refs can
    // appear anywhere (let init, fn arg, return value, conditional
    // branches, ...). Synthesized __proto_<C> / __class_<C> idents
    // are not in class_set (their names carry the prefix), so this
    // pass leaves them untouched.
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Ident(name) = &ast.exprs[i]
            && class_set.contains(name)
        {
            let new_name = format!("__class_{name}");
            ast.exprs[i] = Expr::Ident(new_name);
        }
    }

    // Prepend the new LetDecls so they're initialized before any
    // user code references them. Insert at the very top so static
    // field inits + main body all see them.
    let mut combined = prepended;
    combined.extend(std::mem::take(&mut ast.stmts));
    ast.stmts = combined;
}
