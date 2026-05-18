// P7.1 — built-in `ReferenceError` subclass (spec §20.5.5). Same
// injection path as TypeError; see check-typeerror-001 for the
// machinery rationale.
//
// Acceptance:
//   - new ReferenceError("msg").message / .name
//   - instanceof ReferenceError AND instanceof Error
//   - user subclass `extends ReferenceError` keeps chain + name
//   - throw + typed catch by ReferenceError and by Error parent

// 1. Direct instantiation.
const e1 = new ReferenceError("x is not defined");
console.log(e1.message);                   // x is not defined
console.log(e1.name);                      // ReferenceError
console.log(e1 instanceof ReferenceError); // true
console.log(e1 instanceof Error);          // true

// 2. User subclass of the builtin.
class ScopeError extends ReferenceError {
  symbol: string;
  constructor(msg: string, symbol: string) {
    super(msg);
    this.symbol = symbol;
  }
}
const e2 = new ScopeError("unresolved", "foo");
console.log(e2.message);                   // unresolved
console.log(e2.name);                      // ReferenceError (inherited)
console.log(e2.symbol);                    // foo
console.log(e2 instanceof ScopeError);     // true
console.log(e2 instanceof ReferenceError); // true
console.log(e2 instanceof Error);          // true

// 3. Throw + typed catch by the builtin subclass.
try {
  throw new ReferenceError("y before init");
} catch (err: ReferenceError) {
  console.log(err.message);                // y before init
  console.log(err.name);                  // ReferenceError
}

// 4. Throw + catch by the Error parent.
try {
  throw new ScopeError("missing", "bar");
} catch (err: Error) {
  console.log(err.message);                // missing
}
