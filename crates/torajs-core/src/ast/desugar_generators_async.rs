//! RFC 20260805 D4 — an async generator's steps suspend.
//!
//! `async function*` used to compile to the sync machine plus a
//! `Promise.resolve(step)` wrap on each step method: the body's awaits
//! were settled-slot reads that never suspended, and `yield v` did not
//! await `v` (§27.6.3.8 AsyncGeneratorYield does), so a yielded
//! rejected promise came out as a value and every step settled a tick
//! early (test262 `yield-promise-reject-next`, `expression-await-
//! thenable-as-yield-operand`; the fixture `async-fn-value-expr-001`
//! once its awaiter moved).
//!
//! The textbook shape is the D1 driver applied per step. The body's
//! awaits become yields of an [`__AwaitMark`] (the A-normal lift in
//! mark mode), so the sync machine has ONE suspension kind — yield —
//! and the driver tells the two apart by the marker: an await mark is
//! settled and fed back with `__step(v)` (or `__step_throw(e)` on
//! rejection, which is the throw landing inside the body at the await
//! point); a real yield's operand is awaited and the step completes
//! (a rejection is thrown into the body at the yield point, §27.6.3.8
//! step 5, which is what completes a generator whose `yield` rejected);
//! a `done` step carries its return value already awaited (`prepare`
//! turns the body's `return e` into `return await e`, §27.6.3.2, so
//! the Await sits inside the body's own try regions). The driver answers a fresh
//! step built by the machine's own `__yielded` / `__returned` — the
//! awaited value in the class's step type (a struct a typed consumer
//! can read; assigning into the machine's step through `any` is
//! refused) — and every consumer of an async iterator reads it through
//! the any lane. The driver is an ordinary
//! async function with statement-level awaits, so D1 moves it into its
//! own machine: `inject_async_generator_driver` puts it in the tree
//! BEFORE the pipeline's one `desugar_async_state_machine` run. Running
//! that pass a second time from inside `desugar_generators` was wrong:
//! by then `lift_nested_generators` had emptied the nested generator
//! declarations out of async function values, blade 2's gate
//! (`declares_fn_or_class`) let the value through, and the generator
//! expression it minted was never hoisted (`hoist_gen_fn_exprs` had
//! run) — its `yield` reached the checker (`Array/fromAsync`'s
//! `asyncTest(async function () { async function* g() {…} … })`).
//!
//! Requests queue (§27.6.3.3 AsyncGeneratorEnqueue): the iterator's
//! `next` / `return` / `throw` chain on a per-instance promise
//! (`__q`), so a second `next()` before the first settles waits for it
//! rather than resuming a suspended machine. An idle generator (no
//! request in flight — `__q` undefined) runs the request SYNCHRONOUSLY
//! up to its first await, as AsyncGeneratorResumeNext does: the body's
//! first statements and the yield's own Await land in the same tick
//! as the call, which is the tick count every ordering test observes.
//! The tail is installed BEFORE that synchronous run (a gate promise
//! the drive's settlement opens): the generator is in the `executing`
//! state from the call to its first await, and a request the body
//! makes of its own iterator in that window is queued (§27.6.3.3),
//! not run — running it re-entered `__step`, whose entry had already
//! stamped the dead sentinel, and answered `done: true`
//! (`request-queue-order-state-executing`).
//! The tail resets itself to undefined when it settles unchained. The machine's own step
//! methods keep their bodies under `__step` / `__step_return` /
//! `__step_throw`; the public trio forwards to the queue.
//!
//! A yield-bearing `for await` in the body — which is what `yield*`
//! desugars to — awaits each inner step through an await mark
//! (`desugar_generators_forof::build_manual_protocol`), the step coming
//! from `__agen_step_call` (`desugar_generators_forof_helpers`, shared
//! with the async-function machines): a `next()` that answers a thenable is an
//! ASYNC iterator's and passes through (the driver adopts it); a plain
//! result is a SYNC iterator's and takes the Async-from-Sync posture
//! (§27.1.4.4 AsyncFromSyncIteratorContinuation) — `PromiseResolve
//! (value).then(v => { value: v, done })`, so the element is awaited (a
//! Promise-valued one unwraps) and the step lands one tick later, the
//! tick count the spec's wrapper gives. A throw from an ASYNC iterator's
//! `next()` is the body's own throw (§27.6.3.9 `? Call(next)`): it
//! escapes the helper synchronously, completes the machine at the
//! yield (the operand is evaluated before the resume label is stored)
//! and rejects the step in the same job. A SYNC iterator's `next()`
//! throw is caught by the Async-from-Sync wrapper (§27.1.4.4 step 5,
//! IfAbruptRejectPromise) and answers a rejected promise — one tick
//! later, as bun lands it. Which posture applies is the second
//! argument: the `@@asyncIterator` probe the protocol took the
//! iterator from (non-null ⇒ async). A throw from a `then` / `done` / `value`
//! getter answers a rejected promise instead, so it lands at the await
//! through the driver's `__step_throw` — the spec reads `then` inside
//! Await's own promise resolution and `done` / `value` after that Await,
//! one tick after the call either way. The verdict is taken from
//! the result because a symbol-keyed read cannot see a class's folded
//! `@@asyncIterator` member. A `for await` without a yield in
//! its body still runs the lowering's own async drive inside the
//! machine.

