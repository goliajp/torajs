//! RFC 20260805 blade 1 — compile an async function to a state machine.
//!
//! An async function's `await` must SUSPEND: control returns to the
//! caller at the first one, and the body resumes from a microtask.
//! tr's `await` lowering instead drains the whole queue and reads the
//! settled slot inline (`ssa_lower_member_promise_value`), so the body
//! runs to completion inside the caller's frame — two async functions
//! never interleave, and the statement after the call runs last.
//!
//! The textbook compilation is a generator state machine plus a
//! promise driver, and tr already owns both halves: `desugar_generators`
//! builds machines with try regions and finally frames, and the
//! microtask queue orders `.then` correctly. This pass connects them:
//!
//! ```text
//! async function f(a: A): Promise<T> { BODY }
//!   ⇒  function* __async_gen__f(a: A): any { BODY′ }   // await → yield
//!      function f(a: A): Promise<any> { return __async_drive(__async_gen__f(a)); }
//! ```
//!
//! Runs BEFORE `desugar_generators`, which then builds `__async_gen__f`'s
//! machine like any other `function*`, and before `desugar_async` — a
//! rewritten function leaves `ast.async_fns`, so its `return e` keeps
//! the bare value the driver's `resolve` settles with.
//!
//! **Which functions move.** `Stmt::Yield` is statement-level: `await e;`
//! and `let v = await e;` become yields directly, and an `await` in
//! expression position (`f(await a, await b)`, `if (await x)`, `return
//! await e`) is first hoisted to statement level by the A-normal lift
//! (`desugar_async_sm_rewrite` + `desugar_async_sm_lift`, D1b). A
//! function holding an await the lift does not take (a guarded operand,
//! a loop head) keeps today's lowering verbatim — mixed programs are no
//! worse than today's, because each moved function is CORRECT, and the
//! check is conservative in the safe direction (an await this pass
//! cannot place blocks the move rather than being silently dropped).

use super::{Ast, Expr, ExprId, GEN_RECV_PARAM, Param, Stmt};

pub(super) use super::desugar_async_sm_rewrite::rewrite_body;

/// Prefix of the generator holding a moved async function's body. The
/// wrapper keeps the user's name so every call site is untouched.
const GEN_PREFIX: &str = "__async_gen__";

/// The driver, injected once when at least one function moves.
///
/// Every name the driver binds is `__`-spelled. The receiver-rule
/// census in `fnexpr_this_default` is name-keyed across the whole
/// program (deliberately coarse): a user's `var r = f()` stopped being
/// a certain promise binding the moment the driver also declared an
/// `r`, and every fn-expression handler on it became an unclaimed
/// `this` (`fnexpr-this-var-promise-recv-001`). Injected text must
/// not share a namespace with the program it is injected into.
///
/// `__torajs_await_resolve` is §27.7.5.3 Await step 2 —
/// PromiseResolve(%Promise%, v) through the intrinsic. `Promise.resolve`
/// spelled in the driver honoured a user patch or accessor on the
/// static and was counted by programs that instrument it.
///
/// `__async_step` is deliberately a TOP-LEVEL function taking the
/// generator as a parameter rather than a self-recursive nested one
/// closing over it: a self-recursive nested function that merely
/// MENTIONS a captured generator object segfaults at exit today
/// (probe: recursion never has to happen — the recursive call site
/// alone is enough; a plain class instance, array, string or promise
/// in the same position is fine). That defect is filed separately;
/// this shape does not depend on it being fixed.
const DRIVER_SRC: &str = r#"
function __async_step(__g: any, __resolve: any, __reject: any, __v: any, __is_err: boolean): void {
  let __r: any;
  try {
    __r = __is_err ? __g.throw(__v) : __g.next(__v);
  } catch (__e: any) {
    __reject(__e);
    return;
  }
  if (__r.done) {
    __resolve(__r.value);
    return;
  }
  const __w: Promise<any> = __torajs_await_resolve(__r.value);
  __w.then(
    (__x: any) => { __async_step(__g, __resolve, __reject, __x, false); return 0; },
    (__e: any) => { __async_step(__g, __resolve, __reject, __e, true); return 0; },
  );
}

function __async_drive(__mk: any): Promise<any> {
  return new Promise((__resolve: any, __reject: any) => {
    let __g: any;
    try {
      __g = __mk();
    } catch (__e: any) {
      __reject(__e);
      return;
    }
    __async_step(__g, __resolve, __reject, undefined, false);
  });
}
"#;

