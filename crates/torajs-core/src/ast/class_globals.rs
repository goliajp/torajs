//! P4.prototype-chain Phase A — class-globals synthesis pass.
//!
//! Chunk 343 — extracted from ast.rs. The pass + its doc block (160
//! LOC body + 28 LOC docstring) lift cleanly into a single sibling.
//! ast.rs re-exports the entry so the external caller
//! (`crates/torajs-cli/src/repl.rs`) keeps its existing
//! `torajs_core::ast::synthesize_class_globals` path.

use super::class_globals_register;
use super::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet};

/// r505 (A12) — the synthesized fn that holds every class's prologue
/// (the `__proto_<C>` / `__class_<C>` mints, the chain links, the
/// registers, the reifies). One fn for the whole program, called once
/// at the top of main.
///
/// Why a fn and not the top of main: the prologue's calls all
/// PRODUCE values (`dynobj_alloc`, the boxes, `str_alloc` for the
/// name), so no single call site can be assumed away by the linker
/// without a garbage register flowing into the next kernel; but the
/// `bl` to a void fn can (`cmd_build_elide_class` offers it under the
/// registry-reader guard next to the register sites), and a fn
/// nothing calls any more is what the user-fn dead-strip removes
/// whole — dynobj / anyvalue / str_alloc worlds and all. The cells
/// the fn mints are owned by the by-tag registry from the moment it
/// returns (its class-cell locals are marked moved, not dropped);
/// main releases them at exit through `class_cell_raw` /
/// `proto_cell_raw`, guarded on 0 so an elided prologue releases
/// nothing. Every other read of a cell already goes through the
/// registry (`class_get` / `proto_get` — the guard's readers), in
/// main now too.
///
/// The name is deliberately NOT `__torajs_`-shaped: user-gc keeps
/// every `___torajs_*` user fn rooted as a runtime-facing definition.
pub const CLASS_PROLOGUE_FN: &str = "__cprologue";

/// Class-index derived from `desugar_classes`' output — shared by all
/// three emit helpers so the walk over `ast.stmts` only happens once.
pub(super) struct ClassMetadata {
    pub(super) class_names: Vec<String>,
    pub(super) class_set: HashSet<String>,
    pub(super) class_lengths: HashMap<String, usize>,
    pub(super) static_shadow: HashSet<String>,
}

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
    let meta = collect_class_metadata(ast);
    if meta.class_names.is_empty() {
        return;
    }

    let mut prepended: Vec<Stmt> = Vec::with_capacity(meta.class_names.len() * 3);
    emit_prototype_and_class_stmts(ast, &meta, &mut prepended);
    emit_chain_and_registration_stmts(ast, &meta, &mut prepended);

    // Rewrite Ident("<C>") → Ident("__class_<C>") for each known
    // class name — shadow-aware (`class_globals_shadow`): a local
    // binding of the same spelling (param / let / var / catch) owns
    // its references, and the flat arena scan this used to be
    // silently handed those to the class object (the
    // param-shadow-class bug). Synthesized __proto_<C> / __class_<C>
    // idents are not in class_set (their names carry the prefix), so
    // the walk leaves them untouched.
    super::class_globals_shadow::rewrite_class_value_refs(ast, &meta.class_set);

    // r505 — the prologue becomes `function __cprologue(): void {…}`
    // plus one call at the very top of main, so static field inits +
    // main body all run after it (see `CLASS_PROLOGUE_FN`).
    let callee = ast.add_expr(Expr::Ident(CLASS_PROLOGUE_FN.to_string()));
    let call = ast.add_expr(Expr::Call {
        callee,
        args: Vec::new(),
    });
    let mut combined = vec![
        Stmt::FnDecl {
            name: CLASS_PROLOGUE_FN.to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Some("void".to_string()),
            body: prepended,
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        },
        Stmt::Expr(call),
    ];
    combined.extend(std::mem::take(&mut ast.stmts));
    ast.stmts = combined;
}

/// Collect class names + lengths + static-shadow markers from the
/// post-`desugar_classes` FnDecl stream.
fn collect_class_metadata(ast: &Ast) -> ClassMetadata {
    // Extract class names from the `__new_<C>` factories produced by
    // `desugar_classes`. (ClassDecl stmts are gone post-desugar; the
    // factory FnDecl names are the most stable handle.)
    let mut class_names: Vec<String> = Vec::new();
    // Chunk 812 — `C.length` per ES §15.7.13: the ctor's expected
    // argument count (formal params before the first default / rest).
    // The factory's params ARE the user ctor's, except a synthesized
    // derived default ctor (rest-shaped per spec, so length 0) —
    // desugar records those in `derived_default_ctor_classes`.
    let mut class_lengths: HashMap<String, usize> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s
            && let Some(c) = name.strip_prefix("__new_")
        {
            let len = if ast.injected_error_classes.contains(c) {
                // §20.5.1 / §20.5.6.2 — every Error-family ctor's
                // `length` is spec-pinned to 1, even though `message`
                // is optional (the injected ctor models that with a
                // `message = ""` default, which the expected-arg
                // count below would read as 0).
                1
            } else if ast.derived_default_ctor_classes.contains(c) {
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
    let class_set: HashSet<String> = class_names.iter().cloned().collect();

    // RFC 20260714-dstr-residual blade 4 — a static method named
    // `name` / `length` shadows the synthesized reflection field
    // (§15.7.14: static members are own properties of the class):
    // the `__class_<C>` field holds the `__sm_<C>__<M>` fn value
    // instead of the string / number.
    let static_shadow: HashSet<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } if name.starts_with("__sm_") => {
                (name.ends_with("__name") || name.ends_with("__length")).then(|| name.clone())
            }
            _ => None,
        })
        .collect();

    ClassMetadata {
        class_names,
        class_set,
        class_lengths,
        static_shadow,
    }
}

