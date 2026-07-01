//! Generator (`function*`) desugar pass — chunk 363, extracted from
//! ast.rs. Companion to `ast/desugar_generators_{prep,rewrite,class,
//! methods,sm}.rs` (chunks 336-341); this file houses the top-level
//! `desugar_generators` driver + the state-machine assembly helper
//! + the yield-into / let-lift walkers that the `_prep` sibling
//! delegates to.
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
//!   * `build_state_machine_next_body` (this file, private) — GenSm
//!     lowering + while-true assembly.
//!   * `desugar_generators_methods::build_{next,return,throw}_method`.
//!   * `desugar_generators_class::assemble_generator_class_and_factory`
//!     — final ClassDecl + `__new_<name>` factory splice.
//!
//! Two `pub(crate)` walkers exposed for `ast::desugar_generators_prep`
//! (which imports them via `super::{expand_yield_into_in_stmt,
//! lift_lets_in_stmt}`):
//!   * `expand_yield_into_in_stmt` (J.4 YieldInto → Yield + LetDecl).
//!   * `lift_lets_in_stmt` (J.2.b let-decl → class-field lift).

use super::{Ast, BinOp, ClassCtor, Expr, Param, Stmt, default_init_for_type};

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
    let gen_indices: Vec<(usize, String, Vec<Param>, Option<String>, Vec<Stmt>)> = ast
        .stmts
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
        .collect();

    if gen_indices.is_empty() {
        return;
    }

    let mut appended: Vec<Stmt> = Vec::new();

    for (idx, gen_name, gen_params, gen_ret, gen_body) in gen_indices {
        // P10.7 — Default-Any generator. When the user omits the
        // return-type annotation (`function* foo() {...}`), infer
        // `Generator<any>` so the body's `yield` values flow through
        // the existing Any-tier (NaN-box AnyValue) via the
        // `Expr::As { …, ty_ann: "any" }` wrap inside
        // `GenSm::emit_yield_return`. `Generator<T>` /
        // `IterableIterator<T>` annotations keep their explicit T.
        let yield_ty = gen_ret.unwrap_or_else(|| "any".into());

        // J.2.a/b + J.4 — prep gen_body: expand yield-into pairs,
        // lift `let` decls to class fields, rewrite param/local
        // idents to `this.<name>`. Returns (prepared_body,
        // lifted_locals) for the class-assembly step below.
        let (gen_body, lifted_locals) = crate::ast::desugar_generators_prep::prep_generator_body(
            ast,
            gen_body,
            &gen_name,
            &gen_params,
            &yield_ty,
        );

        // Class name + struct return type for next().
        let class_name = format!("__Gen_{gen_name}");
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
        let next_body = build_state_machine_next_body(ast, gen_body, &yield_ty);

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
        let yield_arg_default = default_init_for_type(&yield_ty);
        let yield_arg_default_id = ast.add_expr(yield_arg_default);
        let next_method = crate::ast::desugar_generators_methods::build_next_method(
            ast,
            yield_arg_default_id,
            &yield_ty,
            &step_ann,
            next_body,
        );
        let return_method =
            crate::ast::desugar_generators_methods::build_return_method(ast, &yield_ty, &step_ann);
        let throw_method =
            crate::ast::desugar_generators_methods::build_throw_method(ast, &step_ann);
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
            next_method,
            return_method,
            throw_method,
            &mut appended,
        );
    }

    ast.stmts.extend(appended);
}

