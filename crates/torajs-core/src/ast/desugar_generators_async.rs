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
//! a `done` step awaits its return value. The driver answers a fresh
//! step built by the machine's own `__yielded` / `__returned` — the
//! awaited value in the class's step type (a struct a typed consumer
//! can read; assigning into the machine's step through `any` is
//! refused) — and every consumer of an async iterator reads it through
//! the any lane. The driver is an ordinary
//! async function with statement-level awaits, so D1 moves it into its
//! own machine — it is injected here and `desugar_async_state_machine`
//! is run again for it before the generators are collected.
//!
//! Requests queue (§27.6.3.3 AsyncGeneratorEnqueue): the iterator's
//! `next` / `return` / `throw` chain on a per-instance promise
//! (`__q`), so a second `next()` before the first settles waits for it
//! rather than resuming a suspended machine. An idle generator (no
//! request in flight — `__q` undefined) runs the request SYNCHRONOUSLY
//! up to its first await, as AsyncGeneratorResumeNext does: the body's
//! first statements and the yield's own Await land in the same tick
//! as the call, which is the tick count every ordering test observes.
//! The tail resets itself to undefined when it settles unchained. The machine's own step
//! methods keep their bodies under `__step` / `__step_return` /
//! `__step_throw`; the public trio forwards to the queue.
//!
//! A yield-bearing `for await` in the body — which is what `yield*`
//! desugars to — awaits each inner step through an await mark, with
//! `__agen_step_call` giving a sync iterator the Async-from-Sync
//! posture (a throwing `next()` answers a rejected promise, so the
//! throw lands at the await, one tick later); `desugar_generators_
//! forof::build_manual_protocol`. A `for await` without a yield in its
//! body still runs the lowering's sync await inside the machine.

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
/// The per-instance request queue tail (a promise, or undefined
/// before the first request).
pub(super) const QUEUE_FIELD: &str = "__q";

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
async function __agen_drive(__g: any, __kind: number, __v: any): Promise<any> {
  let __r: any = __kind === 0 ? __g.__step(__v) : __kind === 1 ? __g.__step_return(__v) : __g.__step_throw(__v);
  let __ok: boolean = true;
  let __s: any = undefined;
  while (true) {
    __ok = true;
    if (__r.done) {
      try { __s = await __r.value; } catch (__e1: any) { __ok = false; __s = __e1; }
      if (!__ok) { throw __s; }
      return __g.__returned(__s);
    }
    if (__r.value instanceof __AwaitMark) {
      try { __s = await __r.value.v; } catch (__e2: any) { __ok = false; __s = __e2; }
      __r = __ok ? __g.__step(__s) : __g.__step_throw(__s);
      continue;
    }
    try { __s = await __r.value; } catch (__e3: any) { __ok = false; __s = __e3; }
    if (!__ok) { __r = __g.__step_throw(__s); continue; }
    return __g.__yielded(__s);
  }
}
function __agen_step_call(__it: any): any {
  try { return __it.next(); } catch (__e: any) { return Promise.reject(__e); }
}
async function __agen_chain(__q: any, __g: any, __kind: number, __v: any): Promise<any> {
  await __q;
  const __r: any = await __agen_drive(__g, __kind, __v);
  return __r;
}
function __agen_enqueue(__g: any, __kind: number, __v: any): Promise<any> {
  const __p: Promise<any> = __g.__q === undefined
    ? __agen_drive(__g, __kind, __v)
    : __agen_chain(__g.__q, __g, __kind, __v);
  let __t: any = undefined;
  const __done = (__x: any) => { if (__g.__q === __t) { __g.__q = undefined; } return 0; };
  __t = __p.then(__done, __done);
  __g.__q = __t;
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

/// `desugar_generators`' first step: inject the driver, mark every
/// async generator body's awaits, and move the driver into its own
/// machine — all before the generators are collected, so the driver's
/// generator is desugared in the same loop. Answers whether anything
/// happened.
pub(super) fn prepare(ast: &mut Ast) -> bool {
    let targets = async_gen_indices(ast);
    if targets.is_empty() {
        return false;
    }
    inject_agen_driver(ast);
    for idx in targets {
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[idx] else {
            continue;
        };
        let mut marked = std::mem::take(body);
        // An await the lift does not take leaves the body as it was —
        // its awaits stay settled-slot reads, the yields still ride the
        // driver.
        let ok = super::desugar_async_sm_rewrite::rewrite_body_marked(ast, &mut marked);
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[idx] else {
            unreachable!()
        };
        *body = marked;
        let _ = ok;
    }
    // The injected driver is an async declaration with statement-level
    // awaits: D1 moves it now, so `collect_generator_fn_decls` sees
    // its generator.
    super::desugar_async_sm::desugar_async_state_machine(ast);
    true
}

fn inject_agen_driver(ast: &mut Ast) {
    if ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == "__agen_drive"))
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
    let mut ms = forwarders(ast);
    ms.push(step_ctor(ast, MACHINE_YIELDED, step_ann, false));
    ms.push(step_ctor(ast, MACHINE_RETURNED, step_ann, true));
    ms
}

/// `__yielded(__v: any): __step_g { return { value: __v, done: false } }`
/// (and `__returned`, `done: true`) — the step literal typed by the
/// method's declared return, exactly as the machine's own arms build it.
fn step_ctor(ast: &mut Ast, name: &str, step_ann: &str, done: bool) -> ClassMethod {
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
        body: vec![Stmt::Return(Some(lit))],
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