use super::{Ast, ClassMethod, Expr, ExprId, Param, Stmt, Visibility};

pub(super) const AWAIT_MARK_CLASS: &str = "__AwaitMark";
pub(super) const MACHINE_NEXT: &str = "__step";
pub(super) const MACHINE_RETURN: &str = "__step_return";
pub(super) const MACHINE_THROW: &str = "__step_throw";
/// The machine's own step constructors — the driver's settled value in
/// a fresh step of the CLASS's step type, so a typed consumer (`.then(
/// (r: {value: any, done: boolean}) => …)`, the typed for-await lane)
/// reads a real `__step_*` struct, not a dynobj wearing its field names.
pub(super) const MACHINE_YIELDED: &str = "__yielded";
pub(super) const MACHINE_RETURNED: &str = "__returned";
/// The per-instance request queue (§27.6.3.3 [[AsyncGeneratorQueue]]:
/// an array of `__AgenReq`, or undefined before the first request).
pub(super) const QUEUE_FIELD: &str = "__q";
/// Whether `__agen_run` is draining the queue (the generator is
/// `executing` or suspended at an await inside a request); a request
/// arriving then is queued and picked up by the running drain.
pub(super) const BUSY_FIELD: &str = "__busy";
/// Whether the body is inside a `yield*` delegation loop
/// (`desugar_generators_yieldstar`): a return(v) request then resumes
/// the delegating yield as a return completion instead of completing
/// the generator.
pub(super) const YS_ACTIVE_FIELD: &str = "__ysact";
/// Raised by `return()` for that resumption; the loop reads and clears
/// it after the delegating yield.
pub(super) const YS_RETURN_FIELD: &str = "__ysret";