/// P4.2 Phase B — `let __proto_<C>: any = {}` + `let __class_<C>: any
/// = { name, prototype: __proto_<C>, length }` for every class. The
/// `prototype` field is the value that `C.prototype` reads — same
/// any-box that `Object.getPrototypeOf(instance)` returns, so identity
/// holds across both paths via the P4.0 nested-Any-dynobj fix.
fn emit_prototype_and_class_stmts(ast: &mut Ast, meta: &ClassMetadata, out: &mut Vec<Stmt>) {
    // Singleton prototype object held in a top-level local; Phase A1's
    // class dynobj's `prototype` field points here, and
    // `Object.getPrototypeOf(instance)` lowers to a load on
    // `__proto_<class_name>`. Empty body — methods stay on the
    // nominal vtable for perf; this is the identity / introspection
    // substrate.
    for cname in &meta.class_names {
        let empty_obj = ast.add_expr(Expr::ObjectLit { fields: vec![] });
        out.push(Stmt::LetDecl {
            mutable: false,
            name: format!("__proto_{cname}"),
            type_ann: Some("any".to_string()),
            init: empty_obj,
            is_var: false,
        });
    }

    for cname in &meta.class_names {
        // RFC 20260714-dstr-residual blade 4 — NamedEvaluation: an
        // anonymous class expression bound by a declaration or a
        // destructuring default reflects the binding identifier as
        // `.name` instead of the `__ClassExpr_<id>` synth name.
        let display = class_display_name(ast, cname).to_string();
        let name_expr = if meta.static_shadow.contains(&format!("__sm_{cname}__name")) {
            ast.add_expr(Expr::Ident(format!("__sm_{cname}__name")))
        } else {
            ast.add_expr(Expr::String(display.into()))
        };
        let proto_ident = ast.add_expr(Expr::Ident(format!("__proto_{cname}")));
        let length_expr = if meta
            .static_shadow
            .contains(&format!("__sm_{cname}__length"))
        {
            ast.add_expr(Expr::Ident(format!("__sm_{cname}__length")))
        } else {
            ast.add_expr(Expr::Number(meta.class_lengths[cname] as f64))
        };
        let obj_expr = ast.add_expr(Expr::ObjectLit {
            fields: vec![
                ("name".into(), name_expr),
                ("prototype".into(), proto_ident),
                ("length".into(), length_expr),
            ],
        });
        out.push(Stmt::LetDecl {
            mutable: false,
            name: format!("__class_{cname}"),
            type_ann: Some("any".to_string()),
            init: obj_expr,
            is_var: false,
        });
    }
}

/// Phase C prototype-chain wire + runtime side-table registration
/// (`__torajs_proto_register` / `__torajs_genfn_chain` /
/// `__torajs_class_register` / `__torajs_register_native_error`).
fn emit_chain_and_registration_stmts(ast: &mut Ast, meta: &ClassMetadata, out: &mut Vec<Stmt>) {
    let gen_class_set: HashSet<String> = ast.generator_factory_classes.values().cloned().collect();
    class_globals_register::emit_proto_chain_and_register(ast, meta, out);
    class_globals_register::emit_class_object_register(ast, meta, &gen_class_set, out);
    class_globals_register::emit_reify_stmts(ast, meta, &gen_class_set, out);
    class_globals_register::emit_native_error_register(ast, meta, out);
}

/// The name a class shows the user — §8.4 NamedEvaluation for a class
/// EXPRESSION, whose parse-time binding name is a `__ClassExpr_<id>`
/// synth. A binding position registers the user's spelling
/// (`const A = class {}` → "A"); with none registered the ES name is
/// the empty string, never the synth. Anything else answers its own
/// declared name.
///
/// The synth used to fall through here, so `(class {}).name` answered
/// "__ClassExpr_4", `console.log(class {})` printed
/// `[class __ClassExpr_0]` and an instance printed
/// `__ClassExpr_0 { x: 1 }` — the implementation's spelling on three
/// faces the user reads.
pub fn class_display_name<'a>(ast: &'a Ast, cname: &'a str) -> &'a str {
    if let Some(display) = ast.class_expr_display_names.get(cname) {
        return display.as_str();
    }
    if cname.starts_with("__ClassExpr_") {
        return "";
    }
    cname
}
