//! The iterator-protocol helpers a state machine's `for…of` /
//! `for await` rewrite calls (`desugar_generators_forof` +
//! `_forof_close`), parsed from TS source and injected once per
//! program whose machines hold such a loop (RFC 20260915 blade 4).
//!
//! `__agen_step_call` is the `for await` step: a `next()` answering a
//! thenable is an ASYNC iterator's and passes through (the driver
//! adopts it); a plain result is a SYNC iterator's and takes the
//! Async-from-Sync posture (§27.1.4.4 AsyncFromSyncIteratorContinuation
//! — the element is awaited, the step lands one tick later). A throw
//! from an async iterator's `next()` is the body's own throw
//! (§27.6.3.9 `? Call(next)`) and escapes synchronously; a sync one's
//! is caught by the wrapper (§27.1.4.4 step 5, IfAbruptRejectPromise)
//! and answers a rejected promise. Which posture applies is the
//! `@@asyncIterator` probe the protocol took the iterator from.
//!
//! `__gfor_close` is §7.4.9 IteratorClose and `__gfor_close_async` +
//! `__gfor_close_result` are §7.4.11 AsyncIteratorClose, both on the
//! loop's completion kind: on a THROW completion the `return()` call's
//! own throw is ignored and the original completion wins (step 5 /
//! step 4), on any other completion `return()`'s throw propagates and
//! a non-object result is a TypeError. The async close answers the
//! promise the machine awaits (`null` when the iterator has no
//! `return` — no Await happens then, §7.4.11 step 3); the result check
//! runs on the awaited value.
//!
//! The `return` method is read as `__it.return` off the helper's own
//! `__`-spelled parameter: the arguments-object pass runs after this
//! injection and refuses an object-literal method's boxed-only argv
//! admission on any member spelling of its name, EXCEPT the runtime
//! protocol's `next` / `return` / `throw` off a compiler-minted
//! binding (`arguments_object_objlit_argv` — the `gen-dstr-rtrn-close-
//! args-001` iterator's `return: function () { args = arguments; … }`
//! lost its `arguments` before that exemption). An index spelling
//! parses to the same `Member`; `Reflect.get` collided with the
//! by-name method routing of a user class declaring `get`.

use super::{Ast, Stmt};

const FOROF_SRC: &str = r#"
function __agen_step_call(__it: any, __am: any): any {
  let __r: any;
  let __d: any;
  let __v0: any;
  if (__am != null) {
    __r = __it.next();
  } else {
    try { __r = __it.next(); } catch (__e0: any) { return Promise.reject(__e0); }
  }
  try {
    if (__r != null && typeof __r.then === "function") { return __r; }
    __d = __r.done;
    __v0 = __r.value;
  } catch (__e: any) { return Promise.reject(__e); }
  return Promise.resolve(__v0).then((__v: any) => { return { value: __v, done: __d }; });
}
function __gfor_close(__it: any, __threw: boolean): void {
  if (__threw) {
    try {
      const __ret0: any = __it.return;
      if (__ret0 != null) { __ret0.call(__it); }
    } catch (__e: any) { }
    return;
  }
  const __ret: any = __it.return;
  if (__ret == null) { return; }
  const __res: any = __ret.call(__it);
  if (__res == null || (typeof __res !== "object" && typeof __res !== "function")) { throw new TypeError("iterator result is not an object"); }
}
function __gfor_close_async(__it: any, __am: any, __threw: boolean): any {
  let __ret: any;
  let __r0: any;
  if (__threw) {
    try {
      __ret = __it.return;
      if (__ret == null) { return null; }
      __r0 = __ret.call(__it);
    } catch (__e: any) { return null; }
  } else {
    __ret = __it.return;
    if (__ret == null) { return null; }
    __r0 = __ret.call(__it);
  }
  if (__am != null) { return __r0; }
  if (__r0 == null || (typeof __r0 !== "object" && typeof __r0 !== "function")) { return Promise.reject(new TypeError("iterator result is not an object")); }
  let __d: any;
  let __v: any;
  try { __d = __r0.done; __v = __r0.value; } catch (__e1: any) { return Promise.reject(__e1); }
  return Promise.resolve(__v).then((__x: any) => { return { value: __x, done: __d }; });
}
function __gfor_close_result(__r: any): void {
  if (__r == null || (typeof __r !== "object" && typeof __r !== "function")) { throw new TypeError("iterator result is not an object"); }
}
"#;

/// The functions [`FOROF_SRC`] defines. Each reads `next` / `return` /
/// `then` off the value it was handed and calls what it read with that
/// same value as the receiver, so none moves a method onto a receiver
/// it was not read from.
pub(crate) const HELPERS: [&str; 4] = [
    "__agen_step_call",
    "__gfor_close",
    "__gfor_close_async",
    "__gfor_close_result",
];

/// Put [`FOROF_SRC`] in the tree when some machine body holds a loop
/// the protocol rewrite can reach — by the time `desugar_generators`
/// calls this, every machine (an async function's, an async
/// generator's, a generator expression's, a nested generator's) is a
/// top-level `function*` declaration. A program whose machines hold no
/// such loop never calls the helpers, so it does not get them. The
/// injected functions are synchronous: no pass that already ran needed
/// to see them.
pub(super) fn inject_for_of_protocol_helpers(ast: &mut Ast) {
    let needs = ast.stmts.iter().any(|s| match s {
        Stmt::FnDecl {
            is_generator: true,
            body,
            ..
        } => body.iter().any(|b| holds_protocol_loop(ast, b)),
        _ => false,
    });
    if !needs
        || ast
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == "__gfor_close"))
    {
        return;
    }
    let Ok(tokens) = crate::lexer::tokenize(FOROF_SRC) else {
        return;
    };
    let expr_base = ast.exprs.len();
    let type_ann_base = ast.type_ann_spans.len();
    let Ok(offset) = crate::parser::parse_into(FOROF_SRC, &tokens, ast) else {
        return;
    };
    // Injected spans index FOROF_SRC, not the user's file (the D1
    // driver's hazard, same sentinel).
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

/// Can `s` hold a loop that `desugar_generators_forof::rewrite_in_stmt`
/// turns into the manual protocol, the only shape that calls the
/// helpers (`build_step_decl` → `__agen_step_call`, `wrap_with_close` →
/// the three close helpers)? That rewrite takes a for-of (not a for-in)
/// whose body yields or that is a `for await`, and every
/// `ForOfSplitIter`. Yield-bearing is NOT tested here: an async
/// generator's awaits become yields only in
/// `desugar_generators_async::prepare`, after this injection, so any
/// for-of counts. An async generator's `yield*` is excluded — it takes
/// the delegation protocol (`desugar_generators_yieldstar`), which
/// calls none of these. Nested statements are all walked; nested
/// function declarations are not, for by now every nested machine is a
/// top-level declaration of its own, so the answer covers every loop
/// the rewrite can visit.
fn holds_protocol_loop(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::ForOfSplitIter { .. } => return true,
        Stmt::ForOf {
            var_name,
            forin_obj: None,
            is_await,
            ..
        } if !(*is_await && ast.yieldstar_async_vars.contains(var_name)) => return true,
        _ => {}
    }
    super::desugar_with::walk::stmt_children_ref(s)
        .into_iter()
        .any(|c| holds_protocol_loop(ast, c))
}
