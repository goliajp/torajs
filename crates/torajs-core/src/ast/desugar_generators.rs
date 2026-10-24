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

pub(super) mod any_slot;
mod trio;

use super::desugar_generators_prep::YieldInfer;
use super::{Ast, ClassCtor, Expr, Param, Stmt, default_init_for_type};
use trio::build_iterator_method_trio;

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
///        the factory just constructs the default-initialized object)
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
    // read before later passes mint `return` reads or lift generators out of scope
    let infer_yields = YieldInfer::of_program(ast);
    // 393-03 — nested `function*` decls lift to the top level first;
    // the claim walk below reads top-level FnDecls only (rationale
    // in the `_lift` sibling's module doc).
    super::desugar_generators_lift::lift_nested_generators(ast);
    // RFC 20260915 blade 4 — the for-of / for-await protocol helpers a
    // machine body's loops call. Injected HERE, after the nested lift,
    // so a generator declared inside a function is on the roll too (a
    // pipeline-level injection before the async machine pass saw only
    // top-level declarations: `nested-generator-capture-001` lost
    // `__gfor_close`). The helpers are synchronous, so no earlier pass
    // needs to have seen them.
    super::desugar_generators_forof_helpers::inject_for_of_protocol_helpers(ast);
    // D4 — async generators: inject the step driver, mark the bodies'
    // awaits, move the driver (its generator joins the collection).
    super::desugar_generators_async::prepare(ast);
    let mut gen_indices = collect_generator_fn_decls(ast);
    if gen_indices.is_empty() {
        return;
    }
    // every generator is top-level now, and still a `function*` the
    // twin can be cut from; the twins join the claim
    if super::desugar_generators_any_twin::mint_generator_any_twins(ast) {
        gen_indices = collect_generator_fn_decls(ast);
    }

    let fn_sigs = super::desugar_generators_field_ann_fnsigs::collect_declared_fn_return_types(ast);
    let mut appended: Vec<Stmt> = Vec::new();
    // Containers first (642-05): a generator holding a by-reference
    // wrapper edits that wrapper's factory while it is still a decl,
    // so the decl is read from the tree HERE, not snapshotted above.
    for k in super::desugar_generators_frame_link::container_first_order(ast, &gen_indices) {
        let idx = gen_indices[k].0;
        let Stmt::FnDecl {
            name,
            type_params,
            params,
            mut return_type,
            body,
            is_generator: true,
            ..
        } = ast.stmts[idx].clone()
        else {
            panic!(
                "generator `{}` at stmt {idx} is no longer a `function*` decl when its turn comes",
                gen_indices[k].1
            );
        };
        // a generic generator's yields name its type parameters, which
        // the iterator class does not declare
        if !type_params.is_empty() {
            return_type.get_or_insert_with(|| "any".into());
        }
        desugar_one_generator(
            ast,
            idx,
            name,
            params,
            return_type,
            body,
            (&fn_sigs, &infer_yields),
            &mut appended,
        );
    }
    ast.stmts.extend(appended);
}

/// Every top-level `function*` decl's index + name, so the driver loop
/// can mutate `ast.stmts` in place (the assemble step replaces the
/// decl at each `idx`; the appended class stmts land at the end).
fn collect_generator_fn_decls(ast: &Ast) -> Vec<(usize, String)> {
    ast.stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl {
                name,
                is_generator: true,
                ..
            } => Some((i, name.clone())),
            _ => None,
        })
        .collect()
}

/// Full per-generator desugar: rewrite one `function*` decl at `idx`
/// into a `__Gen_<name>` class + `__new_<name>` factory, splicing the
/// new stmts into `appended`. The heavy pipeline stays byte-for-byte
/// from the pre-chunk-4 driver — this helper just carves the loop
/// body out so `desugar_generators` reads as a thin orchestrator.
// the generator decl fields moved out of the stmt plus the pass outputs
#[allow(clippy::too_many_arguments)]
fn desugar_one_generator(
    ast: &mut Ast,
    idx: usize,
    gen_name: String,
    mut gen_params: Vec<Param>,
    gen_ret: Option<String>,
    mut gen_body: Vec<Stmt>,
    (fn_sigs, infer_yields): (&std::collections::HashMap<String, String>, &YieldInfer),
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
    // The resume channel (`next(v)`, `__sent`) is the declared yield
    // type's; an unannotated generator's is `any`, and its yield type
    // is inferred from the prepared body below (`infer_yield_ann`).
    let sent_ty = gen_ret.clone().unwrap_or_else(|| "any".into());

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
    super::desugar_generators_method_names::free_method_names(ast, &mut gen_params, &mut gen_body);
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
        &sent_ty,
        fn_sigs,
    );
    let is_async_gen = ast.async_generator_fns.contains(&gen_name);
    let shape = (&gen_body[..], &rewrite_params[..], &lifted_locals[..]);
    let yield_ty =
        gen_ret.unwrap_or_else(|| infer_yields.yield_type(ast, !is_async_gen, shape, fn_sigs));

    // Class name + struct return type for next().
    let class_name = format!("__Gen_{gen_name}");
    ast.generator_machine_classes.insert(class_name.clone());
    // D4 — an async generator's machine is a SYNC one under `__step*`
    // names (`desugar_generators_async`): the driver does every await,
    // the factory stays un-wrapped (`ag()` answers the generator object
    // per §27.6), and the machine yields its operands verbatim.
    super::gen_fn_expr::register_gen_env_methods(ast, &gen_name, &class_name, is_async_gen);
    let step_ann = format!("__step_{gen_name}");
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
    // Read the body's strictness BEFORE the state machine consumes it
    // — see `carry_strictness_to_next`.
    let body_is_strict = ast.body_says_use_strict(&gen_body);
    let (next_body, gen_hoisted, has_try_regions, has_finally_ret) =
        crate::ast::desugar_generators_assemble::build_state_machine_next_body(
            ast,
            gen_body,
            &yield_ty,
            false,
            &class_name,
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
    let (mut next_method, mut return_method, mut throw_method) = build_iterator_method_trio(
        ast,
        (&yield_ty, &sent_ty),
        &step_ann,
        next_body,
        has_try_regions,
        has_finally_ret,
        is_async_gen,
    );
    let extra_methods = super::desugar_generators_async::shape_machine(
        ast,
        is_async_gen,
        &step_ann,
        &yield_ty,
        (&mut next_method, &mut return_method, &mut throw_method),
        &mut lifted_locals,
    );
    crate::ast::desugar_generators_methods::carry_strictness_to_next(
        ast,
        &mut next_method,
        body_is_strict,
    );
    // For Phase J MVP, generator parameters are stored as fields on
    // the iterator object so the body can reference them through
    // `this.<name>`. The fields are auto-prepended to the class
    // declaration; the ctor's prelude (above) adds an assignment
    // for each param.
    // P10.6-A3 — nominal-marker field whose name is unique per
    // generator class. Generator desugar lifts every yield-fn
    // into a class with structurally identical primitives
    // (`__state: number` + `__sent: <sent_ty>` + params /
    // lifted locals); two `function* a()` + `function* b()`
    // sharing the same sent_ty + parameter shape used to
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
        &sent_ty,
        class_name,
        &lifted_locals,
        ctor.body,
        ctor_destr_lets,
        &destr_leaf_fields,
        next_method,
        return_method,
        throw_method,
        extra_methods,
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