pub fn desugar_async_state_machine(ast: &mut Ast) {
    if ast.async_fns.is_empty() && ast.async_fn_value_exprs.is_empty() {
        return;
    }
    // Snapshot by index so `ast.stmts` can be rebuilt in place. Async
    // GENERATORS are excluded: their bodies carry real `yield`s beside
    // the await reads, and telling the two apart in one machine is
    // blade 4.
    let targets: Vec<usize> = ast
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl {
                name,
                is_generator: false,
                ..
            } if ast.async_fns.contains(name)
                && !ast.async_generator_fns.contains(name)
                && !ast.async_ssa.fns.contains(name) =>
            {
                Some(i)
            }
            _ => None,
        })
        .collect();

    let mut generators: Vec<Stmt> = Vec::new();
    for idx in targets {
        let Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type,
            body,
            span,
            ..
        } = ast.stmts[idx].clone()
        else {
            continue;
        };
        if super::desugar_async_sm_gate::blocks_move(ast, &params, &body) {
            continue;
        }
        let mut moved = body.clone();
        if !rewrite_body(ast, &mut moved) {
            continue;
        }
        // A body reading its `this` takes it as the generator's leading
        // parameter, handed in by the wrapper's own `this` — bound by
        // `bind_this_param` as the unmoved body's would have been (a
        // `.call` receiver, the sloppy global object, strict
        // `undefined`); `desugar_async_sm_this` says why it cannot stay
        // `Expr::This`.
        let reads_this = super::desugar_async_sm_this::reads_this(ast, &moved);
        let gen_params = if reads_this {
            super::desugar_async_sm_this::rewrite_this(ast, &moved, GEN_RECV_PARAM);
            recv_generator_params(&params, GEN_RECV_PARAM)
        } else {
            generator_params(&params)
        };
        let gen_name = format!("{GEN_PREFIX}{name}");
        generators.push(Stmt::FnDecl {
            name: gen_name.clone(),
            type_params: type_params.clone(),
            params: gen_params,
            // P10.7 default-Any generator: yields and the return value
            // both ride the Any tier, which is what the driver moves.
            return_type: Some("any".into()),
            body: moved,
            is_generator: true,
            span,
        });
        let wrapper_body = if reads_this {
            let gen_ident = ast.add_expr(Expr::Ident(gen_name.clone()));
            let this_eid = ast.add_expr(Expr::This);
            recv_wrapper_body(ast, gen_ident, &params, this_eid)
        } else {
            vec![Stmt::Return(Some(build_drive_call(
                ast, &gen_name, &params,
            )))]
        };
        let wrapper_params = wrapper_params(ast, &params);
        ast.stmts[idx] = Stmt::FnDecl {
            name: name.clone(),
            type_params,
            params: wrapper_params,
            return_type: Some(promise_ann(return_type.as_deref())),
            body: wrapper_body,
            is_generator: false,
            // The user wrote this declaration; `toString` answers their
            // source text, not the wrapper's.
            span,
        };
        // The name STAYS in `async_fns` — a dozen readers answer "is
        // this an async function" off that set (the fnexpr-this
        // receiver rule, any-widening, the fn value's async flavour
        // bit, the §15.8.4 param-throw posture), and every one of
        // those answers is still true of the wrapper. Only
        // `desugar_async` must skip it, or the wrapper's `return
        // __async_drive(...)` would get a second `Promise.resolve`
        // wrap; that is what this set is for. (The first landing
        // removed the name instead and lost all of those readers at
        // once — three of rotation 640's 19 blockers were that.)
        ast.async_sm_fns.insert(name.clone());
    }

    // Blade 3 — async methods, before `desugar_classes` turns them into
    // `__cm_` FnDecls.
    let moved_methods = super::desugar_async_sm_methods::rewrite_async_methods(ast);
    // Blade 2 — the value shapes, before the hoist that lifts the
    // generator expressions this mints.
    let moved_values = super::desugar_async_sm_values::rewrite_async_fn_values(ast);
    if generators.is_empty() && !moved_methods && !moved_values {
        return;
    }
    ast.stmts.extend(generators);
    inject_driver(ast);
}

/// The wrapper's return annotation. An async function's declared type
/// is already `Promise<T>` (or bare `T`, which `desugar_async` used to
/// wrap); the driver hands back `Promise<any>` either way, so the
/// wrapper says so and the Any tier carries T.
fn promise_ann(_declared: Option<&str>) -> String {
    "Promise<any>".into()
}

/// `__async_drive(() => __async_gen__f(a, b, rest))`.
///
/// The generator is constructed INSIDE the driver's `try`: calling it
/// runs FunctionDeclarationInstantiation, which is where a parameter
/// default's initializer throws, and §15.8.4 step 3 wants that throw
/// as a rejection of the returned promise, not a synchronous one from
/// the wrapper. The thunk carries the wrapper's parameters in; the
/// wrapper's rest parameter arrives already packed and the generator
/// takes it as a plain array (`generator_params`).
fn build_drive_call(ast: &mut Ast, gen_name: &str, params: &[Param]) -> ExprId {
    let gen_ident = ast.add_expr(Expr::Ident(gen_name.into()));
    build_drive_call_to(ast, gen_ident, params)
}