/// The driver, the mark class and the queue — injected once, parsed
/// into the shared arena. Every binding is `__`-spelled (the D1 driver's
/// rationale). Each await sits directly in a `try` (a generator try
/// region), not behind a settlement `.then` — that extra reaction cost
/// every step a tick against the order the spec's Await gives.
const AGEN_SRC: &str = r#"
class __AwaitMark {
  v: any;
  constructor(v: any) { this.v = v; }
}
class __AgenReq {
  k: number;
  v: any;
  res: any;
  rej: any;
  constructor(k: number, v: any, res: any, rej: any) { this.k = k; this.v = v; this.res = res; this.rej = rej; }
}
async function __agen_run(__g: any): Promise<any> {
  __g.__busy = true;
  while (__g.__q.length > 0) {
    const __req: any = __g.__q.shift();
    let __r: any = undefined;
    let __ok: boolean = true;
    let __s: any = undefined;
    let __k: number = __req.k;
    let __v: any = __req.v;
    if (__k === 1) {
      try { __v = await __v; } catch (__e6: any) { __k = 2; __v = __e6; }
    }
    try {
      __r = __k === 0 ? __g.__step(__v) : __k === 1 ? __g.__step_return(__v) : __g.__step_throw(__v);
    } catch (__e0: any) { __ok = false; __s = __e0; }
    while (__ok) {
      if (__r.done) {
        __req.res(__g.__returned(__r.value));
        break;
      }
      if (__r.value instanceof __AwaitMark) {
        try { __s = await __r.value.v; } catch (__e2: any) { __ok = false; __s = __e2; }
        try {
          __r = __ok ? __g.__step(__s) : __g.__step_throw(__s);
          __ok = true;
        } catch (__e3: any) { __ok = false; __s = __e3; }
        continue;
      }
      try { __s = await __r.value; } catch (__e4: any) { __ok = false; __s = __e4; }
      if (__ok) { __req.res(__g.__yielded(__s)); break; }
      try {
        __r = __g.__step_throw(__s);
        __ok = true;
      } catch (__e5: any) { __ok = false; __s = __e5; }
    }
    if (!__ok) { __req.rej(__s); }
  }
  __g.__busy = false;
  return 0;
}
function __agen_delegate_call(__it: any, __fn: any, __arg: any, __kind: number, __am: any): any {
  if (__am != null) { return __fn.call(__it, __arg); }
  let __r: any;
  try { __r = __fn.call(__it, __arg); } catch (__e0: any) { return Promise.reject(__e0); }
  let __d: any;
  let __v0: any;
  try {
    if (__r != null && typeof __r.then === "function") { return __r; }
    if (__r == null || (typeof __r !== "object" && typeof __r !== "function")) { throw new TypeError("iterator result is not an object"); }
    __d = __r.done;
    __v0 = __r.value;
  } catch (__e: any) { return Promise.reject(__e); }
  return Promise.resolve(__v0).then((__v: any) => { return { value: __v, done: __d }; });
}
function __agen_enqueue(__g: any, __kind: number, __v: any): Promise<any> {
  let __res: any = undefined;
  let __rej: any = undefined;
  const __p: Promise<any> = new Promise((__a: any, __b: any) => { __res = __a; __rej = __b; });
  if (__g.__q === undefined) { __g.__q = []; }
  __g.__q.push(new __AgenReq(__kind, __v, __res, __rej));
  if (!__g.__busy) { __agen_run(__g); }
  return __p;
}
"#;

/// Which top-level `function*` declarations are async generators.
fn async_gen_indices(ast: &Ast) -> Vec<usize> {
    ast.stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl {
                name,
                is_generator: true,
                ..
            } if ast.async_generator_fns.contains(name) => Some(i),
            _ => None,
        })
        .collect()
}

/// `desugar_generators`' first step: mark every async generator body's
/// awaits (the bodies are all top-level by now — the nested ones were
/// just lifted). The driver is already in the tree and already moved
/// (`inject_async_generator_driver`). Answers whether anything happened.
pub(super) fn prepare(ast: &mut Ast) -> bool {
    let targets = async_gen_indices(ast);
    if targets.is_empty() {
        return false;
    }
    for idx in targets {
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[idx] else {
            continue;
        };
        let mut marked = std::mem::take(body);
        for s in &mut marked {
            await_returns(ast, s);
        }
        // An await the rewrite does not take stays an await read, which
        // the lowering refuses by the function's name before it lowers
        // anything (`ssa_lower_fn::suspend::stray`).
        let ok = super::desugar_async_sm_rewrite::rewrite_body_marked(ast, &mut marked);
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[idx] else {
            unreachable!()
        };
        *body = marked;
        let _ = ok;
    }
    true
}

