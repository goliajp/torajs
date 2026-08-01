//! Generator (`function*`) desugar pass — chunk 363, extracted from
//! ast.rs. Companion to `ast/desugar_generators_{prep,rewrite,class,
//! methods,sm}.rs` (chunks 336-341); this file houses the top-level
//! `desugar_generators` driver + the state-machine assembly helper;
//! the yield-into / let-lift walkers the `_prep` sibling delegates
//! to live in `desugar_generators_walkers.rs`.
//!
//! Pub entry `desugar_generators` (Phase J MVP) walks
//! `ast.stmts` for every top-level `is_generator: true` FnDecl and
//! rewrites it into a class `__Gen_<name>` with `__state` +
//! `__sent` fields and a `next()` / `return()` / `throw()` method
//! trio. The body's `yield e` becomes an arm return
//! `{ value: e, done: false }`; each control-flow join between
//! yields becomes a distinct state; the enclosing `next()` reruns
//! the state machine via a `while(true) if(state == N) { arm }`
//! chain. Handled by the sibling helpers this file calls into:
//!   * `desugar_generators_prep::prep_generator_body` — J.2.a/b + J.4
//!     yield-into expansion, let-lift, `this.<name>` rewrite.
//!   * `desugar_generators_assemble::build_state_machine_next_body`
//!     — GenSm lowering + while-true assembly (moved out for RFC
//!     20260802 C0; this file was touching the 500-LOC limit).
//!   * `desugar_generators_methods::build_{next,return,throw}_method`.
//!   * `desugar_generators_class::assemble_generator_class_and_factory`
//!     — final ClassDecl + `__new_<name>` factory splice.

use super::{Ast, ClassCtor, Expr, Param, Stmt, default_init_for_type};

/// M5.1 — desugar `class C { ... }` into `type C = {...}` + a series of
/// top-level `function` declarations (ctor / methods / `__new_C` factory).
///
/// This pass MUST run before `lift_arrow_fns` (so arrow fns inside method
/// bodies are still ArrowFn at desugar time) and before `check.rs`. The
/// SSA / runtime layer never sees `Stmt::ClassDecl` / `Expr::This` /
/// `Expr::New` — they are erased here.
///
/// Rewrites performed:
///
///   1. For each class C with method m:
///      - registers `m → C` in a global method table so call-sites
///        `obj.m(...)` can be retargeted to `C__m(obj, ...)`.
///      - duplicate method names across classes are an error (M5.1
///        single-dispatch table; M5.2 will introduce vtables / interfaces).
///   2. Walks every `Expr` in the arena once:
///      - `Expr::This` → `Expr::Ident("__this")`
///      - `Expr::Call { callee = Member{obj, name=m}, args }` where m is a
///        known class method → `Call { callee = Ident("C__m"), args = [obj, ...args] }`
///      - `Expr::New { class_name=C, args }` → `Call { callee = Ident("__new_C"), args }`
///   3. For each `Stmt::ClassDecl`: replace in-place with the corresponding
///      `Stmt::TypeDecl` (fields preserved verbatim), then append:
///      - `function __new_C(args): C { let __this: C = {field0: 0, ...}; C__ctor(__this, args); return __this; }`
///        (ctor params copied; factory return type is C; if no ctor declared,
///         the factory just constructs the default-initialized object)
///      - `function C__ctor(__this: C, ctor_params...): void { body }`
///      - `function C__methodName(__this: C, params...): R { body }` for each method
///
/// The factory's default-initialization strategy: every field gets a typed
/// zero literal (number → 0, string → "", boolean → false, T[] → [], any
/// other named type → calls __new_T() recursively if it's a class, else
/// errors at typecheck). Constructors are responsible for filling fields
/// before they're observably read.
/// Phase J — rewrite every `function*` generator into a class + factory.
/// MVP scope: linear yield sequences (no loops / conditionals between
/// yields). The desugar lowers the body into a `while (true) { ... }`
/// state machine where each yield is one resume point.
///
/// J.2.b — `yield` is allowed inside `if` / `while` / `for` (any
/// nesting). Each yield gets its own state arm. Control flow that
/// crosses a yield boundary becomes `this.__state = N; continue;`
/// gotos through the wrapping `while (true)`. Loop break / continue
/// inside a yield-containing loop rewrite to gotos toward the loop's
/// post-state / step-state respectively. yield-FREE inner control
/// flow is emitted inline so its own break/continue keep their
/// natural semantics.
///
/// For `function* gen(): T { stmt0; yield e0; stmt1; yield e1; }`:
///   - emit a class `__Gen_gen` with field `__state: number` (0-init).
///   - emit `next(): { value: T, done: boolean }` whose body is
///     `while (true) { if (state==0){...} if (state==1){...} ... return {0, true}; }`.
///     Each arm runs its prelude, then either returns `{value:e, done:false}`
///     for a yield, or sets `state=N` and `continue;` for a goto.
///   - emit a factory FnDecl `gen()` returning `__Gen_gen`.
///
/// MVP restrictions logged at desugar-time:
///   - generator return-type annotation supplies the yield value type.
///     Required (no `function* gen()` without `: T`).
///   - yields inside `try` / `catch` / `finally` / `switch` / nested
///     functions are rejected at this stage (no states allocated for them).
///   - all `let` declarations anywhere in the body are lifted to class
///     fields. Same name in two scopes is an error (panic) since both
///     would map to the same `this.<name>` field.
pub fn desugar_generators(ast: &mut Ast) {
    let gen_indices = collect_generator_fn_decls(ast);
    if gen_indices.is_empty() {
        return;
    }

    let mut appended: Vec<Stmt> = Vec::new();
    for (idx, gen_name, gen_params, gen_ret, gen_body) in gen_indices {
        desugar_one_generator(
            ast,
            idx,
            gen_name,
            gen_params,
            gen_ret,
            gen_body,
            &mut appended,
        );
    }
    ast.stmts.extend(appended);
}