/// The same call with the generator given as an expression — the
/// method rewrite hands its receiver-first parameter list and the
/// generator's name; the value rewrite hands its generator
/// EXPRESSION's slot (which the hoist later turns into a name or a
/// wrapper closure).
pub(super) fn build_drive_call_to(ast: &mut Ast, gen_ident: ExprId, params: &[Param]) -> ExprId {
    let args: Vec<ExprId> = params
        .iter()
        .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
        .collect();
    let gen_call = ast.add_expr(Expr::Call {
        callee: gen_ident,
        args,
    });
    let thunk = ast.add_expr(Expr::ArrowFn {
        params: Vec::new(),
        return_type: Some("any".into()),
        body: vec![Stmt::Return(Some(gen_call))],
    });
    let drive_ident = ast.add_expr(Expr::Ident("__async_drive".into()));
    ast.add_expr(Expr::Call {
        callee: drive_ident,
        args: vec![thunk],
    })
}

/// The generator's parameter list for a body whose `this` became a
/// read of the leading receiver parameter (`desugar_async_sm_this::
/// rewrite_this`): `<recv>: any` in front of [`generator_params`]
/// (`__genrecv` for a declaration or a method; a value's rides the
/// hoist and is named there).
/// `any` for every receiver — a static method's `this` is the class
/// object, and an instance receiver typed as its class made every
/// moved instance method SIGSEGV at run time (the parser's generator
/// methods type it by the class only when the body reads it, and that
/// path carries more than this pass mirrors). The body's field reads
/// take the any lane.
pub(super) fn recv_generator_params(params: &[Param], recv: &str) -> Vec<Param> {
    let mut out = vec![Param {
        name: recv.into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    }];
    out.extend(generator_params(params));
    out
}

/// The wrapper body that hands its own `this` to a receiver-taking
/// generator: `const __async_recv: any = <this>; return
/// __async_drive(() => gen(__async_recv, params))`. The receiver is
/// read where the user's function stood — by the wrapper's `this`,
/// which `desugar_classes` / `bind_this_param` / `rewrite_toplevel_
/// this` then bind exactly as they would have bound the unmoved
/// body's — and rides in front of the parameters. The caller mints
/// `this_eid` (a static method also records it in
/// `static_this_sites`).
pub(super) fn recv_wrapper_body(
    ast: &mut Ast,
    gen_ident: ExprId,
    params: &[Param],
    this_eid: ExprId,
) -> Vec<Stmt> {
    let mut recv_first: Vec<Param> = vec![Param {
        name: "__async_recv".into(),
        type_ann: None,
        default: None,
        is_rest: false,
    }];
    recv_first.extend(params.iter().cloned());
    let drive = build_drive_call_to(ast, gen_ident, &recv_first);
    vec![
        Stmt::LetDecl {
            mutable: false,
            name: "__async_recv".into(),
            type_ann: Some("any".into()),
            init: this_eid,
            is_var: false,
        },
        Stmt::Return(Some(drive)),
    ]
}

/// The wrapper's parameter list: the user's, with every defaulted
/// parameter turned into an `any` OPTIONAL — `x = init` becomes
/// `x?: any` — so the initializer runs once, in the generator, when the
/// wrapper forwards the absent argument as `undefined` (§10.2.11: an
/// `undefined` argument takes the default; the lowering's or_default
/// lane binds it for an `any` actual). Left on the wrapper it would run
/// at the wrapper's entry too — twice per call, and a throwing one
/// would escape synchronously. Only `any`-spelled defaults get here
/// (`typed_default_param` gates the rest); rest parameters are kept as
/// written — the wrapper packs, the generator takes the array.
pub(super) fn wrapper_params(ast: &mut Ast, params: &[Param]) -> Vec<Param> {
    params
        .iter()
        .map(|p| {
            let Some(d) = p.default else {
                return p.clone();
            };
            // An optional parameter already IS the shape the wrapper
            // wants; only its implicit default is minted afresh (two
            // declarations must not share one arena node).
            if matches!(ast.get_expr(d), Expr::Ident(n)
                if n == "undefined" || n.starts_with(crate::ast::UNDEF_SLOT_MARKER))
            {
                let type_ann = p.type_ann.clone();
                let default = crate::parser::param_optional_default::implicit_optional_default(
                    ast,
                    type_ann.as_deref(),
                );
                return Param {
                    name: p.name.clone(),
                    type_ann,
                    default: Some(default),
                    is_rest: p.is_rest,
                };
            }
            // A boxed scalar keeps its spelling as `n?: number` — the
            // slot is a box either way, and the annotation still gates
            // the caller. Anything else the gate let through is `any`.
            let type_ann = match p.type_ann.as_deref() {
                Some(a @ ("number" | "boolean")) => format!("__nullable({a})"),
                Some(a @ ("__nullable(number)" | "__nullable(boolean)")) => a.to_string(),
                _ => "any".to_string(),
            };
            let default = crate::parser::param_optional_default::implicit_optional_default(
                ast,
                Some(&type_ann),
            );
            Param {
                name: p.name.clone(),
                type_ann: Some(type_ann),
                default: Some(default),
                is_rest: p.is_rest,
            }
        })
        .collect()
}