/// §27.6.3.2 / ReturnStatement evaluation: an async generator's
/// `return e` awaits e AT THE RETURN — inside any enclosing try, so a
/// `finally` runs after the await and a rejection is a throw at the
/// return site. Each `return e` in the body (a nested function's is
/// its own; `return;` has nothing to await) becomes the parser's await
/// read of e, which the lift then suspends on like any other await.
/// The driver therefore settles a done step with its value as is.
fn await_returns(ast: &mut Ast, s: &mut Stmt) {
    match s {
        // a class's members are functions of their own
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => {}
        Stmt::Return(Some(e)) => {
            let read = ast.add_expr(Expr::Member {
                obj: *e,
                name: "value".into(),
            });
            ast.await_value_reads.insert(read);
            *e = read;
        }
        _ => {
            for c in super::desugar_with::walk::stmt_children(s) {
                await_returns(ast, c);
            }
        }
    }
}

/// Put the async generator driver (`AGEN_SRC`) in the tree when the
/// program has any async generator: a declaration or a method is on the
/// parser's roll `async_generator_fns`; an EXPRESSION is only in
/// `gen_fn_exprs` until the hoist names it (`async-fn-value-expr-001`
/// lost `__agen_enqueue` when this read the roll alone). Runs just
/// before the pipeline's `desugar_async_state_machine`, which moves the
/// driver's async declarations into their machines along with the
/// user's; `desugar_generators` then collects those like any other.
pub fn inject_async_generator_driver(ast: &mut Ast) {
    let has_async_gen = !ast.async_generator_fns.is_empty()
        || ast
            .gen_fn_exprs
            .values()
            .any(|i| i.kind == super::gen_fn_expr::GenFnExprKind::AsyncGenerator);
    if !has_async_gen
        || ast
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == "__agen_run"))
    {
        return;
    }
    let Ok(tokens) = crate::lexer::tokenize(AGEN_SRC) else {
        return;
    };
    let expr_base = ast.exprs.len();
    let type_ann_base = ast.type_ann_spans.len();
    let Ok(offset) = crate::parser::parse_into(AGEN_SRC, &tokens, ast) else {
        return;
    };
    // Injected spans index AGEN_SRC, not the user's file (the D1 driver's
    // hazard, same sentinel).
    let sentinel = crate::lexer::Span { start: 0, end: 0 };
    for sp in ast.type_ann_spans.iter_mut().skip(type_ann_base) {
        *sp = sentinel;
    }
    for sp in ast.expr_spans.iter_mut().skip(expr_base) {
        *sp = sentinel;
    }
    for s in &mut ast.stmts[offset..] {
        crate::modules::splice::clear_injected_spans(s);
    }
}

/// `new __AwaitMark(inner)` — the lift's yield operand in mark mode.
pub(super) fn mint_await_mark(ast: &mut Ast, inner: ExprId) -> ExprId {
    ast.add_expr(Expr::New {
        class_name: AWAIT_MARK_CLASS.to_string(),
        args: vec![inner],
        type_args: Vec::new(),
    })
}

/// `desugar_one_generator`'s D4 step: for an async generator, rename
/// the machine's trio to the `__step*` names, add the request-queue
/// field, and answer the public forwarders; a sync generator is left
/// as it is.
pub(super) fn shape_machine(
    ast: &mut Ast,
    is_async_gen: bool,
    step_ann: &str,
    yield_ty: &str,
    trio: (&mut ClassMethod, &mut ClassMethod, &mut ClassMethod),
    lifted_locals: &mut Vec<(String, String)>,
) -> Vec<ClassMethod> {
    if !is_async_gen {
        return Vec::new();
    }
    trio.0.name = MACHINE_NEXT.into();
    trio.1.name = MACHINE_RETURN.into();
    trio.2.name = MACHINE_THROW.into();
    lifted_locals.push((QUEUE_FIELD.into(), "any".into()));
    lifted_locals.push((BUSY_FIELD.into(), "any".into()));
    lifted_locals.push((YS_ACTIVE_FIELD.into(), "any".into()));
    lifted_locals.push((YS_RETURN_FIELD.into(), "any".into()));
    let mut ms = forwarders(ast);
    ms.push(step_ctor(ast, MACHINE_YIELDED, step_ann, yield_ty, false));
    ms.push(step_ctor(ast, MACHINE_RETURNED, step_ann, yield_ty, true));
    ms
}