/// Snapshot every top-level `function*` decl's index + signature +
/// body so the driver loop can safely mutate `ast.stmts` in place
/// (the assemble_generator_class_and_factory call splices at each
/// captured `idx`).
fn collect_generator_fn_decls(
    ast: &Ast,
) -> Vec<(usize, String, Vec<Param>, Option<String>, Vec<Stmt>)> {
    ast.stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl {
                name,
                params,
                return_type,
                body,
                is_generator: true,
                ..
            } => Some((
                i,
                name.clone(),
                params.clone(),
                return_type.clone(),
                body.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// Full per-generator desugar: rewrite one `function*` decl at `idx`
/// into a `__Gen_<name>` class + `__new_<name>` factory, splicing the
/// new stmts into `appended`. The heavy pipeline stays byte-for-byte
/// from the pre-chunk-4 driver — this helper just carves the loop
/// body out so `desugar_generators` reads as a thin orchestrator.
fn desugar_one_generator(
    ast: &mut Ast,
    idx: usize,
    gen_name: String,
    mut gen_params: Vec<Param>,
    gen_ret: Option<String>,
    mut gen_body: Vec<Stmt>,
    appended: &mut Vec<Stmt>,
) {
    // Un-annotated generator params flow through the Any-tier.
    // The __Gen ctor / fields / factory all clone these params;
    // leaving ann=None fails check_fn_type's mandatory-ann rule
    // on `__cm___Gen_*__ctor` (implicit-generics' `__this` arm
    // never back-fills method params), and the old field-only
    // `"number"` fallback was wrong for non-number defaults.
    for p in &mut gen_params {
        if p.type_ann.is_none() {
            p.type_ann = Some("any".into());
        }
    }
    // P10.7 — Default-Any generator. When the user omits the
    // return-type annotation (`function* foo() {...}`), infer
    // `Generator<any>` so the body's `yield` values flow through
    // the existing Any-tier (NaN-box AnyValue) via the
    // `Expr::As { …, ty_ann: "any" }` wrap inside
    // `GenSm::emit_yield_return`. `Generator<T>` /
    // `IterableIterator<T>` annotations keep their explicit T.
    let yield_ty = gen_ret.unwrap_or_else(|| "any".into());

    // Param-destructuring prefix: parse_fn recorded how many
    // synthesized `let leaf = __param_destr_N.<field>` stmts it
    // prepended to the body. Peel them off — they move into the
    // __Gen ctor (eager binding at the factory call per ES §9.2
    // FunctionDeclarationInstantiation; a throwing destructure
    // must fire at `f()`, not at the first `next()`). Leaf names
    // become class fields so the state machine reads them via
    // `this.<leaf>`.
    let destr_prefix = ast
        .gen_param_destr_prefix
        .get(&gen_name)
        .copied()
        .unwrap_or(0);
    let ctor_destr_lets: Vec<Stmt> = gen_body.drain(..destr_prefix).collect();
    let destr_leaf_fields: Vec<String> = ctor_destr_lets
        .iter()
        .filter_map(|s| match s {
            Stmt::LetDecl { name, .. } if !name.starts_with("__nested_destr_") => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();

    // J.2.a/b + J.4 — prep gen_body: expand yield-into pairs,
    // lift `let` decls to class fields, rewrite param/local
    // idents to `this.<name>`. Returns (prepared_body,
    // lifted_locals) for the class-assembly step below.
    // Destr leaf names join the rewrite set so body reads of a
    // destructured binding resolve to the ctor-assigned field.
    let mut rewrite_params = gen_params.clone();
    for leaf in &destr_leaf_fields {
        rewrite_params.push(Param {
            name: leaf.clone(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        });
    }
    let captures_arguments =
        push_arguments_capture(ast, &gen_params, &gen_body, &mut rewrite_params);
    let (gen_body, mut lifted_locals) = crate::ast::desugar_generators_prep::prep_generator_body(
        ast,
        gen_body,
        &gen_name,
        &rewrite_params,
        &yield_ty,
    );

    // Class name + struct return type for next().
    let class_name = format!("__Gen_{gen_name}");
    let step_ann = format!("__step_{gen_name}");
    // Async generator (RFC 20260713 blade 4): the factory itself
    // stays un-wrapped (parser kept the name out of async_fns —
    // ag() answers the generator object directly per §27.6), and
    // the step methods pick up their Promise<__step_*> shape via
    // the class-method async rewrite: registering the mangled
    // names here is all desugar_classes_emit needs.
    let is_async_gen = ast.async_generator_fns.contains(&gen_name);
    if is_async_gen {
        for m in ["next", "return", "throw"] {
            ast.async_fns.insert(format!("__cm_{class_name}__{m}"));
        }
    }
    // Type alias `type __step_<gen> = { value: T, done: boolean }`.
    ast.stmts.push(Stmt::TypeDecl {
        name: step_ann.clone(),
        type_params: Vec::new(),
        fields: vec![
            ("value".into(), yield_ty.clone()),
            ("done".into(), "boolean".into()),
        ],
    });

    // Build the state machine + while-true loop body + tail return.
    // Yields close an arm with `return {value:e, done:false}`;
    // control-flow gotos close with `state = N; continue;` and the
    // `while(true)` loop re-enters the if-chain at the new state.
    let (next_body, gen_hoisted, has_try_regions, has_finally_ret) =
        crate::ast::desugar_generators_assemble::build_state_machine_next_body(
            ast, gen_body, &yield_ty,
        );
    // RFC 20260802 — the SM's hoisted catch-param slots become class
    // fields alongside the lifted locals.
    lifted_locals.extend(gen_hoisted);

    // Build the generator class with __state field + ctor + next().
    let zero_init = default_init_for_type("number");
    let zero_init_id = ast.add_expr(zero_init);
    let ctor = ClassCtor {
        params: gen_params.clone(),
        body: vec![Stmt::Expr({
            let this_id = ast.add_expr(Expr::This);
            let state_member = ast.add_expr(Expr::Member {
                obj: this_id,
                name: "__state".into(),
            });
            ast.add_expr(Expr::Assign {
                target: state_member,
                value: zero_init_id,
            })
        })],
    };
    // J.4 — next() takes an optional `__yield_arg: <yield_ty> = 0`
    // parameter and stashes it in `this.__sent` before re-entering
    // the state machine. YieldInto-expanded `let v = this.__sent`
    // sites read that field to receive the value passed to
    // `g.next(arg)`. First call's arg is ignored per JS spec; tr's
    // typed-default uses zero/empty depending on yield type.
    // NOT default_init_for_type(&yield_ty): apply_default_args
    // groups method defaults by NAME ("next"), so every __Gen
    // class's __yield_arg default must stay shape-uniform — the
    // first-seen class's default ExprId pads every `it.next()`
    // call site. An any-lane `undefined` default here would leak
    // into a number-lane next() (gate-caught: "argument 0:
    // expected Number, got Undefined"). The first call's arg is
    // ignored per spec, so the numeric zero is only ever a
    // placeholder.
    let yield_arg_default = if yield_ty == "any" {
        Expr::Number(0.0)
    } else {
        default_init_for_type(&yield_ty)
    };
    let yield_arg_default_id = ast.add_expr(yield_arg_default);
    let next_method = crate::ast::desugar_generators_methods::build_next_method(
        ast,
        yield_arg_default_id,
        &yield_ty,
        &step_ann,
        next_body,
    );
    let return_method = crate::ast::desugar_generators_methods::build_return_method(
        ast,
        &yield_ty,
        &step_ann,
        has_finally_ret,
        is_async_gen,
    );
    let throw_method =
        crate::ast::desugar_generators_methods::build_throw_method(ast, &step_ann, has_try_regions);
    // For Phase J MVP, generator parameters are stored as fields on
    // the iterator object so the body can reference them through
    // `this.<name>`. The fields are auto-prepended to the class
    // declaration; the ctor's prelude (above) adds an assignment
    // for each param.
    // P10.6-A3 — nominal-marker field whose name is unique per
    // generator class. Generator desugar lifts every yield-fn
    // into a class with structurally identical primitives
    // (`__state: number` + `__sent: <yield_ty>` + params /
    // lifted locals); two `function* a()` + `function* b()`
    // sharing the same yield_ty + parameter shape used to
    // collapse to the same struct sid, and ssa_lower:18693's
    // sibling-class static dispatch picked the first-matching
    // alias from a HashMap iter — non-deterministically
    // routing `a().next()` to `__cm_<other>__next`. The
    // marker breaks structural equivalence per generator (V8
    // / SpiderMonkey side-step the same problem with nominal
    // class identity; tora's structural type system isn't
    // changing in this phase, so a per-class field-name
    // marker is the narrow fix that keeps the dispatch
    // correct without an SSA-level type-system overhaul).
    crate::ast::desugar_generators_class::assemble_generator_class_and_factory(
        ast,
        idx,
        gen_name,
        gen_params,
        &yield_ty,
        class_name,
        &lifted_locals,
        ctor.body,
        ctor_destr_lets,
        &destr_leaf_fields,
        next_method,
        return_method,
        throw_method,
        captures_arguments,
        appended,
    );
}

/// RFC 20260801-arguments-method-face knife 2a — true when a
/// generator body touches `arguments` and the FACTORY's argv can
/// carry it: joining the rewrite set maps every body `arguments`
/// ident to the `this.arguments` class field; the ctor takes it as
/// a trailing `any` param and the factory passes `[...arguments]`,
/// whose inline-spread rewrite compiles under EVERY argc mode (a
/// static-argv-qualified factory expands the exact call-site argv,
/// extras included; a non-qualified one degrades to the declared
/// params — parity with the historical next()-arity fold, never a
/// compile break). A body that declares its own `arguments` binding
/// keeps it (shadow, pre-face semantics). Class gen METHODS
/// (parser-synth `__cm_gen_*` with a `__genrecv` first param) are
/// excluded: their real argv lives at the class-side forwarder
/// (which drops over-arity today) and the factory's own arguments
/// would put the RECEIVER in slot 0. The forwarder argv channel is
/// knife 2b (RFC, registered).
fn push_arguments_capture(
    ast: &Ast,
    gen_params: &[Param],
    gen_body: &[Stmt],
    rewrite_params: &mut Vec<Param>,
) -> bool {
    use crate::ast::arguments_object_walkers as aw;
    let mut local_binds = std::collections::HashSet::new();
    crate::ast_collect_bindings::collect_local_binding_names(gen_body, &mut local_binds);
    let captures = !gen_params.first().is_some_and(|p| p.name == "__genrecv")
        && !local_binds.contains("arguments")
        && !gen_params.iter().any(|p| p.name == "arguments")
        && (aw::body_has_non_length_arguments_touch(ast, gen_body)
            || aw::body_has_arguments_length(ast, gen_body));
    if captures {
        rewrite_params.push(Param {
            name: "arguments".into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        });
    }
    captures
}