/// The generator's parameter list: the user's, with a rest parameter
/// demoted to a plain array parameter. The wrapper's own rest already
/// packed the caller's arguments; the thunk hands that array over as
/// one argument, and a rest on the generator would pack it again.
pub(super) fn generator_params(params: &[Param]) -> Vec<Param> {
    params
        .iter()
        .map(|p| Param {
            name: p.name.clone(),
            type_ann: p.type_ann.clone(),
            default: p.default,
            is_rest: false,
        })
        .collect()
}

/// Parse the driver into the shared arena once. `parse_into` keeps
/// ExprId numbering continuous, so nothing needs remapping.
fn inject_driver(ast: &mut Ast) {
    if ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == "__async_drive"))
    {
        return;
    }
    let Ok(tokens) = crate::lexer::tokenize(DRIVER_SRC) else {
        return;
    };
    let expr_base = ast.exprs.len();
    let type_ann_base = ast.type_ann_spans.len();
    let Ok(offset) = crate::parser::parse_into(DRIVER_SRC, &tokens, ast) else {
        return;
    };
    // The driver's `__w.then(...)` is §27.7.5.3 Await = PerformPromiseThen,
    // never an Invoke of a user-patched `then` (rotation 652). Its
    // receiver Ident joins `synth_promise_static_calls` — the same
    // "machinery's own §27.7 operation" mark the settle calls carry —
    // so the typed chain lowering emits no own-`then` probe there. By
    // eid, not by name: `__w` went through the lexer, so the
    // source-dunder judge would call it the program's.
    for i in expr_base..ast.exprs.len() {
        if let Expr::Call { callee, .. } = &ast.exprs[i]
            && let Expr::Member { obj, name } = ast.get_expr(*callee)
            && name == "then"
            && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "__w")
        {
            ast.synth_promise_static_calls.insert(*obj);
        }
    }
    // The driver's type annotations record spans too, and those index
    // DRIVER_SRC as well: an `: any` ~620 bytes into the driver sliced
    // the user's file at that offset — inside an em-dash in a comment
    // of `array-from-async-001` ("end byte index 621 is not a char
    // boundary"). Same sentinel as the expression spans below.
    for sp in ast.type_ann_spans.iter_mut().skip(type_ann_base) {
        *sp = crate::lexer::Span { start: 0, end: 0 };
    }
    // Same hazard on the per-expression tables: an arrow's span rides
    // `expr_spans` until `lift_arrow_fns` turns it into a `__closure_N`
    // declaration, and the driver's three arrows would carry
    // DRIVER_SRC offsets into a slice of the user's file.
    let sentinel = crate::lexer::Span { start: 0, end: 0 };
    for sp in ast.expr_spans.iter_mut().skip(expr_base) {
        *sp = sentinel;
    }
    // The driver's spans index DRIVER_SRC, but every span consumer
    // downstream slices the user's `ast.source` — out of bounds when
    // that file is shorter, silently wrong `toString` text otherwise.
    // Same hazard, same sentinel as an injected module's decls.
    for s in &mut ast.stmts[offset..] {
        crate::modules::splice::clear_injected_spans(s);
    }
}

/// `Some(inner)` when `eid` IS a parser-minted `await inner` read.
pub(super) fn await_inner(ast: &Ast, eid: ExprId) -> Option<ExprId> {
    if !ast.await_value_reads.contains(&eid) {
        return None;
    }
    match ast.get_expr(eid) {
        Expr::Member { obj, name } if name == "value" => Some(*obj),
        _ => None,
    }
}

pub(crate) fn stmt_has_await(ast: &Ast, s: &Stmt) -> bool {
    let mut found = false;
    super::desugar_async_sm_walk::visit_stmt_exprs(s, &mut |e| {
        found = found || expr_has_await(ast, e);
    });
    found
}

/// True when `eid`'s subtree holds any await read.
pub(super) fn expr_has_await(ast: &Ast, eid: ExprId) -> bool {
    if ast.await_value_reads.contains(&eid) {
        return true;
    }
    let mut found = false;
    super::desugar_async_sm_walk::visit_expr_children(ast, eid, &mut |c| {
        found = found || expr_has_await(ast, c);
    });
    found
}
