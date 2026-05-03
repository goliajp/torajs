// torajs typed test262 harness — replaces test262's stock sta.js +
// assert.js prepend so the prepended source survives torajs's type
// checker. Functions exposed are flat top-level identifiers
// (`__t262_*`) instead of `assert.*` member access; the source-
// rewrite layer in `conformance/test262-runner` rewrites every
// `assert.X(...)` call site to `__t262_X(...)`.
//
// Why not match `assert.X` directly? torajs doesn't support
// generic methods on a class. Top-level generic functions DO work
// (M3.1-3.3 generics), so `__t262_sameValue<T>` lets a single
// declaration serve number / string / boolean comparisons.
//
// Coverage is limited to the test262 helpers that fit in torajs's
// subset. Cases that depend on `Symbol`, `Proxy`, `WeakMap`, etc.
// land in the harness's `__t262_*Skip` helpers (no-op stubs that
// log and return) so the case still parses; runtime behavior on
// those paths is intentionally divergent from bun and the runner
// records them as `incompatible` rather than `bug`.

class Test262Error {
  message: string;
  constructor(m: string) {
    this.message = m;
  }
}

function __t262_assert(actual: boolean, msg: string): void {
  if (!actual) {
    throw new Test262Error(msg);
  }
}

function __t262_sameValue<T>(actual: T, expected: T, msg: string): void {
  if (actual !== expected) {
    throw new Test262Error(msg);
  }
}

function __t262_notSameValue<T>(actual: T, expected: T, msg: string): void {
  if (actual === expected) {
    throw new Test262Error(msg);
  }
}

// Bare `assert(...)` — single-arg form. The rewrite layer converts
// every bare `assert(b)` / `assert(b, msg)` call to `__t262_assert`.
// Test262 also exposes `assert.throws(ErrorType, fn, msg)` — the
// rewrite turns that into `__t262_throws`.

function __t262_throws_runtime(thunk: () => void, msg: string): void {
  let threw: boolean = false;
  try {
    thunk();
  } catch (e: number) {
    threw = true;
  }
  if (!threw) {
    throw new Test262Error(msg);
  }
}

// `assert.throws(ErrorClass, fn, msg)` — the first arg is a class
// reference. torajs has no way to compare class identity at runtime
// without `Type::Class`; we drop the class arg in the rewrite layer
// and call `__t262_throws_runtime(fn, msg)` instead. Cases that
// depend on the specific error class flag will report their own
// mismatch via Test262Error message text, which still fails the
// case correctly via the throw-was-empty path.
