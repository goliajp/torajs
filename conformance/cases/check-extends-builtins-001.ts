// P4.6 — `class MyError extends Error` builtin-class extension.
// Spec §20.5: Error has `message` + `name` instance fields and a
// ctor taking one optional string argument. Pre-fix tora panicked
// at check.rs with "internal: `new Error` reached check.rs (desugar
// didn't run?)" because no `__new_Error` factory was synthesized.
//
// Implementation: AST pass `inject_builtin_classes` runs before
// desugar_classes and prepends a synthetic `class Error { message:
// string; name: string; constructor(message: string) { ... } }`
// when the AST references Error anywhere. From there the existing
// user-class machinery (desugar_classes + synthesize_class_globals)
// handles factory / ctor / instanceof / prototype-chain wiring,
// including multi-level extends and super-chain forwarding (the P4.5
// super() rewrite already passes __new_target through).
//
// The pass is idempotent (skipped when a user ClassDecl named Error
// already exists, allowing stdlib-rewrite overrides) and gated on
// usage detection (skipped when neither `new Error(...)` nor
// `extends Error` appears) to keep compile time neutral for programs
// that don't use Error.
//
// Acceptance:
//   - Direct `new Error("msg")` returns instance with .message set
//   - `class X extends Error` works with super(msg) → this.message
//   - Multi-level extends (A → B → C) preserves message all the way
//   - instanceof walks the full chain (X → Error)
//   - Custom fields added in subclasses coexist with .message
//   - Throw + catch (typed catch by subclass) reads custom fields
//   - Subset boundary: .stack / .toString format / new Error without
//     `new` keyword stay deferred (not part of P4.6 acceptance)

// 1. Direct instantiation.
const e1 = new Error("oops");
console.log(e1.message);                // oops
console.log(e1 instanceof Error);       // true

// 2. Single-level extends with explicit super().
class MyError extends Error {
  constructor(msg: string) {
    super(msg);
  }
}
const e2 = new MyError("boom");
console.log(e2.message);                // boom
console.log(e2 instanceof MyError);     // true
console.log(e2 instanceof Error);       // true

// 3. Subclass with custom field.
class AppError extends Error {
  code: number;
  constructor(msg: string, code: number) {
    super(msg);
    this.code = code;
  }
}
const e3 = new AppError("db down", 42);
console.log(e3.message);                // db down
console.log(e3.code);                   // 42
console.log(e3 instanceof AppError);    // true
console.log(e3 instanceof Error);       // true

// 4. Multi-level extends (Error → AppError → IoError).
class IoError extends AppError {
  path: string;
  constructor(msg: string, code: number, path: string) {
    super(msg, code);
    this.path = path;
  }
}
const e4 = new IoError("disk full", 17, "/var/log");
console.log(e4.message);                // disk full
console.log(e4.code);                   // 17
console.log(e4.path);                   // /var/log
console.log(e4 instanceof IoError);     // true
console.log(e4 instanceof AppError);    // true
console.log(e4 instanceof Error);       // true

// 5. Throw + typed catch by subclass.
try {
  throw new AppError("dial failed", 99);
} catch (err: AppError) {
  console.log(err.message);             // dial failed
  console.log(err.code);                // 99
}

// 6. Throw + catch by Error parent class.
try {
  throw new MyError("transient");
} catch (err: Error) {
  console.log(err.message);             // transient
}
