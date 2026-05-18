// P7.1 — built-in `RangeError` subclass (spec §20.5.5). Same
// injection path as TypeError; see check-typeerror-001 for the
// machinery rationale.
//
// Acceptance:
//   - new RangeError("msg").message / .name
//   - instanceof RangeError AND instanceof Error
//   - user subclass `extends RangeError` keeps chain + name
//   - throw + typed catch by RangeError and by Error parent

// 1. Direct instantiation.
const e1 = new RangeError("out of bounds");
console.log(e1.message);              // out of bounds
console.log(e1.name);                 // RangeError
console.log(e1 instanceof RangeError); // true
console.log(e1 instanceof Error);     // true

// 2. User subclass of the builtin.
class IndexError extends RangeError {
  index: number;
  constructor(msg: string, index: number) {
    super(msg);
    this.index = index;
  }
}
const e2 = new IndexError("bad index", 99);
console.log(e2.message);               // bad index
console.log(e2.name);                  // RangeError (inherited)
console.log(e2.index);                 // 99
console.log(e2 instanceof IndexError); // true
console.log(e2 instanceof RangeError); // true
console.log(e2 instanceof Error);      // true

// 3. Throw + typed catch by the builtin subclass.
try {
  throw new RangeError("radix must be 2-36");
} catch (err: RangeError) {
  console.log(err.message);            // radix must be 2-36
  console.log(err.name);              // RangeError
}

// 4. Throw + catch by the Error parent.
try {
  throw new IndexError("oob", 5);
} catch (err: Error) {
  console.log(err.message);            // oob
}
