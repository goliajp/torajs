//! The helpers a `using` desugar calls, parsed from source and appended
//! to the program the first time one is needed.

use super::super::{Ast, Stmt};

/// The runtime shape of the rewrite, injected as ordinary top-level
/// fns. Note: spec passes NO message to the SuppressedError here
/// (§DisposeResources 3.b.i), leaving no own `message` property; the
/// injected ctor's checker face wants a String, so we pass "" — the
/// observable `.message` read is "" either way (Error.prototype
/// fallback), only Object.hasOwnProperty("message") differs. B6
/// (SuppressedError checker face: optional params) closes that.
/// Knife 2 adds the async pair: `_add_async` resolves the dispose
/// method with the §GetDisposeMethod async hint (`@@asyncDispose`
/// first, then a `@@dispose` sync fallback whose RESULT is not
/// awaited — the walk awaits undefined after it, spec's closure
/// wrap), stacking a `k` kind tag (2 = async method / 1 = sync
/// fallback / 0 = null-or-undefined binding, which still costs one
/// await tick); sync `_add` entries carry no `k` and never await.
/// `_dispose_async` is itself an injected `async function`, so the
/// ordinary async desugar state-machines its awaits.
/// Injected via the `parse_into` append convention (modules.rs);
/// the fns ride the full downstream pipeline like user code, and
/// `new SuppressedError(…)` here is what makes
/// `inject_builtin_classes` (which runs later) materialize that
/// class.
const HELPER_SRC: &str = r#"
function __torajs_using_add(env: any, value: any): any {
  if (value === null || value === undefined) {
    return value;
  }
  const m = value[Symbol.dispose];
  if (typeof m !== "function") {
    throw new TypeError("value is not disposable: [Symbol.dispose] is not a function");
  }
  env.stack.push({ v: value, d: m });
  return value;
}
function __torajs_using_add_async(env: any, value: any): any {
  if (value === null || value === undefined) {
    env.stack.push({ v: undefined, d: undefined, k: 0 });
    return value;
  }
  const m = value[Symbol.asyncDispose];
  if (m === undefined || m === null) {
    const sm = value[Symbol.dispose];
    if (typeof sm !== "function") {
      throw new TypeError("value is not async disposable: neither [Symbol.asyncDispose] nor [Symbol.dispose] is a function");
    }
    env.stack.push({ v: value, d: sm, k: 1 });
    return value;
  }
  if (typeof m !== "function") {
    throw new TypeError("value is not async disposable: [Symbol.asyncDispose] is not a function");
  }
  env.stack.push({ v: value, d: m, k: 2 });
  return value;
}
async function __torajs_using_dispose_async(env: any): Promise<void> {
  const st = env.stack;
  let error = env.error;
  let hasError = env.hasError;
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
function __torajs_using_pending(env: any): boolean {
  return env.stack.length > 0;
}
function __torajs_using_caught(env: any, e: any): void {
  env.error = e;
  env.hasError = true;
}
function __torajs_using_dispose(env: any): void {
  const st = env.stack;
  let error = env.error;
  let hasError = env.hasError;
  let i = st.length - 1;
  while (i >= 0) {
    const r = st[i];
    try {
      r.d.call(r.v);
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
"#;

pub(super) fn inject_helpers(ast: &mut Ast) {
    let tokens = crate::lexer::tokenize(HELPER_SRC).expect("using helpers lex");
    let offset = crate::parser::parse_into(HELPER_SRC, &tokens, ast).expect("using helpers parse");
    // The parsed spans index HELPER_SRC, not the program's source —
    // consumers slicing `ast.source` with them would read out of
    // bounds. Synthesized FnDecls carry the (0,0) "no user source"
    // sentinel (the stmt.rs span contract); stamp it here.
    for s in ast.stmts[offset..].iter_mut() {
        if let Stmt::FnDecl { span, .. } = s {
            *span = crate::lexer::Span { start: 0, end: 0 };
        }
    }
}
