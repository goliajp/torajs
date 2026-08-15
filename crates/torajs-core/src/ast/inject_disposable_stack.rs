//! `DisposableStack` / `AsyncDisposableStack` injection — Explicit
//! Resource Management builtins (RFC 20260809 B5). The classes are
//! ordinary TS source parsed via `parse_into` (the `desugar_using`
//! HELPER_SRC convention) and front-spliced so a user
//! `class X extends DisposableStack` finds its parent declared first
//! (the `desugar_classes` declaration-order requirement,
//! `inject_builtin_classes` 同款).
//!
//! Runs in the prelude right after `desugar_using` and BEFORE
//! `desugar_async` — `__torajs_adstack_walk` / `__torajs_adstack_done`
//! are `async function`s that the ordinary async desugar
//! state-machines. `inject_builtin_classes` runs later and sees the
//! `SuppressedError` / `ReferenceError` / `TypeError` idents these
//! bodies mint, so the error classes materialize exactly as if user
//! code had referenced them.
//!
//! Spec mapping (§ DisposableStack / AsyncDisposableStack):
//! - entries are `{v, d, k}` dynobj records pushed onto an any-typed
//!   array field; `k` distinguishes the call shape at dispose time
//!   (absent = `use` sync method → `d.call(v)`; 3 = `adopt` →
//!   `d(v)` bare; 4 = `defer` → `d()`; async adds 2 = `@@asyncDispose`
//!   method → awaited, 1 = `@@dispose` sync fallback whose result is
//!   NOT awaited (one catch-up tick instead), 0 = null/undefined
//!   binding costing one await tick — the `__torajs_using_add_async`
//!   tags, knife 2).
//! - dispose method resolution happens at `use()` time (the
//!   AddDisposableResource read-once contract), state flips + stack
//!   detach happen synchronously in the method body (nominal `this`;
//!   an any-boxed struct rejects property writes — knife 1 lesson),
//!   and only the detached entries array crosses into the walk
//!   helpers.
//! - `move()` answers a fresh intrinsic-class stack (spec pins
//!   %DisposableStack%, no species) and leaves the source disposed
//!   without disposing.
//! - B6 residuals (landed post store-split): BOTH dispose pairs are
//!   aliased by post-class assignments — spec pins
//!   `prototype[@@dispose]` / `prototype[@@asyncDispose]` to the
//!   same function object as `dispose` / `disposeAsync`, and each
//!   assignment overwrites the class body's wrapper entry (bun
//!   answers `===` true for both pairs — the earlier "async pair is
//!   distinct" note was a mis-probe). `@@toStringTag` is a
//!   defineProperty W0/E0/C1 entry on both prototypes. These are own
//!   writes past the 7-entry initial dense capacity — dead until RFC
//!   20260809-dynobj-store-split made resize address-stable
//!   (previously the write vanished and `.prototype` identity split).
//!
//! Both classes ride the standalone-probe parity baseline
//! (probe-dstack / probe-adstack, tr == bun byte-equal, rotation 342).

use super::{Ast, Expr, Stmt};

const SYNC_SRC: &str = r#"
function __torajs_dstack_walk(st: any): void {
  let error: any = undefined;
  let hasError = false;
  let i = st.length - 1;
  while (i >= 0) {
    const r = st[i];
    try {
      if (r.k === 3) {
        r.d(r.v);
      } else if (r.k === 4) {
        r.d();
      } else {
        r.d.call(r.v);
      }
    } catch (e) {
      if (hasError) {
        error = new SuppressedError(e, error, "");
      } else {
        error = e;
      }
      hasError = true;
    }
    i = i - 1;
  }
  if (hasError) {
    throw error;
  }
}
class DisposableStack {
  __s: any = [];
  __d: boolean = false;
  get disposed(): boolean {
    return this.__d;
  }
  use(value: any): any {
    if (this.__d) {
      throw new ReferenceError("DisposableStack already disposed");
    }
    if (value === null || value === undefined) {
      return value;
    }
    const m = value[Symbol.dispose];
    if (typeof m !== "function") {
      throw new TypeError("value is not disposable: [Symbol.dispose] is not a function");
    }
    this.__s.push({ v: value, d: m });
    return value;
  }
  adopt(value: any, onDispose: any): any {
    if (this.__d) {
      throw new ReferenceError("DisposableStack already disposed");
    }
    if (typeof onDispose !== "function") {
      throw new TypeError("onDispose is not a function");
    }
    this.__s.push({ v: value, d: onDispose, k: 3 });
    return value;
  }
  defer(onDispose: any): any {
    if (this.__d) {
      throw new ReferenceError("DisposableStack already disposed");
    }
    if (typeof onDispose !== "function") {
      throw new TypeError("onDispose is not a function");
    }
    this.__s.push({ v: undefined, d: onDispose, k: 4 });
    return undefined;
  }
  move(): DisposableStack {
    if (this.__d) {
      throw new ReferenceError("DisposableStack already disposed");
    }
    const n = new DisposableStack();
    n.__s = this.__s;
    this.__s = [];
    this.__d = true;
    return n;
  }
  dispose(): any {
    if (this.__d) {
      return undefined;
    }
    this.__d = true;
    const st: any = this.__s;
    this.__s = [];
    __torajs_dstack_walk(st);
    return undefined;
  }
  [Symbol.dispose](): any {
    return this.dispose();
  }
}
(DisposableStack.prototype as any)[Symbol.dispose] = (DisposableStack.prototype as any).dispose;
Object.defineProperty(DisposableStack.prototype, Symbol.toStringTag, { value: "DisposableStack", configurable: true });
"#;

