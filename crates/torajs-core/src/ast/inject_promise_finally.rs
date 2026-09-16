//! `Promise.prototype.finally` via `then` (RFC 20260915-promise-finally-via-then).
//!
//! §27.2.5.3 is `Invoke(promise, "then", «thenFinally, catchFinally»)`
//! over two closures it mints — each runs `onFinally`, resolves the
//! result through `PromiseResolve(SpeciesConstructor(promise))`, and
//! re-settles with the original outcome. The runtime's builtin
//! `finally` kernel is only what that amounts to when the receiver's
//! `then` is the builtin's; when it is not (an own `then`, a patched
//! `Promise.prototype.then`, a thenable receiver of the reified
//! `Promise.prototype.finally.call`), the spec body must run, closures
//! and all. The body is TypeScript this pass injects; the runtime's
//! Invoke lane (`torajs-anyvalue::method_call_promise_invoke`) calls
//! it through the self-hosted bind registry, the way the reified
//! `Array.fromAsync` cell reaches its helper.
//!
//! The builtin kernels reach in too: a handler that returned a promise
//! whose `then` is not the builtin's needs `inner.then(thunk)` Invoked
//! (`__promise_finally_resettle`, the second bind).
//!
//! Gated on the member name `finally` appearing anywhere (the
//! `inject_array_from_async` over-approximation): a program that
//! never spells it pays nothing, and the runtime lane it feeds is
//! only reachable through that spelling.

use super::{Ast, Stmt};

/// The helper's name — a declaration the parser binds like any other.
pub(crate) const HELPER: &str = "__promise_finally_via_then";

/// §27.2.5.3 steps 2-7 in spec order. `SpeciesConstructor` (§7.3.22)
/// and `PromiseResolve` (§27.2.4.7.1) are spelled inline, with
/// `undefined` standing for `%Promise%`: `Promise.resolve` IS
/// `PromiseResolve(%Promise%, x)`, and the intrinsic never has to be
/// named as a VALUE — a bare `Promise` in value position, like a
/// `.constructor` member read on an opaque object, is a constructor
/// escape to `builtin_proto_shadow`, which then turns every typed
/// `p.then(...)` in the program into an any-lane call (rotation 654
/// gate: `assignment to p mismatch — slot is Promise but value is
/// Any`). So SpeciesConstructor is the kernel
/// `__torajs_promise_species_ctor` (`ssa_lower_call_injected_spec_ops`),
/// whose classification answers the default for %Promise% (its
/// `@@species` getter returns %Promise% itself); a subclass with a
/// species constructor takes the `new C(executor)` route. The closures
/// are passed as literals so NamedEvaluation never names them
/// (`.name` is `""`, `.length` 1, not constructors).
const HELPER_SRC: &str = r#"
function __promise_resolve_with(__c: any, __x: any): any {
  if (__c === undefined) {
    return Promise.resolve(__x);
  }
  if (__x instanceof Promise && __x.constructor === __c) {
    return __x;
  }
  let __res: any = undefined;
  const __cap: any = new __c((__a: any, __b: any) => { __res = __a; });
  __res(__x);
  return __cap;
}

function __promise_finally_via_then(__p: any, __on_finally: any): any {
  if (__p === null || (typeof __p !== "object" && typeof __p !== "function")) {
    throw new TypeError("Promise.prototype.finally called on a non-object");
  }
  if (typeof __on_finally !== "function") {
    return __p.then(__on_finally, __on_finally);
  }
  const __c: any = __torajs_promise_species_ctor(__p);
  return __p.then(
    (__value: any): any => {
      const __result: any = __on_finally();
      return __promise_resolve_with(__c, __result).then((): any => __value);
    },
    (__reason: any): any => {
      const __result: any = __on_finally();
      return __promise_resolve_with(__c, __result).then((): any => { throw __reason; });
    });
}

function __promise_finally_resettle(__q: any, __rejected: boolean, __v: any): any {
  if (__rejected) {
    return __q.then((): any => { throw __v; });
  }
  return __q.then((): any => __v);
}
"#;

fn referenced(ast: &Ast) -> bool {
    ast.exprs
        .iter()
        .any(|e| matches!(e, super::Expr::Member { name, .. } if name == "finally"))
}

pub fn inject_promise_finally(ast: &mut Ast) {
    if !referenced(ast) {
        return;
    }
    if ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == HELPER))
    {
        return;
    }
    let id = torajs_rc::ns_static::HELPER_ID_PROMISE_FINALLY;
    let id2 = torajs_rc::ns_static::HELPER_ID_PROMISE_FINALLY_RESETTLE;
    let src = format!(
        "{HELPER_SRC}\n__torajs_ns_static_bind({id}, (__p: any, __f: any) => {HELPER}(__p, __f));\n__torajs_ns_static_bind({id2}, (__q: any, __r: any, __v: any) => __promise_finally_resettle(__q, __r, __v));\n"
    );
    let tokens = crate::lexer::tokenize(&src).expect("promise-finally helper lex");
    let expr_base = ast.exprs.len();
    let type_ann_base = ast.type_ann_spans.len();
    let offset =
        crate::parser::parse_into(&src, &tokens, ast).expect("promise-finally helper parse");
    // The parsed spans index the helper source, not the program's —
    // every consumer slices `ast.source` with them (the
    // `inject_driver` sentinel, on all three tables).
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
    // Front-splice: the bind runs before any user statement, so a
    // `finally` on the program's first line already forwards.
    let injected: Vec<Stmt> = ast.stmts.split_off(offset);
    ast.stmts.splice(0..0, injected);
}