/// `__yielded(__v: any): __step_g { if (__v === undefined) return {
/// value: <T's undefined>, done }; return { value: __v, done } }` (and
/// `__returned`, `done: true`) — the step literal typed by the method's
/// declared return, exactly as the machine's own arms build it. An
/// `undefined` settled value takes the yield type's own spelling of
/// undefined (`completion_undefined`): coerced into a `number` slot the
/// any box read back as NaN (`gen-async-step-typed-read-001`, a typed
/// generator's `return` with no value).
fn step_ctor(ast: &mut Ast, name: &str, step_ann: &str, yield_ty: &str, done: bool) -> ClassMethod {
    let undef_probe = ast.add_expr(Expr::Ident("__v".into()));
    let undef_lit = ast.add_expr(Expr::Ident("undefined".into()));
    let is_undef = ast.add_expr(Expr::BinOp {
        op: super::BinOp::Eq,
        left: undef_probe,
        right: undef_lit,
    });
    let typed_undef = ast.add_expr(super::desugar_generators_methods::completion_undefined(
        yield_ty,
    ));
    let d0 = ast.add_expr(Expr::Bool(done));
    let undef_step = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), typed_undef), ("done".into(), d0)],
    });
    let undef_return = Stmt::If {
        cond: is_undef,
        then_branch: Box::new(Stmt::Return(Some(undef_step))),
        else_branch: None,
    };
    let v = ast.add_expr(Expr::Ident("__v".into()));
    let d = ast.add_expr(Expr::Bool(done));
    let lit = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), v), ("done".into(), d)],
    });
    ClassMethod {
        name: name.into(),
        type_params: Vec::new(),
        params: vec![Param {
            name: "__v".into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        }],
        return_type: Some(step_ann.into()),
        body: vec![undef_return, Stmt::Return(Some(lit))],
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}

/// The public `next` / `return` / `throw` of an async generator's
/// iterator class: each hands its request to the queue. Their result
/// is `Promise<any>` — a typed promise, so `it.next().then(f, $DONE)`
/// keeps the typed then lane (the any-method lane cannot take a
/// top-level function as an argument) — of the driver's `{ value,
/// done }`, which every consumer reads through the any lane.
pub(super) fn forwarders(ast: &mut Ast) -> Vec<ClassMethod> {
    ["next", "return", "throw"]
        .iter()
        .enumerate()
        .map(|(kind, name)| {
            let this = ast.add_expr(Expr::This);
            let kind_e = ast.add_expr(Expr::Number(kind as f64));
            let arg = ast.add_expr(Expr::Ident("__v".into()));
            let callee = ast.add_expr(Expr::Ident("__agen_enqueue".into()));
            let call = ast.add_expr(Expr::Call {
                callee,
                args: vec![this, kind_e, arg],
            });
            let undef = ast.add_expr(Expr::Ident("undefined".into()));
            ClassMethod {
                name: (*name).into(),
                type_params: Vec::new(),
                params: vec![Param {
                    name: "__v".into(),
                    type_ann: Some("any".into()),
                    default: Some(undef),
                    is_rest: false,
                }],
                return_type: Some("Promise<any>".into()),
                body: vec![Stmt::Return(Some(call))],
                is_abstract: false,
                visibility: Visibility::Public,
                accessor_kind: None,
                span: crate::lexer::Span { start: 0, end: 0 },
            }
        })
        .collect()
}