const ASYNC_SRC: &str = r#"
async function __torajs_adstack_walk(st: any): Promise<void> {
  let error: any = undefined;
  let hasError = false;
  let i = st.length - 1;
  while (i >= 0) {
    const r = st[i];
    try {
      if (r.k === 2) {
        await r.d.call(r.v);
      } else if (r.k === 1) {
        r.d.call(r.v);
        await undefined;
      } else if (r.k === 0) {
        await undefined;
      } else if (r.k === 3) {
        await r.d(r.v);
      } else {
        await r.d();
      }
    } catch (e) {
      if (hasError) {
        error = new SuppressedError(e, error, "");
      } else {
        error = e;
      }
      hasError = true;
    }
    i = i - 1;
  }
  if (hasError) {
    throw error;
  }
}
async function __torajs_adstack_done(): Promise<void> {}
class AsyncDisposableStack {
  __s: any = [];
  __d: boolean = false;
  get disposed(): boolean {
    return this.__d;
  }
  use(value: any): any {
    if (this.__d) {
      throw new ReferenceError("AsyncDisposableStack already disposed");
    }
    if (value === null || value === undefined) {
      this.__s.push({ v: undefined, d: undefined, k: 0 });
      return value;
    }
    const m = value[Symbol.asyncDispose];
    if (m === undefined || m === null) {
      const sm = value[Symbol.dispose];
      if (typeof sm !== "function") {
        throw new TypeError("value is not async disposable: neither [Symbol.asyncDispose] nor [Symbol.dispose] is a function");
      }
      this.__s.push({ v: value, d: sm, k: 1 });
      return value;
    }
    if (typeof m !== "function") {
      throw new TypeError("value is not async disposable: [Symbol.asyncDispose] is not a function");
    }
    this.__s.push({ v: value, d: m, k: 2 });
    return value;
  }
  adopt(value: any, onDispose: any): any {
    if (this.__d) {
      throw new ReferenceError("AsyncDisposableStack already disposed");
    }
    if (typeof onDispose !== "function") {
      throw new TypeError("onDispose is not a function");
    }
    this.__s.push({ v: value, d: onDispose, k: 3 });
    return value;
  }
  defer(onDispose: any): any {
    if (this.__d) {
      throw new ReferenceError("AsyncDisposableStack already disposed");
    }
    if (typeof onDispose !== "function") {
      throw new TypeError("onDispose is not a function");
    }
    this.__s.push({ v: undefined, d: onDispose, k: 4 });
    return undefined;
  }
  move(): AsyncDisposableStack {
    if (this.__d) {
      throw new ReferenceError("AsyncDisposableStack already disposed");
    }
    const n = new AsyncDisposableStack();
    n.__s = this.__s;
    this.__s = [];
    this.__d = true;
    return n;
  }
  disposeAsync(): any {
    if (this.__d) {
      return __torajs_adstack_done();
    }
    this.__d = true;
    const st: any = this.__s;
    this.__s = [];
    return __torajs_adstack_walk(st);
  }
  [Symbol.asyncDispose](): any {
    return this.disposeAsync();
  }
}
(AsyncDisposableStack.prototype as any)[Symbol.asyncDispose] = (AsyncDisposableStack.prototype as any).disposeAsync;
Object.defineProperty(AsyncDisposableStack.prototype, Symbol.toStringTag, { value: "AsyncDisposableStack", configurable: true });
"#;

/// A program mentions the class — the `inject_builtin_classes`
/// reference shapes: bare Ident, `new <N>()`, a `.<N>` member,
/// `extends <N>`, or a `catch (e: <N>)` annotation.
fn referenced(ast: &Ast, n: &str) -> bool {
    // `extends <N>` needs no arm of its own: the heritage is an arena
    // expression (RFC 20260815), so its bare name is the `Expr::Ident`
    // the first line matches.
    ast.exprs.iter().any(|e| {
        matches!(e, Expr::Ident(x) | Expr::New { class_name: x, .. } if x == n)
            || matches!(e, Expr::Member { name, .. } if name == n)
    }) || ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::Try { catch_type: Some(t), .. } if t == n))
}

fn user_shadows(ast: &Ast, n: &str) -> bool {
    ast.stmts
        .iter()
        .any(|s| matches!(s, Stmt::ClassDecl { name, .. } if name == n))
}

pub fn inject_disposable_stack(ast: &mut Ast) {
    let want_sync = referenced(ast, "DisposableStack") && !user_shadows(ast, "DisposableStack");
    let want_async =
        referenced(ast, "AsyncDisposableStack") && !user_shadows(ast, "AsyncDisposableStack");
    if !want_sync && !want_async {
        return;
    }
    let mut src = String::new();
    if want_sync {
        src.push_str(SYNC_SRC);
    }
    if want_async {
        src.push_str(ASYNC_SRC);
    }
    let tokens = crate::lexer::tokenize(&src).expect("disposable stack lex");
    let offset = crate::parser::parse_into(&src, &tokens, ast).expect("disposable stack parse");
    // Parsed spans index the injected source, not the program's —
    // stamp the (0,0) "no user source" sentinel (the stmt.rs span
    // contract, `desugar_using::inject_helpers` 同款), on FnDecls and
    // on every class method.
    for s in ast.stmts[offset..].iter_mut() {
        match s {
            Stmt::FnDecl { span, .. } => *span = crate::lexer::Span { start: 0, end: 0 },
            Stmt::ClassDecl {
                methods,
                static_methods,
                ..
            } => {
                for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                    m.span = crate::lexer::Span { start: 0, end: 0 };
                }
            }
            _ => {}
        }
    }
    // Front-splice: `class X extends DisposableStack` needs the parent
    // declared ahead of it in statement order.
    let injected: Vec<Stmt> = ast.stmts.split_off(offset);
    ast.stmts.splice(0..0, injected);
}