/// Build the next()'s state-machine body: lower `gen_body` through
/// GenSm into `arms`, then assemble `[while(true) { if(state==0){arm0}
/// if(state==1){arm1} ... ; catch-all }, return {value:0,done:true}]`
/// as the two-stmt body. Caller wraps it into the ClassMethod.
///
/// Kept file-private (not a sibling) so GenSm and `default_init_for_type`
/// stay ast-internal — externalizing would require a handful of
/// pub(super) markers across the GenSm struct, three of its methods,
/// three of its fields, and `default_init_for_type`. Inlining the
/// driver here closes the desugar_generators-LOC HARD limit gap
/// (208 -> 148 LOC) at zero visibility cost.
fn build_state_machine_next_body(ast: &mut Ast, gen_body: Vec<Stmt>, yield_ty: &str) -> Vec<Stmt> {
    // Build the state machine. Each arm is the body of one state in
    // an if-chain wrapped by `while (true) { ... }`. Yields close an
    // arm with `return {value:e, done:false}`; control-flow gotos
    // close with `state = N; continue;` and the `while(true)` loop
    // re-enters the if-chain at the new state.
    let mut sm = super::desugar_generators_sm::GenSm::new(ast, yield_ty.to_string());
    sm.lower_seq(gen_body);
    // After the last body stmt, the natural exit is "done forever".
    let zero = default_init_for_type(yield_ty);
    let zero_id = sm.ast.add_expr(zero);
    let done_lit = sm.ast.add_expr(Expr::Bool(true));
    let final_obj = sm.ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_id), ("done".into(), done_lit)],
    });
    sm.cur_buf.push(Stmt::Return(Some(final_obj)));
    sm.flush_cur();

    // Assemble: while (true) { if (state==0){arm0} if (state==1){arm1} ... ; catch-all }
    let mut loop_body: Vec<Stmt> = Vec::new();
    for (i, arm_stmts) in sm.arms.iter().enumerate() {
        let i_lit = ast.add_expr(Expr::Number(i as f64));
        let this_state = ast.add_expr(Expr::This);
        let state_member = ast.add_expr(Expr::Member {
            obj: this_state,
            name: "__state".into(),
        });
        let cond = ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: state_member,
            right: i_lit,
        });
        loop_body.push(Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Block(arm_stmts.clone())),
            else_branch: None,
        });
    }
    // Catch-all for any state past the last allocated arm (covers
    // unreachable dead-states from break/continue and any "fell off
    // the end" case that didn't return inside the if-chain).
    let zero_tail = default_init_for_type(yield_ty);
    let zero_tail_id = ast.add_expr(zero_tail);
    let done_tail = ast.add_expr(Expr::Bool(true));
    let final_tail = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_tail_id), ("done".into(), done_tail)],
    });
    loop_body.push(Stmt::Return(Some(final_tail)));

    let true_lit = ast.add_expr(Expr::Bool(true));
    // Unreachable trailing return after the `while (true)` — the
    // typechecker's "all paths return" analysis doesn't infer that
    // a `cond=true` while never falls out, so without this the
    // function's tail path looks indeterminate. Cheap to emit, no
    // runtime cost (LLVM dead-code-eliminates it).
    let zero_after = default_init_for_type(yield_ty);
    let zero_after_id = ast.add_expr(zero_after);
    let done_after = ast.add_expr(Expr::Bool(true));
    let final_after = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_after_id), ("done".into(), done_after)],
    });
    vec![
        Stmt::While {
            cond: true_lit,
            body: Box::new(Stmt::Block(loop_body)),
        },
        Stmt::Return(Some(final_after)),
    ]
}

/// J.4 — recursively expand every `Stmt::YieldInto { var, type_ann,
/// value }` in `s` into the pair `[Stmt::Yield(value);
/// Stmt::LetDecl { name: var, type_ann, init: this.__sent }]`. The
/// pair is wrapped in `Stmt::Multi` so it occupies the YieldInto's
/// original slot without disturbing surrounding scope. Walks into
/// nested control-flow.
///
/// `yield_ty` is the surrounding generator's declared yield type; it
/// supplies the let's annotation when the user omitted one (so the
/// J.2.b lift picks the right field type).
pub(crate) fn expand_yield_into_in_stmt(ast: &mut Ast, s: &mut Stmt, yield_ty: &str) {
    match s {
        Stmt::YieldInto {
            var,
            type_ann,
            value,
        } => {
            let var = std::mem::take(var);
            let ty = type_ann.clone().or_else(|| Some(yield_ty.to_string()));
            let value = *value;
            let yield_stmt = Stmt::Yield(value);
            let this_id = ast.add_expr(Expr::This);
            let sent_member = ast.add_expr(Expr::Member {
                obj: this_id,
                name: "__sent".into(),
            });
            let let_stmt = Stmt::LetDecl {
                mutable: true,
                name: var,
                type_ann: ty,
                init: sent_member,
                is_var: false,
            };
            *s = Stmt::Multi(vec![yield_stmt, let_stmt]);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            expand_yield_into_in_stmt(ast, then_branch, yield_ty);
            if let Some(eb) = else_branch.as_deref_mut() {
                expand_yield_into_in_stmt(ast, eb, yield_ty);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                expand_yield_into_in_stmt(ast, i, yield_ty);
            }
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                expand_yield_into_in_stmt(ast, s, yield_ty);
            }
        }
        // Switch / try cases not yet in yield scope (J.2.b).
        _ => {}
    }
}

/// Recursively replace every `let x = init` in `s` (and any nested
/// stmts) with `this.x = init`, recording each lifted `(name, type)`
/// in `lifted`. Used by `desugar_generators` so locals declared in
/// for-init / if-branches / while-bodies survive yield boundaries
/// the same way top-level lets do.
pub(crate) fn lift_lets_in_stmt(ast: &mut Ast, s: &mut Stmt, lifted: &mut Vec<(String, String)>) {
    match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            let n = name.clone();
            let t = type_ann.clone().unwrap_or_else(|| "number".into());
            lifted.push((n.clone(), t));
            let this_id = ast.add_expr(Expr::This);
            let m = ast.add_expr(Expr::Member {
                obj: this_id,
                name: n,
            });
            let assign = ast.add_expr(Expr::Assign {
                target: m,
                value: *init,
            });
            *s = Stmt::Expr(assign);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            lift_lets_in_stmt(ast, then_branch, lifted);
            if let Some(eb) = else_branch.as_deref_mut() {
                lift_lets_in_stmt(ast, eb, lifted);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            lift_lets_in_stmt(ast, body, lifted);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                lift_lets_in_stmt(ast, i, lifted);
            }
            lift_lets_in_stmt(ast, body, lifted);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                lift_lets_in_stmt(ast, s, lifted);
            }
        }
        // Switch / try cases don't yet support yields (J.2.b scope)
        // so their inner lets stay as plain locals — no lift needed.
        _ => {}
    }
}
